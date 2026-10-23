//! `lezer-core` — wallet state owner: encrypted vault, auto-lock, sessions, approvals,
//! and the canonical snapshot. All chain work goes through `lezer_adapter::LezEngine`
//! (official upstream `WalletCore`); `lezer-core` never touches keys or tx encoding.

pub mod autolock;
pub mod broker;
pub mod cache;
pub mod errors;
pub mod intent;
pub mod logging;
pub mod state;
pub mod summary;
pub mod vault;

use std::path::{Path, PathBuf};
use std::time::Instant;

use lezer_adapter::{
    AccountKind, ChainStatus, DeployedProgram, LezEngine, NetworkProfile, OwnedAccount,
    PrivateReceiveKeys, TxReceipt, TxStatus, parse_target,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zeroize::Zeroizing;

pub use autolock::{AutoLockFsm, DEFAULT_AUTOLOCK_TIMEOUT, LockState};
pub use broker::{
    ApprovalBroker, DEFAULT_SESSION_TTL_SECS, DappConnectRequest, PendingProposal,
    RequestResolution, verify_session_balance_permission,
};
pub use cache::{CURRENT_CACHE_SCHEMA_VERSION, Cache};
pub use errors::{ErrorCategory, ErrorCode, ErrorEnvelope, LezerError, LezerResult};
pub use intent::TxIntent;
pub use logging::{AuditLogger, Redacted};
pub use state::{
    AccountSummaryView, ActivityItemView, InFlightView, PendingRequestView, SessionSummaryView,
    WalletStateSnapshot,
};
pub use summary::{
    ApproveDecision, Precheck, SecurityWarning, SourceVerificationBadge, TransactionSummary,
    VerificationBadgeState, WarningSeverity,
};
pub use vault::{
    CURRENT_VAULT_SCHEMA_VERSION, PermissionScope, SealedVaultEnvelopeV1, SessionGrant,
    TxJournalEntry, VaultKey, WalletStateV2,
};

/// Returned once by `init`/`restore`. The recovery phrase is never stored by Lezer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub vault_path: String,
    pub network_id: String,
    /// Present only for `init` (a restore already knows its phrase).
    pub recovery_phrase: Option<String>,
    pub accounts: Vec<OwnedAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceView {
    pub account_id: String,
    pub kind: AccountKind,
    pub balance: String,
    /// `live` (read from sequencer now), `cached` (offline, from cache.json), or
    /// `local` (private: decrypted local state as of `block_id`).
    pub source: String,
    pub block_id: Option<u64>,
}

pub fn default_lezer_home() -> PathBuf {
    if let Ok(custom) = std::env::var("LEZER_HOME")
        && !custom.trim().is_empty()
    {
        return PathBuf::from(custom);
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map_or_else(|| PathBuf::from(".lezer"), |h| PathBuf::from(h).join(".lezer"))
}

pub fn session_id(token: &str) -> String {
    use sha2::{Digest as _, Sha256};
    hex::encode(&Sha256::digest(token.as_bytes())[..8])
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn chain_err(code: ErrorCode, e: &anyhow::Error) -> LezerError {
    let msg = format!("{e:#}");
    let lower = msg.to_lowercase();
    let code = if lower.contains("insufficient") || lower.contains("can not pay") {
        ErrorCode::SimulationReverted
    } else if lower.contains("connection")
        || lower.contains("timed out")
        || lower.contains("error sending request")
        || lower.contains("failed to get data from sequencer")
    {
        ErrorCode::SequencerUnreachable
    } else {
        code
    };
    LezerError::new(code, msg)
}

struct Unlocked {
    engine: LezEngine,
    state: WalletStateV2,
    key: VaultKey,
}

pub struct WalletEngine {
    vault_path: PathBuf,
    cache_path: PathBuf,
    profile_override: Option<NetworkProfile>,
    logger: AuditLogger,
    autolock: AutoLockFsm,
    broker: ApprovalBroker,
    unlocked: Option<Unlocked>,
    cache: Cache,
    revision: u64,
    in_flight: Option<InFlightView>,
}

impl WalletEngine {
    /// `profile_override`: network to use; `None` means "the network recorded in the vault"
    /// (or `LEZ_SEQUENCER_URL` / public testnet for a new vault).
    pub fn new(home: &Path, profile_override: Option<NetworkProfile>) -> Self {
        let vault_path = home.join("vault.enc");
        let cache_path = home.join("cache.json");
        let state = if vault_path.exists() { LockState::Locked } else { LockState::Uninitialized };
        Self {
            cache: Cache::load_or_default(&cache_path),
            logger: AuditLogger::new(home),
            autolock: AutoLockFsm::new(state, DEFAULT_AUTOLOCK_TIMEOUT),
            broker: ApprovalBroker::new(),
            unlocked: None,
            revision: 1,
            in_flight: None,
            vault_path,
            cache_path,
            profile_override,
        }
    }

    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    pub fn lock_state(&self) -> LockState {
        self.autolock.state()
    }

    pub fn broker(&self) -> &ApprovalBroker {
        &self.broker
    }

    /// Effective network: explicit override → configured network marker beside the vault → env/testnet.
    fn effective_profile_for_new(&self) -> NetworkProfile {
        self.profile_override
            .clone()
            .or_else(|| Self::configured_network(self.vault_path.parent().unwrap_or_else(|| std::path::Path::new("."))))
            .unwrap_or_else(NetworkProfile::from_env)
    }

    fn profile_from_state(&self, state: &WalletStateV2) -> LezerResult<NetworkProfile> {
        if let Some(p) = &self.profile_override {
            return Ok(p.clone());
        }
        let mut p = NetworkProfile::resolve(Some(&state.sequencer_url))
            .map_err(|e| LezerError::new(ErrorCode::DisallowedEndpointBlocked, e.to_string()))?;
        p.id.clone_from(&state.network_id);
        Ok(p)
    }

    // ── Vault lifecycle ────────────────────────────────────────────────────

    fn ensure_no_vault(&self) -> LezerResult<()> {
        if self.vault_path.exists() {
            return Err(LezerError::new(
                ErrorCode::VaultAlreadyExists,
                format!("a vault already exists at '{}'", self.vault_path.display()),
            ));
        }
        Ok(())
    }

    fn validate_password(password: &str) -> LezerResult<()> {
        if password.chars().count() < 8 {
            return Err(LezerError::new(ErrorCode::InvalidPassword, "password must be at least 8 characters"));
        }
        Ok(())
    }

    fn adopt_new_engine(&mut self, engine: LezEngine, password: &str) -> LezerResult<()> {
        let profile = engine.profile().clone();
        let state = WalletStateV2 {
            upstream_storage_b64: String::new(),
            network_id: profile.id.clone(),
            sequencer_url: profile.sequencer_url.clone(),
            active_account_id: None,
            sessions: Vec::new(),
            tx_journal: Vec::new(),
            tracked_tokens: Vec::new(),
            created_at: now_unix(),
        };
        self.unlocked = Some(Unlocked { engine, state, key: VaultKey::derive_new(password)? });
        self.autolock.on_unlock(Instant::now());
        Ok(())
    }

    /// Label the upstream key-tree root accounts (deterministic from the recovery phrase)
    /// as `main` (public) and `private`, creating them only if absent.
    fn label_default_accounts(&mut self) -> LezerResult<Vec<OwnedAccount>> {
        let u = self.unlocked.as_mut().expect("unlocked");
        let mk = |e: anyhow::Error| LezerError::new(ErrorCode::VaultCorrupted, e.to_string());
        for (kind, label) in [(AccountKind::Public, "main"), (AccountKind::Private, "private")] {
            let accounts = u.engine.accounts();
            if accounts.iter().any(|a| a.labels.iter().any(|l| l == label)) {
                continue;
            }
            match accounts.iter().find(|a| a.kind == kind && a.labels.is_empty()) {
                Some(a) => u.engine.set_label(&a.account_id, label).map_err(mk)?,
                None => {
                    u.engine.new_account(kind, Some(label)).map_err(mk)?;
                }
            }
        }
        let accounts = u.engine.accounts();
        u.state.active_account_id = accounts
            .iter()
            .find(|a| a.labels.iter().any(|l| l == "main"))
            .map(|a| a.account_id.clone());
        Ok(accounts)
    }

    pub fn init(&mut self, password: &str) -> LezerResult<InitResult> {
        self.ensure_no_vault()?;
        Self::validate_password(password)?;
        let profile = self.effective_profile_for_new();
        let (engine, phrase) = LezEngine::create(profile.clone())
            .map_err(|e| LezerError::new(ErrorCode::DisallowedEndpointBlocked, e.to_string()))?;
        self.adopt_new_engine(engine, password)?;
        let accounts = self.label_default_accounts()?;
        self.persist()?;
        self.logger.record("INFO", "vault_init", json!({ "network": profile.id }));
        Ok(InitResult {
            vault_path: self.vault_path.display().to_string(),
            network_id: profile.id,
            recovery_phrase: Some(phrase.to_string()),
            accounts,
        })
    }

    /// Restore from a recovery phrase. With `discover_depth`, scans the chain for
    /// previously used accounts (needs network); otherwise re-derives the default accounts.
    pub async fn restore(
        &mut self,
        phrase: &str,
        password: &str,
        discover_depth: Option<u32>,
    ) -> LezerResult<InitResult> {
        self.ensure_no_vault()?;
        Self::validate_password(password)?;
        let profile = self.effective_profile_for_new();
        let engine = LezEngine::restore(profile.clone(), phrase)
            .map_err(|e| LezerError::new(ErrorCode::InvalidMnemonic, e.to_string()))?;
        self.adopt_new_engine(engine, password)?;
        if let Some(depth) = discover_depth {
            let u = self.unlocked.as_mut().expect("unlocked");
            u.engine
                .discover_accounts(depth)
                .await
                .map_err(|e| chain_err(ErrorCode::SequencerUnreachable, &e))?;
        }
        let accounts = self.label_default_accounts()?;
        self.persist()?;
        self.logger.record("INFO", "vault_restore", json!({ "network": profile.id, "accounts": accounts.len() }));
        Ok(InitResult {
            vault_path: self.vault_path.display().to_string(),
            network_id: profile.id,
            recovery_phrase: None,
            accounts,
        })
    }

    pub fn unlock(&mut self, password: &str) -> LezerResult<()> {
        let envelope = SealedVaultEnvelopeV1::read_from_disk(&self.vault_path)?;
        let (state, key) = envelope.unseal(password)?;
        let profile = self.profile_from_state(&state)?;
        let storage = state.upstream_storage()?;
        let engine = LezEngine::open(profile, &storage)
            .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, format!("{e:#}")))?;
        self.unlocked = Some(Unlocked { engine, state, key });
        self.autolock.on_unlock(Instant::now());
        self.revision += 1;
        self.logger.record("INFO", "vault_unlock", json!({}));
        Ok(())
    }

    pub fn lock(&mut self) -> LockState {
        let next = self.autolock.request_lock();
        if next == LockState::Locked {
            self.unlocked = None;
        }
        self.revision += 1;
        self.logger.record("INFO", "vault_lock", json!({ "lock_state": next }));
        next
    }

    pub fn check_inactivity(&mut self, now: Instant) -> LockState {
        if self.autolock.check_inactivity(now) {
            self.unlocked = None;
            self.revision += 1;
        }
        self.autolock.state()
    }

    fn require_unlocked(&mut self) -> LezerResult<&mut Unlocked> {
        self.check_inactivity(Instant::now());
        self.autolock.record_activity(Instant::now());
        let exists = self.vault_path.exists();
        self.unlocked.as_mut().ok_or_else(|| {
            if exists {
                LezerError::new(ErrorCode::VaultLocked, "vault is locked — unlock it first")
            } else {
                LezerError::new(ErrorCode::VaultUninitialized, "no vault — run `lezer init` or `lezer restore`")
            }
        })
    }

    /// Re-encrypt and persist the vault (upstream storage + Lezer metadata).
    fn persist(&mut self) -> LezerResult<()> {
        let u = self.unlocked.as_mut().ok_or_else(|| LezerError::new(ErrorCode::VaultLocked, "vault is locked"))?;
        let storage = u
            .engine
            .export_storage()
            .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, format!("{e:#}")))?;
        u.state.set_upstream_storage(&storage);
        SealedVaultEnvelopeV1::seal(&u.state, &u.key)?.write_atomic(&self.vault_path)?;
        self.revision += 1;
        let p = u.engine.profile().clone();
        Self::write_network_marker(&self.vault_path, &p);
        Ok(())
    }

    /// Non-secret `{network_id, sequencer_url}` marker beside the vault so read-only tools
    /// (e.g. `verify-adoption`) can target the configured network without unlocking.
    pub fn write_network_marker(vault_path: &Path, profile: &NetworkProfile) {
        let marker = json!({ "network_id": profile.id, "sequencer_url": profile.sequencer_url });
        let _ = std::fs::write(vault_path.with_file_name("network.json"), marker.to_string());
    }

    /// A program deployed from this wallet, without unlocking (`deployments.json`).
    pub fn stored_deployment(home: &Path, name: &str) -> Option<String> {
        serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(
            &std::fs::read_to_string(home.join("deployments.json")).ok()?,
        )
        .ok()?
        .get(name)?
        .get("program_id")?
        .as_str()
        .map(ToString::to_string)
    }

    /// The network the vault was created for, without unlocking (None if no vault).
    pub fn configured_network(home: &Path) -> Option<NetworkProfile> {
        #[derive(Deserialize)]
        struct Marker {
            network_id: String,
            sequencer_url: String,
        }
        let m: Marker = serde_json::from_str(&std::fs::read_to_string(home.join("network.json")).ok()?).ok()?;
        let mut p = NetworkProfile::resolve(Some(&m.sequencer_url)).ok()?;
        p.id = m.network_id;
        Some(p)
    }

    // ── Accounts & balances ────────────────────────────────────────────────

    pub fn accounts(&mut self) -> LezerResult<Vec<OwnedAccount>> {
        Ok(self.require_unlocked()?.engine.accounts())
    }

    pub fn create_account(&mut self, kind: AccountKind, label: Option<&str>) -> LezerResult<OwnedAccount> {
        let u = self.require_unlocked()?;
        let acct = u.engine.new_account(kind, label).map_err(|e| {
            let msg = format!("{e:#}");
            let code = if msg.contains("already in use") { ErrorCode::DuplicateAccountLabel } else { ErrorCode::VaultCorrupted };
            LezerError::new(code, msg)
        })?;
        self.persist()?;
        self.logger.record("INFO", "account_created", json!({ "account_id": acct.account_id, "kind": acct.kind }));
        Ok(acct)
    }

    pub fn import_public_key(&mut self, secret_hex: &str, label: Option<&str>) -> LezerResult<OwnedAccount> {
        let u = self.require_unlocked()?;
        let acct = u.engine.import_public_key(secret_hex, label).map_err(|e| {
            let msg = format!("{e:#}");
            let code = if msg.contains("already in use") { ErrorCode::DuplicateAccountLabel } else { ErrorCode::AccountNotFound };
            LezerError::new(code, msg)
        })?;
        self.persist()?;
        self.logger.record("INFO", "account_imported", json!({ "account_id": acct.account_id }));
        Ok(acct)
    }

    pub fn create_label(&mut self, account: &str, label: &str) -> LezerResult<()> {
        let u = self.require_unlocked()?;
        u.engine.set_label(account, label).map_err(|e| {
            let msg = format!("{e:#}");
            let code = if msg.contains("already in use") { ErrorCode::DuplicateAccountLabel } else { ErrorCode::AccountNotFound };
            LezerError::new(code, msg)
        })?;
        self.persist()
    }

    pub fn set_active_account(&mut self, selector: &str) -> LezerResult<String> {
        let u = self.require_unlocked()?;
        let id = u.engine.resolve(selector).map_err(|e| LezerError::new(ErrorCode::AccountNotFound, e.to_string()))?;
        u.state.active_account_id = Some(id.to_string());
        self.persist()?;
        Ok(id.to_string())
    }

    pub fn resolve_account(&mut self, selector: &str) -> LezerResult<String> {
        let u = self.require_unlocked()?;
        u.engine
            .resolve(selector)
            .map(|id| id.to_string())
            .map_err(|e| LezerError::new(ErrorCode::AccountNotFound, e.to_string()))
    }

    pub async fn balance(&mut self, selector: &str) -> LezerResult<BalanceView> {
        let account_id = self.resolve_account(selector)?;
        let u = self.unlocked.as_ref().expect("checked");
        if account_id.starts_with("Private/") {
            let balance = u.engine.balance(&account_id).await.map_err(|e| chain_err(ErrorCode::AccountNotFound, &e))?;
            return Ok(BalanceView {
                account_id,
                kind: AccountKind::Private,
                balance: balance.to_string(),
                source: "local".into(),
                block_id: Some(u.engine.status().await.last_synced_block),
            });
        }
        match u.engine.balance(&account_id).await {
            Ok(balance) => {
                let block = u.engine.status().await.last_block_id.unwrap_or_default();
                self.cache.record_balance(&account_id, balance, block, now_unix());
                self.cache.save(&self.cache_path);
                Ok(BalanceView {
                    account_id,
                    kind: AccountKind::Public,
                    balance: balance.to_string(),
                    source: "live".into(),
                    block_id: Some(block),
                })
            }
            Err(e) => match self.cache.balances.get(&account_id) {
                Some(c) => Ok(BalanceView {
                    account_id,
                    kind: AccountKind::Public,
                    balance: c.balance.clone(),
                    source: "cached".into(),
                    block_id: Some(c.block_id),
                }),
                None => Err(chain_err(ErrorCode::SequencerUnreachable, &e)),
            },
        }
    }

    /// Scan new blocks for incoming private notes and persist the updated state.
    pub async fn sync(&mut self) -> LezerResult<u64> {
        let u = self.require_unlocked()?;
        let block = u.engine.sync().await.map_err(|e| chain_err(ErrorCode::SequencerUnreachable, &e))?;
        self.persist()?;
        Ok(block)
    }

    pub fn receive_keys(&mut self, selector: &str) -> LezerResult<PrivateReceiveKeys> {
        self.require_unlocked()?
            .engine
            .private_receive_keys(selector)
            .map_err(|e| LezerError::new(ErrorCode::AccountNotFound, e.to_string()))
    }

    pub async fn chain_status(&mut self) -> LezerResult<ChainStatus> {
        Ok(self.require_unlocked()?.engine.status().await)
    }

    // ── Transfers ──────────────────────────────────────────────────────────

    /// Execute a native transfer (all four privacy modes). Returns only after the
    /// sequencer accepted the transaction; `status` tells whether it was included.
    pub async fn transfer(&mut self, from: &str, to: &str, amount: u128) -> LezerResult<TxReceipt> {
        let u = self.require_unlocked()?;
        let target = parse_target(&u.engine, to).map_err(|e| LezerError::new(ErrorCode::AccountNotFound, e.to_string()))?;
        let from_id = u.engine.resolve(from).map_err(|e| LezerError::new(ErrorCode::AccountNotFound, e.to_string()))?.to_string();
        let mode = intent::privacy_mode(&from_id, to);
        self.in_flight = Some(InFlightView {
            description: format!("{mode} transfer of {amount}"),
            requires_zk_proof: mode != "public",
            started_unix: now_unix(),
        });
        self.autolock.begin_critical_op(Instant::now());
        let result = self.unlocked.as_mut().expect("checked").engine.transfer(&from_id, target, amount).await;
        let post_lock = self.autolock.end_critical_op(Instant::now());
        self.in_flight = None;

        let receipt = result.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        let (status, block_id) = match &receipt.status {
            TxStatus::Included { block_id } => ("included", Some(*block_id)),
            TxStatus::Submitted { .. } => ("submitted", None),
        };
        self.unlocked.as_mut().expect("still unlocked during op").state.tx_journal.push(TxJournalEntry {
            tx_hash: receipt.tx_hash.clone(),
            kind: format!("{mode}_transfer"),
            from_account: receipt.from.clone(),
            to_account: receipt.to.clone(),
            amount: receipt.amount.clone(),
            status: status.to_string(),
            block_id,
            created_at: now_unix(),
        });
        self.persist()?;
        self.logger.record(
            "INFO",
            "transfer",
            json!({ "tx_hash": receipt.tx_hash, "mode": mode, "status": status, "build_ms": receipt.build_ms }),
        );
        if post_lock == LockState::Locked {
            self.unlocked = None;
        }
        Ok(receipt)
    }

    // ── Programs ───────────────────────────────────────────────────────────

    /// Testimonial program for the active network: a deployment recorded by
    /// `program deploy` (keyed by the binary's file stem) or `$LEZER_TESTIMONIAL_PROGRAM`.
    pub fn testimonial_program(&self) -> Option<String> {
        self.deployment("lezer_testimonial")
            .map(|d| d.program_id)
            .or_else(|| {
                self.unlocked
                    .as_ref()
                    .map(|u| u.engine.profile().clone())
                    .unwrap_or_else(|| self.effective_profile_for_new())
                    .deployments
                    .testimonial
            })
    }

    /// Look up a program deployed from this wallet by binary name (elf file stem).
    pub fn deployment(&self, name: &str) -> Option<DeployedProgram> {
        serde_json::from_str::<std::collections::BTreeMap<String, DeployedProgram>>(
            &std::fs::read_to_string(self.vault_path.with_file_name("deployments.json")).ok()?,
        )
        .ok()?
        .get(name)
        .cloned()
    }

    fn record_deployment(&self, name: &str, d: &DeployedProgram) {
        let path = self.vault_path.with_file_name("deployments.json");
        let mut map: std::collections::BTreeMap<String, DeployedProgram> =
            std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        map.insert(name.to_string(), d.clone());
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&map).unwrap_or_default());
    }

    /// Deploy a RISC0 program via the official program loader; `payer` pays the fees.
    /// `name` keys the deployment in `deployments.json` (use the binary's file stem).
    pub async fn deploy_program(&mut self, elf: &[u8], payer: &str, name: &str) -> LezerResult<DeployedProgram> {
        self.require_unlocked()?;
        self.autolock.begin_critical_op(Instant::now());
        let result = self.unlocked.as_mut().expect("checked").engine.deploy_program(elf, payer).await;
        self.autolock.end_critical_op(Instant::now());
        let deployed = result.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        if !name.is_empty() {
            self.record_deployment(name, &deployed);
        }
        self.persist()?;
        self.logger.record("INFO", "program_deployed", json!({ "program_id": deployed.program_id, "image_id": deployed.image_id }));
        Ok(deployed)
    }

    /// Publish an LP-0021 testimonial from `author` (public, signs); `payer` pays the fee.
    pub async fn submit_testimonial(
        &mut self,
        program_id: &str,
        author: &str,
        payer: &str,
        username: &str,
        text: &str,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: "publish testimonial".into(), requires_zk_proof: false, started_unix: now_unix() });
        let result = self
            .unlocked
            .as_mut()
            .expect("checked")
            .engine
            .submit_testimonial(program_id, author, payer, username, text)
            .await;
        self.in_flight = None;
        let receipt = result.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        let (status, block_id) = match &receipt.status {
            TxStatus::Included { block_id } => ("included", Some(*block_id)),
            TxStatus::Submitted { .. } => ("submitted", None),
        };
        self.unlocked.as_mut().expect("checked").state.tx_journal.push(TxJournalEntry {
            tx_hash: receipt.tx_hash.clone(),
            kind: "testimonial".into(),
            from_account: receipt.from.clone(),
            to_account: receipt.to.clone(),
            amount: "0".into(),
            status: status.into(),
            block_id,
            created_at: now_unix(),
        });
        self.persist()?;
        Ok(receipt)
    }

    // ── fungible tokens (lezer_token program) ─────────────────────────────

    /// Configured `lezer_token` program id: a program deployed from this wallet first.
    pub fn token_program(&self) -> Option<String> {
        self.deployment("lezer_token").map(|d| d.program_id)
    }

    /// Remember a token id the wallet interacted with (non-secret convenience list).
    fn track_token(&mut self, token_id: &str) {
        let tokens = &mut self.unlocked.as_mut().expect("checked").state.tracked_tokens;
        if !tokens.iter().any(|t| t == token_id) {
            tokens.push(token_id.to_string());
        }
    }

    /// Token ids this wallet has used, for UI enumeration (balances still come from chain).
    pub fn tracked_tokens(&self) -> Vec<String> {
        self.unlocked.as_ref().map(|u| u.state.tracked_tokens.clone()).unwrap_or_default()
    }

    /// Add a token id manually (for tokens received / discovered out-of-band).
    pub fn track_token_id(&mut self, token_id: &str) -> LezerResult<()> {
        self.require_unlocked()?;
        lezer_adapter::parse_public_id(token_id).map_err(|e| chain_err(ErrorCode::InvalidAddress, &e))?;
        self.track_token(token_id);
        self.persist()
    }

    /// Journal + persist a finished token call.
    fn journal_receipt(&mut self, description: &str, receipt: &TxReceipt) -> LezerResult<()> {
        let (status, block_id) = match &receipt.status {
            TxStatus::Included { block_id } => ("included", Some(*block_id)),
            TxStatus::Submitted { .. } => ("submitted", None),
        };
        self.unlocked.as_mut().expect("checked").state.tx_journal.push(TxJournalEntry {
            tx_hash: receipt.tx_hash.clone(),
            kind: description.into(),
            from_account: receipt.from.clone(),
            to_account: receipt.to.clone(),
            amount: "0".into(),
            status: status.into(),
            block_id,
            created_at: now_unix(),
        });
        self.persist()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn token_define(
        &mut self,
        program: &str, definition: &str, payer: &str,
        name: &str, symbol: &str, decimals: u8, authority: Option<&str>, initial_supply: u128,
    ) -> LezerResult<lezer_adapter::TokenInfo> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: format!("define token {symbol}"), requires_zk_proof: false, started_unix: now_unix() });
        let result = self
            .unlocked
            .as_mut()
            .expect("checked")
            .engine
            .token_define(program, definition, payer, name, symbol, decimals, authority, initial_supply)
            .await;
        self.in_flight = None;
        let info = result.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.track_token(&info.token_id);
        self.persist()?;
        self.logger.record("INFO", "token_defined", json!({ "token_id": info.token_id, "symbol": info.symbol }));
        Ok(info)
    }

    /// Chain read; works locked or unlocked (definition accounts are public data).
    pub async fn token_info(&self, program: &str, token_id: &str) -> LezerResult<lezer_adapter::TokenInfo> {
        let profile = self.unlocked.as_ref().map(|u| u.engine.profile().clone()).unwrap_or_else(|| self.effective_profile_for_new());
        lezer_adapter::token_info_rpc(&profile, program, token_id).await.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))
    }

    /// Token balance of a wallet account; exact-account read, no local ledger.
    pub async fn token_balance(&mut self, program: &str, token_id: &str, owner: &str) -> LezerResult<u128> {
        self.require_unlocked()?;
        self.unlocked
            .as_mut()
            .expect("checked")
            .engine
            .token_balance(program, token_id, owner)
            .await
            .map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))
    }

    /// Token balance of any `Public/..` owner — wallet-less chain read.
    pub async fn token_balance_of(&self, program: &str, token_id: &str, owner_id: &str) -> LezerResult<u128> {
        let profile = self.unlocked.as_ref().map(|u| u.engine.profile().clone()).unwrap_or_else(|| self.effective_profile_for_new());
        let owner = lezer_adapter::parse_public_id(owner_id).map_err(|e| chain_err(ErrorCode::InvalidAddress, &e))?;
        lezer_adapter::token_balance_rpc(&profile, program, token_id, &owner).await.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))
    }

    pub async fn token_mint(
        &mut self, program: &str, token_id: &str, authority: &str, owner: &str, payer: &str, amount: u128,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: "token mint".into(), requires_zk_proof: false, started_unix: now_unix() });
        let engine = &mut self.unlocked.as_mut().expect("checked").engine;
        let receipt = engine.token_mint(program, token_id, authority, owner, payer, amount).await;
        self.in_flight = None;
        let receipt = receipt.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.track_token(token_id);
        self.journal_receipt("token mint", &receipt)?;
        Ok(receipt)
    }

    pub async fn token_transfer(
        &mut self, program: &str, token_id: &str, owner: &str, to_owner: &str, payer: &str, amount: u128,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: "token transfer".into(), requires_zk_proof: false, started_unix: now_unix() });
        let engine = &mut self.unlocked.as_mut().expect("checked").engine;
        let receipt = engine.token_transfer(program, token_id, owner, to_owner, payer, amount).await;
        self.in_flight = None;
        let receipt = receipt.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.track_token(token_id);
        self.journal_receipt("token transfer", &receipt)?;
        Ok(receipt)
    }

    pub async fn token_burn(
        &mut self, program: &str, token_id: &str, owner: &str, payer: &str, amount: u128,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: "token burn".into(), requires_zk_proof: false, started_unix: now_unix() });
        let engine = &mut self.unlocked.as_mut().expect("checked").engine;
        let receipt = engine.token_burn(program, token_id, owner, payer, amount).await;
        self.in_flight = None;
        let receipt = receipt.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.track_token(token_id);
        self.journal_receipt("token burn", &receipt)?;
        Ok(receipt)
    }

    /// `new_authority: None` renounces permanently.
    pub async fn token_set_authority(
        &mut self, program: &str, token_id: &str, authority: &str, payer: &str, new_authority: Option<&str>,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        self.in_flight = Some(InFlightView { description: "token set-authority".into(), requires_zk_proof: false, started_unix: now_unix() });
        let engine = &mut self.unlocked.as_mut().expect("checked").engine;
        let receipt = engine.token_set_authority(program, token_id, authority, payer, new_authority).await;
        self.in_flight = None;
        let receipt = receipt.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.track_token(token_id);
        self.journal_receipt("token set-authority", &receipt)?;
        Ok(receipt)
    }

    // ── LP-0023 program registry ──────────────────────────────────────────

    /// Configured `lezer_registry` program id.
    pub fn registry_program(&self) -> Option<String> {
        self.deployment("lezer_registry").map(|d| d.program_id)
    }

    /// List every registered program — wallet-less chain read.
    pub async fn list_registry(&self, registry_id: &str) -> LezerResult<lezer_adapter::RegistryReport> {
        let profile = self.unlocked.as_ref().map(|u| u.engine.profile().clone()).unwrap_or_else(|| self.effective_profile_for_new());
        lezer_adapter::list_registry(&profile, registry_id)
            .await
            .map_err(|e| LezerError::new(ErrorCode::SequencerUnreachable, format!("{e:#}")))
    }

    /// Self-register `subject` (a deployed program name, e.g. `lezer_token`) in the registry
    /// through a chained call — the entry is keyed by the program itself.
    #[allow(clippy::too_many_arguments)]
    pub async fn register_program(
        &mut self,
        registry_id: &str,
        subject: &str,
        name: &str,
        source_url: &str,
        commit: &str,
        version: &str,
        payer: &str,
    ) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        let subject_id = self
            .deployment(subject)
            .map(|d| d.program_id)
            .ok_or_else(|| LezerError::new(ErrorCode::ProgramNotDeployed, format!("no deployment of '{subject}' recorded — deploy it with `lezer program deploy`")))?;
        let registry = lezer_adapter::parse_public_id(registry_id).map_err(|e| chain_err(ErrorCode::InvalidAddress, &e))?;
        let page = lezer_adapter::first_page_with_room_rpc(
                self.unlocked.as_ref().expect("checked").engine.profile(),
                registry_id,
                &subject_id,
        )
        .await
        .map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        let ix = lezer_adapter::register_self_instruction(
            subject,
            registry_core::RegisterSelfParams {
                registry: *registry.value(),
                page,
                name: name.trim().into(),
                source_url: source_url.trim().into(),
                commit: commit.trim().into(),
                version: version.trim().into(),
            },
        )
        .map_err(|e| LezerError::new(ErrorCode::InvalidAddress, format!("{e:#}")))?;
        self.in_flight = Some(InFlightView { description: format!("register {subject}"), requires_zk_proof: false, started_unix: now_unix() });
        let engine = &mut self.unlocked.as_mut().expect("checked").engine;
        let receipt = engine.register_program_self(registry_id, &subject_id, ix, payer, page).await;
        self.in_flight = None;
        let receipt = receipt.map_err(|e| chain_err(ErrorCode::TransactionRejected, &e))?;
        self.journal_receipt("registry register", &receipt)?;
        Ok(receipt)
    }

    // ── dApp sessions & approvals ──────────────────────────────────────────

    pub fn queue_connect(&mut self, dapp_name: &str, dapp_origin: &str, wants_private_balance: bool) -> DappConnectRequest {
        self.revision += 1;
        self.broker.queue_connect_request(dapp_name, dapp_origin, wants_private_balance, now_unix())
    }

    pub fn approve_connect(
        &mut self,
        request_id: &str,
        accounts: Vec<String>,
        allow_private_balances: &[String],
    ) -> LezerResult<SessionGrant> {
        let owned: Vec<String> = self.accounts()?.into_iter().map(|a| a.account_id).collect();
        if let Some(bad) = accounts.iter().find(|a| !owned.contains(a)) {
            return Err(LezerError::new(ErrorCode::AccountNotFound, format!("'{bad}' is not an account of this wallet")));
        }
        let grant = self.broker.approve_connect(request_id, accounts, allow_private_balances, now_unix())?;
        self.require_unlocked()?.state.sessions.push(grant.clone());
        self.persist()?;
        Ok(grant)
    }

    pub fn session(&self, token: &str) -> LezerResult<SessionGrant> {
        let now = now_unix();
        self.unlocked
            .as_ref()
            .and_then(|u| u.state.sessions.iter().find(|s| s.session_token == token && s.expires_at > now))
            .cloned()
            .ok_or_else(|| LezerError::new(ErrorCode::SessionExpiredOrInvalid, "invalid, expired, or locked session"))
    }

    pub fn revoke_session(&mut self, token: &str) -> LezerResult<()> {
        self.broker.cancel_proposals_for_session(token);
        let u = self.require_unlocked()?;
        u.state.sessions.retain(|s| s.session_token != token);
        self.persist()
    }

    /// Revoke by the non-secret `session_id` shown in the snapshot (wallet UI).
    pub fn revoke_session_by_id(&mut self, id: &str) -> LezerResult<()> {
        let token = self
            .require_unlocked()?
            .state
            .sessions
            .iter()
            .find(|s| session_id(&s.session_token) == id)
            .map(|s| s.session_token.clone())
            .ok_or_else(|| LezerError::new(ErrorCode::SessionExpiredOrInvalid, "unknown session"))?;
        self.revoke_session(&token)
    }

    /// Balance read on behalf of a dApp (enforces account sharing + private-balance gate).
    pub async fn dapp_balance(&mut self, token: &str, account_id: &str) -> LezerResult<BalanceView> {
        let session = self.session(token)?;
        verify_session_balance_permission(&session, account_id, now_unix())?;
        self.balance(account_id).await
    }

    async fn precheck(&mut self, intent: &TxIntent) -> Precheck {
        let Some(amount) = intent.amount() else {
            return Precheck::Unavailable { reason: "invalid amount".into() };
        };
        match self.balance(intent.signing_account()).await {
            Ok(b) => {
                let available: u128 = b.balance.parse().unwrap_or(0);
                if available >= amount {
                    Precheck::Passed { available: b.balance }
                } else {
                    Precheck::InsufficientFunds { available: b.balance, required: amount.to_string() }
                }
            }
            Err(e) => Precheck::Unavailable { reason: e.message },
        }
    }

    pub async fn propose(&mut self, token: &str, intent: TxIntent) -> LezerResult<PendingProposal> {
        let session = self.session(token)?;
        let precheck = self.precheck(&intent).await;
        let program = self.testimonial_program();
        let proposal = self.broker.queue_proposal(&session, intent, precheck, program.as_deref(), now_unix())?;
        self.revision += 1;
        Ok(proposal)
    }

    /// Verify the user's decision against the stored summary and execute the transaction.
    pub async fn approve_proposal(&mut self, decision: &ApproveDecision) -> LezerResult<TxReceipt> {
        self.require_unlocked()?;
        let proposal = self.broker.take_verified_proposal(decision)?;
        let result = match &proposal.intent {
            TxIntent::NativeTransfer { from, to, amount } => {
                let amount: u128 = amount.parse().map_err(|_| LezerError::new(ErrorCode::SimulationReverted, "invalid amount"))?;
                self.transfer(from, to, amount).await
            }
            TxIntent::SubmitTestimonial { author, username, text } => {
                let program = proposal.summary.to.clone();
                self.submit_testimonial(&program, author, author, username, text).await
            }
        };
        match result {
            Ok(receipt) => {
                self.broker.record_proposal_executed(
                    &proposal.proposal_id,
                    receipt.tx_hash.clone(),
                    serde_json::to_value(&receipt).unwrap_or_default(),
                );
                Ok(receipt)
            }
            Err(e) => {
                self.broker.record_proposal_failed(&proposal.proposal_id, &e);
                Err(e)
            }
        }
    }

    pub fn reject_request(&mut self, request_id: &str, reason: Option<&str>) -> LezerResult<RequestResolution> {
        self.revision += 1;
        self.broker.reject_request(request_id, reason)
    }

    // ── Snapshot ───────────────────────────────────────────────────────────

    pub fn snapshot(&self) -> WalletStateSnapshot {
        let (accounts, sessions, activity, active, network_id, sequencer_url) = match &self.unlocked {
            Some(u) => (
                u.engine
                    .accounts()
                    .into_iter()
                    .map(|a| {
                        let cached = self.cache.balances.get(&a.account_id);
                        let local = (a.kind == AccountKind::Private)
                            .then(|| u.engine.private_balance(&a.account_id).ok())
                            .flatten();
                        AccountSummaryView {
                            native_balance: local.map(|b| b.to_string()).or_else(|| cached.map(|c| c.balance.clone())),
                            balance_block: cached.map(|c| c.block_id),
                            account_id: a.account_id,
                            kind: a.kind,
                            labels: a.labels,
                        }
                    })
                    .collect(),
                u.state
                    .sessions
                    .iter()
                    .map(|s| SessionSummaryView {
                        session_id: session_id(&s.session_token),
                        dapp_name: s.dapp_name.clone(),
                        dapp_origin: s.dapp_origin.clone(),
                        authorized_accounts: s.authorized_accounts.clone(),
                        expires_at: s.expires_at,
                    })
                    .collect(),
                u.state
                    .tx_journal
                    .iter()
                    .rev()
                    .take(50)
                    .map(|j| ActivityItemView {
                        tx_hash: j.tx_hash.clone(),
                        kind: j.kind.clone(),
                        from_account: j.from_account.clone(),
                        to_account: j.to_account.clone(),
                        amount: j.amount.clone(),
                        status: j.status.clone(),
                        block_id: j.block_id,
                        created_at: j.created_at,
                    })
                    .collect(),
                u.state.active_account_id.clone(),
                u.state.network_id.clone(),
                u.state.sequencer_url.clone(),
            ),
            None => {
                let p = self.effective_profile_for_new();
                (Vec::new(), Vec::new(), Vec::new(), None, p.id, p.sequencer_url)
            }
        };
        let pending_requests = self
            .broker
            .pending_connects()
            .values()
            .map(|c| PendingRequestView {
                request_id: c.request_id.clone(),
                kind: "connect".into(),
                dapp_name: c.dapp_name.clone(),
                summary_json: json!({ "dapp_origin": c.dapp_origin, "wants_private_balance": c.request_private_balance_read }),
            })
            .chain(self.broker.pending_proposals().values().map(|p| PendingRequestView {
                request_id: p.proposal_id.clone(),
                kind: "proposal".into(),
                dapp_name: p.dapp_name.clone(),
                summary_json: serde_json::to_value(&p.summary).unwrap_or_default(),
            }))
            .collect();
        WalletStateSnapshot {
            revision: self.revision,
            lock_state: self.autolock.state(),
            network_id,
            sequencer_url,
            last_seen_block: self.cache.last_seen_block,
            active_account_id: active,
            accounts,
            active_sessions: sessions,
            pending_requests,
            in_flight: self.in_flight.clone(),
            recent_activity: activity,
        }
    }

    /// The recovery phrase is never stored; this only proves the password still opens the vault.
    pub fn verify_password(&self, password: &str) -> LezerResult<()> {
        SealedVaultEnvelopeV1::read_from_disk(&self.vault_path)?.unseal(password).map(|_| ())
    }
}

