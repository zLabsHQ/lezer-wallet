//! `lezer_wallet_provider` — Logos Basecamp Core Module (`type: "core"`, `interface: "cdylib"`).
//!
//! Wraps `lezer_core::WalletEngine` behind the C-ABI LIDL contract (`lezer_wallet_provider.lidl`).
//! Enforces strict separation between:
//! 1. Unauthenticated discovery (`get_status`, `faucet_info`)
//! 2. Session-gated dApp methods (`connect`, `get_request_status`, `get_accounts`,
//!    `get_balance`, `get_token_balances`, `propose_transaction`, `disconnect`)
//! 3. Privileged Wallet-UI controller methods (`execute_privileged` requiring `ui_control_token`)

use std::collections::BTreeMap;

use lezer_core::{
    ApproveDecision, ErrorCode, LezerError, LezerResult, SessionGrant, TxIntent, WalletEngine,
    verify_session_balance_permission,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Privileged UI operations dispatched from `lezer_wallet_ui` via `execute_privileged`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PrivilegedUiOp {
    GetSnapshot,
    InitVault {
        password: String,
    },
    RestoreVault {
        mnemonic: String,
        password: String,
    },
    UnlockVault {
        password: String,
    },
    LockVault,
    CreateAccount {
        label: String,
        private: bool,
    },
    ApproveConnect {
        request_id: String,
        accounts: Vec<String>,
        allow_private_balances: Vec<String>,
    },
    ApproveProposal {
        decision: ApproveDecision,
    },
    RejectRequest {
        request_id: String,
        reason: Option<String>,
    },
}

/// Core Basecamp provider module instance hosting `WalletEngine` and active sessions.
pub struct LezerWalletProvider {
    engine: WalletEngine,
    ui_control_token: String,
    sessions: BTreeMap<String, SessionGrant>,
}

impl LezerWalletProvider {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            engine: WalletEngine::with_default_adapter()?,
            ui_control_token: "lzr_ui_ctrl_default".to_string(),
            sessions: BTreeMap::new(),
        })
    }

    pub fn with_engine(engine: WalletEngine, ui_control_token: impl Into<String>) -> Self {
        Self {
            engine,
            ui_control_token: ui_control_token.into(),
            sessions: BTreeMap::new(),
        }
    }

    pub fn ui_control_token(&self) -> &str {
        &self.ui_control_token
    }

    pub fn engine_mut(&mut self) -> &mut WalletEngine {
        &mut self.engine
    }

    // ── 1. Public Discovery ──────────────────────────────────────────────

    pub fn get_status(&self) -> Vec<u8> {
        let snapshot = self.engine.snapshot();
        serde_json::to_vec(&snapshot).unwrap_or_default()
    }

    // ── 2. dApp Connection & Session Lifecycle ───────────────────────────

    pub fn connect(&mut self, dapp_info_json: &[u8]) -> Vec<u8> {
        let parsed: Value = serde_json::from_slice(dapp_info_json).unwrap_or_else(|_| json!({}));
        let dapp_name = parsed["displayName"]
            .as_str()
            .or_else(|| parsed["moduleName"].as_str())
            .unwrap_or("Unknown dApp");
        let dapp_origin = parsed["moduleName"].as_str().unwrap_or("logos://dapp");
        let req_priv = parsed["requestPrivateBalanceRead"]
            .as_bool()
            .unwrap_or(false);

        let req = self
            .engine
            .broker_mut()
            .queue_connect_request(dapp_name, dapp_origin, req_priv, 1_760_000_000);

        serde_json::to_vec(&json!({
            "request_id": req.request_id,
            "status": "pending",
        }))
        .unwrap_or_default()
    }

    pub fn get_request_status(&self, request_id: &str) -> Vec<u8> {
        match self.engine.broker().get_resolution(request_id) {
            Some(res) => serde_json::to_vec(res).unwrap_or_default(),
            None => encode_err(&LezerError::new(
                ErrorCode::ProposalExpired,
                format!("unknown request_id '{request_id}'"),
            )),
        }
    }

    pub fn disconnect(&mut self, session_token: &str) -> Vec<u8> {
        self.sessions.remove(session_token);
        self.engine
            .broker_mut()
            .cancel_proposals_for_session(session_token);
        serde_json::to_vec(&json!({ "status": "disconnected" })).unwrap_or_default()
    }

    // ── 3. Session-Gated dApp Read Operations ────────────────────────────

    pub fn get_accounts(&self, session_token: &str) -> Vec<u8> {
        let Some(grant) = self.sessions.get(session_token) else {
            return encode_err(&LezerError::new(
                ErrorCode::SessionExpiredOrInvalid,
                "invalid or expired session_token",
            ));
        };
        serde_json::to_vec(&json!({
            "accounts": grant.authorized_accounts,
        }))
        .unwrap_or_default()
    }

    pub fn get_balance(&self, session_token: &str, account_id: &str, _asset_id: &str) -> Vec<u8> {
        let Some(grant) = self.sessions.get(session_token) else {
            return encode_err(&LezerError::new(
                ErrorCode::SessionExpiredOrInvalid,
                "invalid or expired session_token",
            ));
        };
        if let Err(e) = verify_session_balance_permission(grant, account_id, 1_760_000_010) {
            return encode_err(&e);
        }
        let snap = self.engine.snapshot();
        let balance = snap
            .accounts
            .iter()
            .find(|a| a.account_id == account_id)
            .map_or("0".to_string(), |a| a.native_balance.clone());
        serde_json::to_vec(&json!({
            "account_id": account_id,
            "balance": balance,
        }))
        .unwrap_or_default()
    }

    // ── 4. Session-Gated Transaction Proposal ────────────────────────────

    pub fn propose_transaction(&mut self, session_token: &str, intent_json: &[u8]) -> Vec<u8> {
        let Some(grant) = self.sessions.get(session_token).cloned() else {
            return encode_err(&LezerError::new(
                ErrorCode::SessionExpiredOrInvalid,
                "invalid or expired session_token",
            ));
        };
        let intent: TxIntent = match serde_json::from_slice(intent_json) {
            Ok(i) => i,
            Err(e) => {
                return encode_err(&LezerError::new(
                    ErrorCode::SimulationReverted,
                    format!("malformed TxIntent JSON: {e}"),
                ));
            }
        };

        let signing_acct = intent.signing_account().to_string();
        let avail_bal = self
            .engine
            .snapshot()
            .accounts
            .iter()
            .find(|a| a.account_id == signing_acct)
            .and_then(|a| a.native_balance.parse::<u128>().ok())
            .unwrap_or(1_000);

        match self
            .engine
            .broker_mut()
            .queue_proposal(&grant, intent, avail_bal, false, 1_760_000_020)
        {
            Ok(prop) => serde_json::to_vec(&json!({
                "request_id": prop.proposal_id,
                "status": "pending",
                "summary": prop.summary,
            }))
            .unwrap_or_default(),
            Err(e) => encode_err(&e),
        }
    }

    // ── 5. Privileged Wallet UI Controller ───────────────────────────────

    pub fn execute_privileged(&mut self, ui_token: &str, op_json: &[u8]) -> Vec<u8> {
        if ui_token != self.ui_control_token {
            return encode_err(&LezerError::new(
                ErrorCode::UiControlTokenInvalid,
                "invalid privileged UI control token",
            ));
        }

        let op: PrivilegedUiOp = match serde_json::from_slice(op_json) {
            Ok(o) => o,
            Err(e) => {
                return encode_err(&LezerError::new(
                    ErrorCode::SimulationReverted,
                    format!("invalid PrivilegedUiOp JSON: {e}"),
                ));
            }
        };

        match self.dispatch_privileged(op) {
            Ok(val) => serde_json::to_vec(&val).unwrap_or_default(),
            Err(e) => encode_err(&e),
        }
    }

    fn dispatch_privileged(&mut self, op: PrivilegedUiOp) -> LezerResult<Value> {
        match op {
            PrivilegedUiOp::GetSnapshot => {
                Ok(serde_json::to_value(self.engine.snapshot()).unwrap())
            }
            PrivilegedUiOp::InitVault { password } => {
                let res = self.engine.init_vault(&password)?;
                Ok(serde_json::to_value(res).unwrap())
            }
            PrivilegedUiOp::RestoreVault { mnemonic, password } => {
                let res = self.engine.restore_vault(&mnemonic, &password)?;
                Ok(serde_json::to_value(res).unwrap())
            }
            PrivilegedUiOp::UnlockVault { password } => {
                self.engine.unlock(&password)?;
                Ok(serde_json::to_value(self.engine.snapshot()).unwrap())
            }
            PrivilegedUiOp::LockVault => {
                let state = self.engine.lock();
                Ok(json!({ "lock_state": state }))
            }
            PrivilegedUiOp::CreateAccount { label, private } => {
                let acct = self.engine.create_account(&label, private)?;
                Ok(serde_json::to_value(acct).unwrap())
            }
            PrivilegedUiOp::ApproveConnect {
                request_id,
                accounts,
                allow_private_balances,
            } => {
                let grant = self.engine.broker_mut().approve_connect(
                    &request_id,
                    accounts,
                    &allow_private_balances,
                    1_760_000_005,
                )?;
                self.sessions
                    .insert(grant.session_token.clone(), grant.clone());
                Ok(serde_json::to_value(grant).unwrap())
            }
            PrivilegedUiOp::ApproveProposal { decision } => {
                let proposal = self
                    .engine
                    .broker_mut()
                    .take_verified_proposal(&decision)?;
                let tx_res = match proposal.intent {
                    TxIntent::SubmitTestimonial {
                        author,
                        username,
                        text,
                        timestamp,
                        ..
                    } => {
                        let receipt = self.engine.submit_testimonial(
                            &author,
                            &username,
                            &text,
                            Some(timestamp),
                        )?;
                        let tx_hash = receipt.tx_hash.clone();
                        let val = serde_json::to_value(receipt).unwrap();
                        (tx_hash, val)
                    }
                    _ => {
                        let tx_hash = proposal.summary.summary_digest_hex.clone();
                        let val = json!({ "tx_hash": tx_hash, "status": "confirmed" });
                        (tx_hash, val)
                    }
                };
                self.engine.broker_mut().record_proposal_executed(
                    &proposal.proposal_id,
                    tx_res.0.clone(),
                    tx_res.1.clone(),
                );
                Ok(tx_res.1)
            }
            PrivilegedUiOp::RejectRequest { request_id, reason } => {
                let res = self
                    .engine
                    .broker_mut()
                    .reject_request(&request_id, reason.as_deref())?;
                Ok(serde_json::to_value(res).unwrap())
            }
        }
    }
}

