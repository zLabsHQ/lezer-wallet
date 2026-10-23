//! `lezer-provider` — the Basecamp-facing wallet provider, independent of the Logos SDK so it
//! can be unit-tested. The generated module shim (`modules/lezer_wallet_provider`) passes in
//! the caller identity the Logos host authenticated (`logos_rust_sdk::current_caller()`).
//!
//! Security model:
//! - Privileged wallet operations (`wallet_call`) are accepted only from the module named
//!   [`WALLET_UI_MODULE`]. Identity comes from the host's capability tokens, not from any
//!   argument, so another module cannot impersonate the wallet UI.
//! - A dApp session is bound to the module that opened it; a session token presented by a
//!   different module is rejected.
//! - Long operations (proving, inclusion polling) run on a background runtime; IPC calls
//!   return a `job_id` immediately and never block the module's event loop.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use lezer_adapter::{AccountKind, NetworkProfile};
use lezer_core::{
    ApproveDecision, ErrorCode, LezerError, LezerResult, RequestResolution, TxIntent, WalletEngine,
    WalletStateSnapshot, now_unix,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;

pub use lezer_core::default_lezer_home as default_home;

/// The only module allowed to call privileged wallet operations.
pub const WALLET_UI_MODULE: &str = "lezer_wallet_ui";

/// Host-authenticated identity of the module making the current call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Module(String),
    /// Not nameable (no token, host anchor, or no dispatch in flight). Fail-closed.
    Unknown,
}

impl Caller {
    fn module_name(&self) -> LezerResult<&str> {
        match self {
            Self::Module(n) if !n.is_empty() => Ok(n),
            _ => Err(LezerError::new(ErrorCode::AccountNotAuthorized, "caller module could not be authenticated")),
        }
    }

    fn is_wallet_ui(&self) -> bool {
        matches!(self, Self::Module(n) if n == WALLET_UI_MODULE)
    }
}

/// Privileged operations available to `lezer_wallet_ui`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WalletOp {
    Snapshot,
    Init { password: String },
    Restore { phrase: String, password: String, discover_depth: Option<u32> },
    Unlock { password: String },
    Lock,
    CreateAccount { kind: AccountKind, label: Option<String> },
    SetActive { account: String },
    ReceiveAddress { account: String },
    Balance { account: String },
    Sync,
    Send { from: String, to: String, amount: String },
    ApproveConnect { request_id: String, accounts: Vec<String>, allow_private_balances: Vec<String> },
    ApproveProposal { decision: ApproveDecision },
    Reject { request_id: String, reason: Option<String> },
    RevokeSession { session_id: String },
    Job { job_id: String },
    // ── fungible tokens (lezer_token program) — wallet UI only ──
    Tokens,
    TokenTrack { token_id: String },
    TokenInfo { token_id: String },
    TokenBalance { token_id: String, account: String },
    TokenDefine { definition: String, name: String, symbol: String, decimals: u8, supply: String, authority: Option<String>, payer: String },
    TokenMint { token_id: String, authority: String, to: String, amount: String, payer: String },
    TokenTransfer { token_id: String, from: String, to: String, amount: String, payer: String },
    TokenBurn { token_id: String, from: String, amount: String, payer: String },
    TokenSetAuthority { token_id: String, authority: String, new_authority: Option<String>, payer: String },
    // ── LP-0023 registry ──
    RegistryList,
    RegistryRegister { subject: String, name: String, source_url: String, commit: String, version: String, payer: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Running { description: String, started_unix: u64 },
    Done { result: Value },
    Failed { error: Value },
}

/// Events the shim forwards to the host (`approvalRequested`, `requestResolved`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: &'static str,
    pub request_id: String,
    pub payload: Value,
}

type JobFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = LezerResult<Value>> + Send + 'a>>;

pub struct Provider {
    engine: Arc<AsyncMutex<WalletEngine>>,
    snapshot: Arc<RwLock<WalletStateSnapshot>>,
    jobs: Arc<Mutex<BTreeMap<String, JobState>>>,
    /// request_id → module that created it (connect or proposal).
    origins: Mutex<BTreeMap<String, String>>,
    /// Provider-side resolution mirror for requests whose execution runs in the background.
    resolutions: Arc<Mutex<BTreeMap<String, Value>>>,
    pending_events: Arc<Mutex<Vec<Event>>>,
    next_job: Mutex<u64>,
    rt: tokio::runtime::Runtime,
}