/// Zeroizing helper for callers handling recovery phrases.
pub fn zeroizing(s: String) -> Zeroizing<String> {
    Zeroizing::new(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(dir: &Path) -> WalletEngine {
        WalletEngine::new(dir, Some(NetworkProfile::localnet()))
    }

    #[test]
    fn init_lock_unlock_roundtrip_preserves_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = engine(dir.path());
        let init = w.init("correct horse battery").unwrap();
        assert_eq!(init.recovery_phrase.as_ref().unwrap().split_whitespace().count(), 24);
        assert_eq!(init.accounts.len(), 2);
        assert!(w.init("correct horse battery").is_err());

        w.lock();
        assert_eq!(w.accounts().unwrap_err().code, ErrorCode::VaultLocked);
        assert_eq!(w.unlock("wrong password!").unwrap_err().code, ErrorCode::InvalidPassword);
        w.unlock("correct horse battery").unwrap();
        let ids: Vec<_> = w.accounts().unwrap().into_iter().map(|a| a.account_id).collect();
        for a in &init.accounts {
            assert!(ids.contains(&a.account_id));
        }

        let raw = std::fs::read_to_string(w.vault_path()).unwrap();
        assert!(!raw.contains(&init.accounts[0].account_id[7..]), "vault must not leak plaintext");
    }

    #[tokio::test]
    async fn restore_reproduces_default_accounts() {
        let dir_a = tempfile::tempdir().unwrap();
        let mut a = engine(dir_a.path());
        let init = a.init("password-a-123").unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let mut b = engine(dir_b.path());
        let restored = b.restore(init.recovery_phrase.as_ref().unwrap(), "password-b-123", None).await.unwrap();
        let main = |accts: &[OwnedAccount]| accts.iter().find(|a| a.labels.contains(&"main".to_string())).unwrap().account_id.clone();
        assert_eq!(main(&restored.accounts), main(&init.accounts));
    }

    #[test]
    fn snapshot_never_contains_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = engine(dir.path());
        let init = w.init("correct horse battery").unwrap();
        let snap = serde_json::to_string(&w.snapshot()).unwrap();
        assert!(!snap.contains(&init.recovery_phrase.unwrap()));
        assert_eq!(w.snapshot().accounts.len(), init.accounts.len());
    }
}