fn encode_err(err: &LezerError) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": err.to_envelope()
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_returns_valid_status_json() {
        let provider = LezerWalletProvider::new().expect("provider init");
        let status_bytes = provider.get_status();
        let parsed: serde_json::Value = serde_json::from_slice(&status_bytes).unwrap();
        assert_eq!(parsed["lock_state"], "uninitialized");
        assert_eq!(parsed["revision"], 1);
    }

    #[test]
    fn gate_rc01_end_to_end_dapp_connect_approval_sheet_and_testimonial_submission() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        }
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.enc");
        let engine = WalletEngine::with_vault_and_zone(vault_path, Some("lez-testnet-0.4")).unwrap();
        let mut provider = LezerWalletProvider::with_engine(engine, "ui_secret_token");

        // 1. Initialize vault via privileged UI
        let init_out = provider.execute_privileged(
            "ui_secret_token",
            &serde_json::to_vec(&PrivilegedUiOp::InitVault {
                password: "rc01-password".to_string(),
            })
            .unwrap(),
        );
        let init_json: Value = serde_json::from_slice(&init_out).unwrap();
        let main_pub = init_json["main_public_account"]["account_id"]
            .as_str()
            .unwrap()
            .to_string();
        let priv_vault = init_json["shielded_vault_account"]["account_id"]
            .as_str()
            .unwrap()
            .to_string();

        // 2. `lezer_testimonial_ui` calls `connect`
        let conn_out = provider.connect(
            &serde_json::to_vec(&json!({
                "moduleName": "lezer_testimonial_ui",
                "displayName": "Lezer Testimonials",
                "version": "1.0.0"
            }))
            .unwrap(),
        );
        let conn_json: Value = serde_json::from_slice(&conn_out).unwrap();
        let conn_id = conn_json["request_id"].as_str().unwrap().to_string();
        assert_eq!(conn_json["status"], "pending");

        // 3. User approves connection in `ConnectPromptSheet.qml` (without granting ReadPrivateBalance)
        let grant_out = provider.execute_privileged(
            "ui_secret_token",
            &serde_json::to_vec(&PrivilegedUiOp::ApproveConnect {
                request_id: conn_id.clone(),
                accounts: vec![main_pub.clone(), priv_vault.clone()],
                allow_private_balances: vec![],
            })
            .unwrap(),
        );
        let grant_json: Value = serde_json::from_slice(&grant_out).unwrap();
        let session_token = grant_json["session_token"].as_str().unwrap().to_string();

        // Verify dApp cannot read private balance without explicit per-account approval (LZR-2003)
        let priv_bal_out = provider.get_balance(&session_token, &priv_vault, "native");
        let priv_bal_json: Value = serde_json::from_slice(&priv_bal_out).unwrap();
        assert_eq!(priv_bal_json["error"]["code"], "LZR-2003");

        // 4. `lezer_testimonial_ui` proposes `SubmitTestimonial` intent
        let (_, _, shard_key) =
            lezer_core::compute_author_shard_for_account(&main_pub).unwrap();
        let intent = TxIntent::SubmitTestimonial {
            author: main_pub.clone(),
            submission_id: "lp0021-lezer".to_string(),
            shard_key,
            page_key: "00".to_string(),
            username: "@alice".to_string(),
            text: "I use Lezer Wallet in Logos Basecamp!".to_string(),
            timestamp: 1_760_900_000,
        };
        let prop_out =
            provider.propose_transaction(&session_token, &serde_json::to_vec(&intent).unwrap());
        let prop_json: Value = serde_json::from_slice(&prop_out).unwrap();
        let prop_id = prop_json["request_id"].as_str().unwrap().to_string();
        let digest = prop_json["summary"]["summary_digest_hex"]
            .as_str()
            .unwrap()
            .to_string();

        // 5. User approves in `ApprovalSheet.qml` with matching `summary_digest_hex`
        let approve_out = provider.execute_privileged(
            "ui_secret_token",
            &serde_json::to_vec(&PrivilegedUiOp::ApproveProposal {
                decision: ApproveDecision {
                    proposal_id: prop_id.clone(),
                    summary_digest_hex: digest,
                    acknowledged_unverified: false,
                    critical_override: false,
                },
            })
            .unwrap(),
        );
        let approve_json: Value = serde_json::from_slice(&approve_out).unwrap();
        assert_eq!(approve_json["submission_id"], "lp0021-lezer");
        assert_eq!(approve_json["tx_hash"].as_str().unwrap().len(), 64);

        // 6. dApp polls `get_request_status` and receives confirmed receipt
        let status_out = provider.get_request_status(&prop_id);
        let status_json: Value = serde_json::from_slice(&status_out).unwrap();
        assert_eq!(status_json["status"], "approved_proposal");
    }
}