fn ok(v: impl Serialize) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap_or_default()
}

fn err(e: &LezerError) -> Vec<u8> {
    ok(json!({ "error": e.to_envelope() }))
}

fn respond(r: LezerResult<Value>) -> Vec<u8> {
    r.map_or_else(|e| err(&e), ok)
}

fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8], what: &str) -> LezerResult<T> {
    serde_json::from_slice(bytes).map_err(|e| LezerError::new(ErrorCode::SimulationReverted, format!("malformed {what}: {e}")))
}

fn parse_amount(s: &str) -> LezerResult<u128> {
    s.trim()
        .parse::<u128>()
        .ok()
        .filter(|a| *a > 0)
        .ok_or_else(|| LezerError::new(ErrorCode::SimulationReverted, "amount must be a positive integer"))
}

impl Provider {
    /// `home`: the module's persistence directory (from the host context).
    pub fn new(home: PathBuf, network: Option<NetworkProfile>) -> Self {
        let engine = WalletEngine::new(&home, network);
        let snapshot = engine.snapshot();
        Self {
            engine: Arc::new(AsyncMutex::new(engine)),
            snapshot: Arc::new(RwLock::new(snapshot)),
            jobs: Arc::default(),
            origins: Mutex::default(),
            resolutions: Arc::default(),
            pending_events: Arc::default(),
            next_job: Mutex::new(0),
            rt: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("provider runtime"),
        }
    }

    /// Drain events produced since the last call (emitted by the shim on its own thread).
    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut *self.pending_events.lock().expect("events lock"))
    }

    fn push_event(&self, name: &'static str, request_id: &str, payload: Value) {
        self.pending_events.lock().expect("events lock").push(Event { name, request_id: request_id.to_string(), payload });
    }

    /// Run a short engine operation synchronously. Fails fast (`ProverQueueBusy`) if a
    /// background job currently holds the engine.
    fn with_engine<T>(&self, f: impl AsyncFnOnce(&mut WalletEngine) -> LezerResult<T>) -> LezerResult<T> {
        let mut guard = self.engine.try_lock().map_err(|_| {
            LezerError::new(ErrorCode::ProverQueueBusy, "the wallet is busy with another operation; retry shortly")
        })?;
        let result = self.rt.block_on(f(&mut guard));
        *self.snapshot.write().expect("snapshot lock") = guard.snapshot();
        result
    }

    fn snapshot(&self) -> WalletStateSnapshot {
        if let Ok(guard) = self.engine.try_lock() {
            let fresh = guard.snapshot();
            *self.snapshot.write().expect("snapshot lock") = fresh.clone();
            return fresh;
        }
        self.snapshot.read().expect("snapshot lock").clone()
    }

    fn spawn_job<F>(&self, description: String, request_id: Option<String>, f: F) -> String
    where
        F: for<'a> FnOnce(&'a mut WalletEngine) -> JobFuture<'a> + Send + 'static,
    {
        let job_id = {
            let mut n = self.next_job.lock().expect("job counter");
            *n += 1;
            format!("job_{n}")
        };
        self.jobs.lock().expect("jobs").insert(
            job_id.clone(),
            JobState::Running { description, started_unix: now_unix() },
        );
        if let Some(rid) = &request_id {
            self.resolutions.lock().expect("res").insert(rid.clone(), json!({ "status": "executing", "request_id": rid }));
        }
        let (engine, snapshot, jobs, resolutions, events, jid) = (
            self.engine.clone(),
            self.snapshot.clone(),
            self.jobs.clone(),
            self.resolutions.clone(),
            self.pending_events.clone(),
            job_id.clone(),
        );
        self.rt.spawn(async move {
            let mut guard = engine.lock().await;
            let outcome = f(&mut guard).await;
            *snapshot.write().expect("snapshot lock") = guard.snapshot();
            let resolution = guard.broker().get_resolution(request_id.as_deref().unwrap_or("")).cloned();
            drop(guard);
            let state = match outcome {
                Ok(result) => JobState::Done { result },
                Err(e) => JobState::Failed { error: json!(e.to_envelope()) },
            };
            if let Some(rid) = request_id {
                let payload = resolution.map_or_else(|| json!(state), |r| json!(r));
                resolutions.lock().expect("res").insert(rid.clone(), payload.clone());
                events.lock().expect("events").push(Event { name: "requestResolved", request_id: rid, payload });
            }
            jobs.lock().expect("jobs").insert(jid, state);
        });
        job_id
    }

    // ── Public discovery ───────────────────────────────────────────────────

    /// No account data: only network and lock state.
    pub fn get_status(&self) -> Vec<u8> {
        let s = self.snapshot();
        ok(json!({
            "network_id": s.network_id,
            "sequencer_url": s.sequencer_url,
            "lock_state": s.lock_state,
            "busy": s.in_flight.is_some(),
        }))
    }

    // ── dApp surface ───────────────────────────────────────────────────────

    pub fn connect(&self, caller: &Caller, dapp_info: &[u8]) -> Vec<u8> {
        respond((|| {
            let origin = caller.module_name()?.to_string();
            if caller.is_wallet_ui() {
                return Err(LezerError::new(ErrorCode::AccountNotAuthorized, "the wallet UI does not connect as a dApp"));
            }
            let info: Value = serde_json::from_slice(dapp_info).unwrap_or_else(|_| json!({}));
            let display = info["displayName"].as_str().unwrap_or(&origin).chars().take(64).collect::<String>();
            let wants_private = info["requestPrivateBalanceRead"].as_bool().unwrap_or(false);
            let req = self.with_engine(async |w| Ok(w.queue_connect(&display, &origin, wants_private)))?;
            self.origins.lock().expect("origins").insert(req.request_id.clone(), origin.clone());
            self.push_event(
                "approvalRequested",
                &req.request_id,
                json!({ "kind": "connect", "dapp_origin": origin, "dapp_name": display, "wants_private_balance": wants_private }),
            );
            Ok(json!({ "request_id": req.request_id, "status": "pending" }))
        })())
    }

    fn check_origin(&self, caller: &Caller, request_id: &str) -> LezerResult<()> {
        let owner = self.origins.lock().expect("origins").get(request_id).cloned();
        match owner {
            Some(o) if caller.is_wallet_ui() || caller.module_name().ok() == Some(o.as_str()) => Ok(()),
            _ => Err(LezerError::new(ErrorCode::ProposalExpired, format!("unknown request '{request_id}'"))),
        }
    }

    pub fn get_request_status(&self, caller: &Caller, request_id: &str) -> Vec<u8> {
        respond((|| {
            self.check_origin(caller, request_id)?;
            if let Some(v) = self.resolutions.lock().expect("res").get(request_id) {
                return Ok(v.clone());
            }
            match self.engine.try_lock() {
                Ok(guard) => guard
                    .broker()
                    .get_resolution(request_id)
                    .map(|r| json!(r))
                    .ok_or_else(|| LezerError::new(ErrorCode::ProposalExpired, format!("unknown request '{request_id}'"))),
                Err(_) => Ok(json!({ "status": "pending", "request_id": request_id })),
            }
        })())
    }

    fn session_for(&self, caller: &Caller, token: &str) -> LezerResult<lezer_core::SessionGrant> {
        let origin = caller.module_name()?;
        let grant = self.with_engine(async |w| w.session(token))?;
        if grant.dapp_origin != origin {
            return Err(LezerError::new(ErrorCode::SessionExpiredOrInvalid, "session belongs to a different module"));
        }
        Ok(grant)
    }

    pub fn disconnect(&self, caller: &Caller, token: &str) -> Vec<u8> {
        respond((|| {
            self.session_for(caller, token)?;
            self.with_engine(async |w| w.revoke_session(token))?;
            Ok(json!({ "status": "disconnected" }))
        })())
    }

    pub fn get_accounts(&self, caller: &Caller, token: &str) -> Vec<u8> {
        respond(self.session_for(caller, token).map(|g| json!({ "accounts": g.authorized_accounts })))
    }

    pub fn get_balance(&self, caller: &Caller, token: &str, account_id: &str) -> Vec<u8> {
        respond((|| {
            self.session_for(caller, token)?;
            let b = self.with_engine(async |w| w.dapp_balance(token, account_id).await)?;
            Ok(json!(b))
        })())
    }

    pub fn get_receive_address(&self, caller: &Caller, token: &str, account_id: &str) -> Vec<u8> {
        respond((|| {
            let grant = self.session_for(caller, token)?;
            let allowed = grant.scopes.iter().any(|s| {
                matches!(s, lezer_core::PermissionScope::ReadPrivateReceiveBundle { account_id: a } if a == account_id)
            });
            if !allowed {
                return Err(LezerError::new(ErrorCode::AccountNotAuthorized, "account not shared with this session"));
            }
            let keys = self.with_engine(async |w| w.receive_keys(account_id))?;
            Ok(json!({ "account_id": keys.account_id, "receive_address": keys.receive_address }))
        })())
    }

    pub fn propose_transaction(&self, caller: &Caller, token: &str, intent_json: &[u8]) -> Vec<u8> {
        respond((|| {
            let origin = caller.module_name()?.to_string();
            self.session_for(caller, token)?;
            let intent: TxIntent = parse(intent_json, "transaction intent")?;
            let proposal = self.with_engine(async |w| w.propose(token, intent).await)?;
            self.origins.lock().expect("origins").insert(proposal.proposal_id.clone(), origin);
            self.push_event("approvalRequested", &proposal.proposal_id, json!({ "kind": "proposal", "summary": proposal.summary }));
            Ok(json!({ "request_id": proposal.proposal_id, "status": "pending", "summary": proposal.summary }))
        })())
    }

    // ── Privileged wallet UI surface ───────────────────────────────────────

    pub fn wallet_call(&self, caller: &Caller, op_json: &[u8]) -> Vec<u8> {
        respond((|| {
            if !caller.is_wallet_ui() {
                return Err(LezerError::new(ErrorCode::UiControlTokenInvalid, format!("only {WALLET_UI_MODULE} may call wallet operations")));
            }
            let op: WalletOp = parse(op_json, "wallet operation")?;
            self.dispatch(op)
        })())
    }

    fn dispatch(&self, op: WalletOp) -> LezerResult<Value> {
        match op {
            WalletOp::Snapshot => Ok(json!(self.snapshot())),
            WalletOp::Init { password } => self.with_engine(async |w| w.init(&password).map(|r| json!(r))),
            WalletOp::Restore { phrase, password, discover_depth } => {
                Ok(json!({ "job_id": self.spawn_job("restore wallet".into(), None, move |w| Box::pin(async move {
                    w.restore(&phrase, &password, discover_depth).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::Unlock { password } => self.with_engine(async |w| w.unlock(&password).map(|()| json!(w.snapshot()))),
            WalletOp::Lock => self.with_engine(async |w| Ok(json!({ "lock_state": w.lock() }))),
            WalletOp::CreateAccount { kind, label } => {
                self.with_engine(async |w| w.create_account(kind, label.as_deref()).map(|a| json!(a)))
            }
            WalletOp::SetActive { account } => self.with_engine(async |w| w.set_active_account(&account).map(|id| json!({ "active_account_id": id }))),
            WalletOp::ReceiveAddress { account } => self.with_engine(async |w| w.receive_keys(&account).map(|k| json!(k))),
            WalletOp::Balance { account } => self.with_engine(async |w| w.balance(&account).await.map(|b| json!(b))),
            WalletOp::Sync => Ok(json!({ "job_id": self.spawn_job("sync private accounts".into(), None, |w| Box::pin(async move {
                w.sync().await.map(|b| json!({ "synced_block": b }))
            })) })),
            WalletOp::Send { from, to, amount } => {
                let amount = parse_amount(&amount)?;
                Ok(json!({ "job_id": self.spawn_job(format!("send {amount}"), None, move |w| Box::pin(async move {
                    w.transfer(&from, &to, amount).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::ApproveConnect { request_id, accounts, allow_private_balances } => {
                let grant = self.with_engine(async |w| w.approve_connect(&request_id, accounts, &allow_private_balances))?;
                let payload = json!(RequestResolution::ApprovedConnect {
                    request_id: request_id.clone(),
                    session_token: grant.session_token.clone(),
                    authorized_accounts: grant.authorized_accounts.clone(),
                });
                self.resolutions.lock().expect("res").insert(request_id.clone(), payload.clone());
                self.push_event("requestResolved", &request_id, json!({ "status": "approved_connect", "request_id": request_id }));
                Ok(json!({ "status": "approved", "dapp_origin": grant.dapp_origin, "accounts": grant.authorized_accounts }))
            }
            WalletOp::ApproveProposal { decision } => {
                let rid = decision.proposal_id.clone();
                Ok(json!({ "job_id": self.spawn_job(format!("execute {rid}"), Some(rid), move |w| Box::pin(async move {
                    w.approve_proposal(&decision).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::Reject { request_id, reason } => {
                let res = self.with_engine(async |w| w.reject_request(&request_id, reason.as_deref()))?;
                let payload = json!(res);
                self.resolutions.lock().expect("res").insert(request_id.clone(), payload.clone());
                self.push_event("requestResolved", &request_id, payload.clone());
                Ok(payload)
            }
            WalletOp::RevokeSession { session_id } => {
                self.with_engine(async |w| w.revoke_session_by_id(&session_id).map(|()| json!({ "status": "revoked" })))
            }
            WalletOp::Tokens => self.with_engine(async |w| {
                Ok(json!({ "token_ids": w.tracked_tokens(), "token_program": w.token_program() }))
            }),
            WalletOp::TokenTrack { token_id } => {
                self.with_engine(async |w| w.track_token_id(&token_id).map(|()| json!({ "status": "tracked" })))
            }
            WalletOp::TokenInfo { token_id } => self.with_engine(async |w| {
                let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                w.token_info(&program, &token_id).await.map(|i| json!(i))
            }),
            WalletOp::TokenBalance { token_id, account } => self.with_engine(async |w| {
                let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                w.token_balance(&program, &token_id, &account).await.map(|b| json!({ "balance": b.to_string() }))
            }),
            WalletOp::TokenDefine { definition, name, symbol, decimals, supply, authority, payer } => {
                let supply = parse_amount(&supply)?;
                Ok(json!({ "job_id": self.spawn_job(format!("define token {symbol}"), None, move |w| Box::pin(async move {
                    let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                    w.token_define(&program, &definition, &payer, &name, &symbol, decimals, authority.as_deref(), supply).await.map(|i| json!(i))
                })) }))
            }
            WalletOp::TokenMint { token_id, authority, to, amount, payer } => {
                let amount = parse_amount(&amount)?;
                Ok(json!({ "job_id": self.spawn_job("token mint".into(), None, move |w| Box::pin(async move {
                    let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                    w.token_mint(&program, &token_id, &authority, &to, &payer, amount).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::TokenTransfer { token_id, from, to, amount, payer } => {
                let amount = parse_amount(&amount)?;
                Ok(json!({ "job_id": self.spawn_job("token transfer".into(), None, move |w| Box::pin(async move {
                    let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                    w.token_transfer(&program, &token_id, &from, &to, &payer, amount).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::TokenBurn { token_id, from, amount, payer } => {
                let amount = parse_amount(&amount)?;
                Ok(json!({ "job_id": self.spawn_job("token burn".into(), None, move |w| Box::pin(async move {
                    let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                    w.token_burn(&program, &token_id, &from, &payer, amount).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::TokenSetAuthority { token_id, authority, new_authority, payer } => {
                Ok(json!({ "job_id": self.spawn_job("token set-authority".into(), None, move |w| Box::pin(async move {
                    let program = w.token_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_token deployment")))?;
                    w.token_set_authority(&program, &token_id, &authority, &payer, new_authority.as_deref()).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::RegistryList => self.with_engine(async |w| {
                let program = w.registry_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_registry deployment")))?;
                w.list_registry(&program).await.map(|r| json!(r))
            }),
            WalletOp::RegistryRegister { subject, name, source_url, commit, version, payer } => {
                Ok(json!({ "job_id": self.spawn_job(format!("register {subject}"), None, move |w| Box::pin(async move {
                    let program = w.registry_program().ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, String::from("no lezer_registry deployment")))?;
                    w.register_program(&program, &subject, &name, &source_url, &commit, &version, &payer).await.map(|r| json!(r))
                })) }))
            }
            WalletOp::Job { job_id } => self
                .jobs
                .lock()
                .expect("jobs")
                .get(&job_id)
                .map(|j| json!(j))
                .ok_or_else(|| LezerError::new(ErrorCode::ProposalExpired, format!("unknown job '{job_id}'"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> (Provider, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Provider::new(dir.path().to_path_buf(), Some(NetworkProfile::localnet())), dir)
    }

    fn ui() -> Caller {
        Caller::Module(WALLET_UI_MODULE.into())
    }

    fn call(p: &Provider, c: &Caller, op: Value) -> Value {
        serde_json::from_slice(&p.wallet_call(c, op.to_string().as_bytes())).unwrap()
    }

    #[test]
    fn only_wallet_ui_can_call_privileged_ops() {
        let (p, _d) = provider();
        for c in [Caller::Unknown, Caller::Module("evil_dapp".into()), Caller::Module("".into())] {
            let r = call(&p, &c, json!({ "op": "init", "password": "password123" }));
            assert_eq!(r["error"]["code"], "LZR-2004", "caller {c:?} must be rejected");
        }
        let r = call(&p, &ui(), json!({ "op": "init", "password": "password123" }));
        assert!(r["recovery_phrase"].is_string());
    }

    #[test]
    fn dapp_flow_sessions_are_bound_to_the_connecting_module() {
        let (p, _d) = provider();
        let init = call(&p, &ui(), json!({ "op": "init", "password": "password123" }));
        let main = init["accounts"].as_array().unwrap().iter().find(|a| a["labels"][0] == "main").unwrap()["account_id"].as_str().unwrap().to_string();
        let private = init["accounts"].as_array().unwrap().iter().find(|a| a["labels"][0] == "private").unwrap()["account_id"].as_str().unwrap().to_string();

        let dapp = Caller::Module("testimonial_dapp".into());
        let other = Caller::Module("other_dapp".into());
        let conn: Value = serde_json::from_slice(&p.connect(&dapp, br#"{"displayName":"Testimonials"}"#)).unwrap();
        let rid = conn["request_id"].as_str().unwrap().to_string();
        assert_eq!(p.take_events()[0].name, "approvalRequested");

        // Another module can't read the request.
        let peek: Value = serde_json::from_slice(&p.get_request_status(&other, &rid)).unwrap();
        assert!(peek["error"].is_object());

        call(&p, &ui(), json!({ "op": "approve_connect", "request_id": rid, "accounts": [main, private], "allow_private_balances": [] }));
        let st: Value = serde_json::from_slice(&p.get_request_status(&dapp, &rid)).unwrap();
        let token = st["session_token"].as_str().unwrap().to_string();

        let accts: Value = serde_json::from_slice(&p.get_accounts(&dapp, &token)).unwrap();
        assert_eq!(accts["accounts"].as_array().unwrap().len(), 2);

        // Stolen token is useless to another module.
        let stolen: Value = serde_json::from_slice(&p.get_accounts(&other, &token)).unwrap();
        assert_eq!(stolen["error"]["code"], "LZR-2001");

        // Private balance gated without explicit grant.
        let priv_bal: Value = serde_json::from_slice(&p.get_balance(&dapp, &token, &private)).unwrap();
        assert_eq!(priv_bal["error"]["code"], "LZR-2003");

        // Proposal yields a digest-bound summary; tampered decision rejected.
        let intent = json!({ "kind": "native_transfer", "from": main, "to": private, "amount": "5" });
        let prop: Value = serde_json::from_slice(&p.propose_transaction(&dapp, &token, intent.to_string().as_bytes())).unwrap();
        let pid = prop["request_id"].as_str().unwrap().to_string();
        assert_eq!(prop["summary"]["privacy_mode"], "shielded");
        let bad = call(&p, &ui(), json!({ "op": "approve_proposal", "decision": {
            "proposal_id": pid, "summary_digest_hex": "00", "acknowledged_unverified": false, "critical_override": false } }));
        let job = bad["job_id"].as_str().unwrap().to_string();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let status = call(&p, &ui(), json!({ "op": "job", "job_id": job }));
        assert_eq!(status["state"], "failed");
        assert_eq!(status["error"]["code"], "LZR-3005");
    }

    #[test]
    fn status_reveals_no_accounts() {
        let (p, _d) = provider();
        call(&p, &ui(), json!({ "op": "init", "password": "password123" }));
        let s: Value = serde_json::from_slice(&p.get_status()).unwrap();
        assert!(s.get("accounts").is_none());
        assert_eq!(s["lock_state"], "unlocked");
    }
}
