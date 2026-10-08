//! `lezer-core` — Canonical Native Rust Wallet Engine.
//!
//! **Architectural Boundary Rule #2:** `lezer-core` NEVER imports upstream `wallet` or
//! `lee_core` crates directly. All protocol interactions route through `lezer_adapter::ChainAdapter`.

pub mod autolock;
pub mod cache;
pub mod errors;
pub mod logging;
pub mod migration;
pub mod state;
pub mod vault;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lezer_adapter::{ChainAdapter, LezAdapter, NetworkProfile, PublicTransferReceipt};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use autolock::{AutoLockFsm, DEFAULT_AUTOLOCK_TIMEOUT, LockState};
pub use cache::{CURRENT_CACHE_SCHEMA_VERSION, CacheEnvelopeV1, CachedBalanceEntry};
pub use errors::{ErrorCategory, ErrorCode, ErrorEnvelope, LezerError, LezerResult};
pub use logging::{AuditLogger, Redacted, StructuredLogEntry};
pub use migration::{create_pre_migration_backup, unlock_and_migrate_if_needed};
pub use state::{
    AccountSummaryView, ActivityItemView, NetworkStatusView, PendingRequestView,
    ProverQueueStatusView, SessionSummaryView, WalletStateSnapshot,
};
pub use vault::{
    AccountMetadataRecord, AccountPrivacy, Argon2ParamsRecord, BackupMetadata,
    CURRENT_VAULT_SCHEMA_VERSION, CanonicalWalletStateV1, PermissionScope, SealedVaultEnvelopeV1,
    SessionGrant, TrackedTokenRecord, TxJournalEntry, compute_header_mac_hex, derive_wrapping_key,
    generate_mnemonic_24, validate_mnemonic,
};

/// Result of `WalletEngine::init_vault` or `WalletEngine::restore_vault`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultInitResult {
    pub vault_path: String,
    pub mnemonic_phrase: String,
    pub main_public_account: AccountMetadataRecord,
    pub shielded_vault_account: AccountMetadataRecord,
    pub active_zone: String,
}

/// Result of `WalletEngine::query_balance` (supports both Online and Offline cached modes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceQueryResult {
    pub account_id: String,
    pub label: String,
    pub privacy: AccountPrivacy,
    pub balance: String,
    pub nonce: u128,
    pub block_height: u64,
    pub is_cached_offline: bool,
    pub badge: String,
}

/// Resolve the default Lezer runtime directory (`~/.lezer` or `LEZER_HOME` override).
pub fn default_lezer_home() -> PathBuf {
    if let Ok(custom) = std::env::var("LEZER_HOME")
        && !custom.trim().is_empty()
    {
        return PathBuf::from(custom);
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(".lezer");
    }
    PathBuf::from(".lezer")
}

/// Canonical Lezer Wallet Engine owning `vault.enc`, `cache.json`, `AutoLockFsm`, and `AuditLogger`.
pub struct WalletEngine {
    adapter: Arc<dyn ChainAdapter>,
    vault_path: PathBuf,
    cache_path: PathBuf,
    logger: AuditLogger,
    autolock: AutoLockFsm,
    unlocked_state: Option<CanonicalWalletStateV1>,
    unlocked_password: Option<Redacted<String>>,
    cache: CacheEnvelopeV1,
    revision: u64,
    online_block_height: Option<u64>,
}

impl WalletEngine {
    /// Construct a [`WalletEngine`] with an explicit `adapter` and `vault_path`.
    /// The separated rebuildable `cache.json` and `logs/lezer.log` live in `vault_path`'s parent directory.
    pub fn with_paths(adapter: Arc<dyn ChainAdapter>, vault_path: PathBuf) -> Self {
        let home_dir = vault_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_lezer_home);
        let cache_path = home_dir.join("cache.json");
        let logger = AuditLogger::new(&home_dir);
        let cache = CacheEnvelopeV1::load_or_rebuild(&cache_path);

        let initial_lock_state = if vault_path.exists() {
            LockState::Locked
        } else {
            LockState::Uninitialized
        };

        Self {
            adapter,
            vault_path,
            cache_path,
            logger,
            autolock: AutoLockFsm::new(initial_lock_state, DEFAULT_AUTOLOCK_TIMEOUT),
            unlocked_state: None,
            unlocked_password: None,
            cache,
            revision: 1,
            online_block_height: None,
        }
    }

    /// Construct a [`WalletEngine`] using the default [`LezAdapter`] and `~/.lezer/vault.enc`.
    pub fn with_default_adapter() -> anyhow::Result<Self> {
        let adapter = Arc::new(LezAdapter::from_env()?);
        let vault_path = default_lezer_home().join("vault.enc");
        Ok(Self::with_paths(adapter, vault_path))
    }

    /// Construct a [`WalletEngine`] with a custom `vault_path` and optional zone selector.
    pub fn with_vault_and_zone(vault_path: PathBuf, zone: Option<&str>) -> LezerResult<Self> {
        let profile = NetworkProfile::resolve(zone).map_err(|e| {
            LezerError::new(ErrorCode::DisallowedEndpointBlocked, e.to_string())
        })?;
        let adapter = Arc::new(LezAdapter::new(profile).map_err(|e| {
            LezerError::new(ErrorCode::SequencerUnreachable, e.to_string())
        })?);
        Ok(Self::with_paths(adapter, vault_path))
    }

    pub fn profile(&self) -> &NetworkProfile {
        self.adapter.profile()
    }

    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }

    pub fn lock_state(&self) -> LockState {
        self.autolock.state()
    }

    pub fn autolock_mut(&mut self) -> &mut AutoLockFsm {
        &mut self.autolock
    }

    /// Initialize a brand-new 24-word BIP-39 vault (`lezer init`), automatically deriving
    /// `Main Public` (`Public/<base58>` at index 0) and `Shielded Vault` (`Private/<base58>` at index 0).
    pub fn init_vault(&mut self, password: &str) -> LezerResult<VaultInitResult> {
        let mnemonic = generate_mnemonic_24();
        self.initialize_from_mnemonic(&mnemonic, password, "vault_init")
    }

    /// Restore a vault from an existing 12/24-word BIP-39 mnemonic (`lezer restore`).
    pub fn restore_vault(
        &mut self,
        mnemonic_phrase: &str,
        password: &str,
    ) -> LezerResult<VaultInitResult> {
        let validated = validate_mnemonic(mnemonic_phrase)?;
        self.initialize_from_mnemonic(&validated, password, "vault_restore")
    }

    fn initialize_from_mnemonic(
        &mut self,
        mnemonic: &str,
        password: &str,
        event_name: &str,
    ) -> LezerResult<VaultInitResult> {
        let now_unix = current_unix_timestamp();
        let active_zone = self.adapter.profile().id.clone();
        let mut state = CanonicalWalletStateV1::new(mnemonic.to_string(), active_zone.clone());

        let pub0 = self.adapter.derive_public_account(mnemonic, 0);
        let main_public_account = AccountMetadataRecord {
            account_id: pub0.account_id.clone(),
            label: "Main Public".to_string(),
            privacy: AccountPrivacy::Public,
            derivation_index: 0,
            public_key_hex: pub0.public_key_hex,
            receive_uri: None,
            created_at: now_unix,
        };

        let priv0 = self.adapter.derive_private_account(mnemonic, 0);
        let shielded_vault_account = AccountMetadataRecord {
            account_id: priv0.account_id.clone(),
            label: "Shielded Vault".to_string(),
            privacy: AccountPrivacy::Private,
            derivation_index: 0,
            public_key_hex: priv0.npk_hex,
            receive_uri: Some(priv0.receive_uri),
            created_at: now_unix,
        };

        state.next_public_index = 1;
        state.next_private_index = 1;
        state.active_account_id = Some(main_public_account.account_id.clone());
        state.account_metadata.insert(
            main_public_account.account_id.clone(),
            main_public_account.clone(),
        );
        state.account_metadata.insert(
            shielded_vault_account.account_id.clone(),
            shielded_vault_account.clone(),
        );

        let envelope = SealedVaultEnvelopeV1::seal(&state, password)?;
        envelope.write_atomic(&self.vault_path)?;

        self.unlocked_state = Some(state);
        self.unlocked_password = Some(Redacted::new(password.to_string()));
        self.autolock.on_unlock(Instant::now());
        self.revision += 1;

        self.logger.record(
            "INFO",
            event_name,
            json!({
                "vault_path": self.vault_path.display().to_string(),
                "main_public": main_public_account.account_id,
                "shielded_vault": shielded_vault_account.account_id,
                "mnemonic": Redacted::new(mnemonic.to_string()),
                "password": Redacted::new(password.to_string()),
            }),
        );

        Ok(VaultInitResult {
            vault_path: self.vault_path.display().to_string(),
            mnemonic_phrase: mnemonic.to_string(),
            main_public_account,
            shielded_vault_account,
            active_zone,
        })
    }

    /// Unlock the vault on disk using `password` (`lezer unlock`).
    /// Rejects wrong passwords with `LZR-1003` (`InvalidPassword`).
    pub fn unlock(&mut self, password: &str) -> LezerResult<()> {
        let envelope = SealedVaultEnvelopeV1::read_from_disk(&self.vault_path)?;
        let (state, migrated_bak) =
            unlock_and_migrate_if_needed(&self.vault_path, &envelope, password)?;

        self.unlocked_state = Some(state);
        self.unlocked_password = Some(Redacted::new(password.to_string()));
        self.autolock.on_unlock(Instant::now());
        self.revision += 1;

        self.logger.record(
            "INFO",
            "vault_unlock",
            json!({
                "vault_path": self.vault_path.display().to_string(),
                "migrated_backup": migrated_bak.map(|p| p.display().to_string()),
                "password": Redacted::new(password.to_string()),
            }),
        );
        Ok(())
    }

    /// Lock the vault (`lezer lock`), immediately zeroizing decrypted key material
    /// unless a critical operation is in flight (`DeferredLockPending`).
    pub fn lock(&mut self) -> LockState {
        let next = self.autolock.request_lock();
        if next == LockState::Locked {
            self.unlocked_state = None;
            self.unlocked_password = None;
        }
        self.revision += 1;
        self.logger.record(
            "INFO",
            "vault_lock",
            json!({ "lock_state": next }),
        );
        next
    }

    /// Check the 15-minute inactivity timer at `now` and wipe keys if expired.
    pub fn check_inactivity(&mut self, now: Instant) -> LockState {
        if self.autolock.check_inactivity(now) {
            self.unlocked_state = None;
            self.unlocked_password = None;
            self.revision += 1;
        }
        self.autolock.state()
    }

    fn require_unlocked(&mut self) -> LezerResult<&mut CanonicalWalletStateV1> {
        self.check_inactivity(Instant::now());
        self.autolock.record_activity(Instant::now());
        if self.unlocked_state.is_none() {
            let code = if self.vault_path.exists() {
                ErrorCode::VaultLocked
            } else {
                ErrorCode::VaultUninitialized
            };
            return Err(LezerError::new(
                code,
                "vault must be unlocked before performing this operation",
            ));
        }
        Ok(self.unlocked_state.as_mut().expect("checked Some"))
    }

    fn persist_unlocked_state(&self) -> LezerResult<()> {
        let Some(ref state) = self.unlocked_state else {
            return Err(LezerError::new(ErrorCode::VaultLocked, "vault is locked"));
        };
        let Some(ref pw) = self.unlocked_password else {
            return Err(LezerError::new(ErrorCode::VaultLocked, "vault is locked"));
        };
        let envelope = SealedVaultEnvelopeV1::seal(state, pw.expose_secret())?;
        envelope.write_atomic(&self.vault_path)
    }

    /// Create and persist a new labeled public or private account (`lezer account new --label <name>`).
    pub fn create_account(
        &mut self,
        label: &str,
        private: bool,
    ) -> LezerResult<AccountMetadataRecord> {
        let trimmed_label = label.trim();
        if trimmed_label.is_empty() {
            return Err(LezerError::new(
                ErrorCode::DuplicateAccountLabel,
                "account label cannot be empty",
            ));
        }

        let adapter = self.adapter.clone();
        let state = self.require_unlocked()?;

        if state
            .account_metadata
            .values()
            .any(|a| a.label.eq_ignore_ascii_case(trimmed_label))
        {
            return Err(LezerError::new(
                ErrorCode::DuplicateAccountLabel,
                format!("an account with label '{trimmed_label}' already exists"),
            ));
        }

        let now_unix = current_unix_timestamp();
        let record = if private {
            let idx = state.next_private_index;
            let derived = adapter.derive_private_account(&state.mnemonic_phrase, idx);
            state.next_private_index = idx + 1;
            AccountMetadataRecord {
                account_id: derived.account_id,
                label: trimmed_label.to_string(),
                privacy: AccountPrivacy::Private,
                derivation_index: idx,
                public_key_hex: derived.npk_hex,
                receive_uri: Some(derived.receive_uri),
                created_at: now_unix,
            }
        } else {
            let idx = state.next_public_index;
            let derived = adapter.derive_public_account(&state.mnemonic_phrase, idx);
            state.next_public_index = idx + 1;
            AccountMetadataRecord {
                account_id: derived.account_id,
                label: trimmed_label.to_string(),
                privacy: AccountPrivacy::Public,
                derivation_index: idx,
                public_key_hex: derived.public_key_hex,
                receive_uri: None,
                created_at: now_unix,
            }
        };

        state
            .account_metadata
            .insert(record.account_id.clone(), record.clone());
        if state.active_account_id.is_none() {
            state.active_account_id = Some(record.account_id.clone());
        }

        self.persist_unlocked_state()?;
        self.revision += 1;

        self.logger.record(
            "INFO",
            "account_created",
            json!({
                "account_id": record.account_id,
                "label": record.label,
                "privacy": record.privacy,
            }),
        );

        Ok(record)
    }

    /// List all accounts in the unlocked vault, ordered by creation/derivation.
    pub fn list_accounts(&mut self) -> LezerResult<Vec<AccountMetadataRecord>> {
        let state = self.require_unlocked()?;
        let mut list: Vec<AccountMetadataRecord> =
            state.account_metadata.values().cloned().collect();
        list.sort_by_key(|a| (a.created_at, a.derivation_index, a.account_id.clone()));
        Ok(list)
    }

    /// Resolve a user-supplied selector (label such as `"alice"` or full `"Public/<base58>"` ID)
    /// to an owned [`AccountMetadataRecord`], or synthesize a transient record for a foreign `Public/<base58>`.
    pub fn resolve_account_or_address(
        &mut self,
        selector: &str,
    ) -> LezerResult<AccountMetadataRecord> {
        let trimmed = selector.trim();
        let state = self.require_unlocked()?;

        if let Some(found) = state.account_metadata.get(trimmed) {
            return Ok(found.clone());
        }
        if let Some(by_label) = state
            .account_metadata
            .values()
            .find(|a| a.label.eq_ignore_ascii_case(trimmed))
        {
            return Ok(by_label.clone());
        }

        // Check if `selector` is a valid external `Public/<base58>` address
        if let Ok((prefixed, _bare)) = lezer_adapter::parse_public_account_id(trimmed) {
            return Ok(AccountMetadataRecord {
                account_id: prefixed,
                label: "External Public".to_string(),
                privacy: AccountPrivacy::Public,
                derivation_index: u32::MAX,
                public_key_hex: String::new(),
                receive_uri: None,
                created_at: 0,
            });
        }

        Err(LezerError::new(
            ErrorCode::AccountNotFound,
            format!("no account matching label or address '{trimmed}'"),
        ))
    }

    /// Query the native LEZ balance for `selector` (label or `Public/<base58>`).
    ///
    /// - When online, queries the live sequencer, updates `cache.json`, and returns `"Live"`.
    /// - When the sequencer is unreachable (or `LEZER_OFFLINE=1` is set), falls back to
    ///   `cache.json` and returns `"Cached (Offline — Block #N)"`.
    pub async fn query_balance(&mut self, selector: &str) -> LezerResult<BalanceQueryResult> {
        let acct = self.resolve_account_or_address(selector)?;
        let force_offline = std::env::var("LEZER_OFFLINE").as_deref() == Ok("1");

        if acct.privacy == AccountPrivacy::Public
            && !force_offline
            && let Ok(live_state) = self
                .adapter
                .get_public_account_state(&acct.account_id)
                .await
        {
            let block_height = self
                .adapter
                .get_last_block_id()
                .await
                .unwrap_or(self.cache.last_seen_block);
            self.online_block_height = Some(block_height);
            self.cache.update_native_balance(
                &acct.account_id,
                live_state.balance,
                live_state.nonce,
                block_height,
            );
            self.cache.save(&self.cache_path);

            return Ok(BalanceQueryResult {
                account_id: acct.account_id,
                label: acct.label,
                privacy: acct.privacy,
                balance: live_state.balance.to_string(),
                nonce: live_state.nonce,
                block_height,
                is_cached_offline: false,
                badge: format!("Online (Block #{block_height})"),
            });
        }

        // Offline Mode fallback via `cache.json`
        let (cached_bal, cached_block) = self
            .cache
            .get_cached_native_balance(&acct.account_id)
            .unwrap_or((0, self.cache.last_seen_block));
        let cached_nonce = self
            .cache
            .balances
            .get(&acct.account_id)
            .map_or(0, |e| e.nonce);

        Ok(BalanceQueryResult {
            account_id: acct.account_id,
            label: acct.label,
            privacy: acct.privacy,
            balance: cached_bal.to_string(),
            nonce: cached_nonce,
            block_height: cached_block,
            is_cached_offline: true,
            badge: format!("Cached (Offline — Block #{cached_block})"),
        })
    }

    /// Seed or update a balance entry in `cache.json` (used during sync or test setup).
    pub fn update_cached_balance(
        &mut self,
        account_id: &str,
        balance: u128,
        nonce: u128,
        block_height: u64,
    ) {
        self.cache
            .update_native_balance(account_id, balance, nonce, block_height);
        self.cache.save(&self.cache_path);
    }

    /// Execute a public native LEZ transfer (`lezer transfer --from <alice> --to <bob> --amount <N>`).
    pub async fn transfer_public_native(
        &mut self,
        from_selector: &str,
        to_selector: &str,
        amount: u128,
    ) -> LezerResult<PublicTransferReceipt> {
        if std::env::var("LEZER_OFFLINE").as_deref() == Ok("1") {
            return Err(LezerError::new(
                ErrorCode::SequencerUnreachable,
                "cannot broadcast transfer while in Offline Mode",
            ));
        }

        let from_acct = self.resolve_account_or_address(from_selector)?;
        if from_acct.privacy != AccountPrivacy::Public || from_acct.derivation_index == u32::MAX {
            return Err(LezerError::new(
                ErrorCode::AccountNotAuthorized,
                format!("sender '{}' is not an owned public account", from_acct.account_id),
            ));
        }
        let to_acct = self.resolve_account_or_address(to_selector)?;
        if to_acct.privacy != AccountPrivacy::Public {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                "Phase 1 public transfer requires a Public/<base58> recipient",
            ));
        }

        let mnemonic = self
            .require_unlocked()?
            .mnemonic_phrase
            .clone();

        self.autolock.begin_critical_op(Instant::now());
        let transfer_res = self
            .adapter
            .send_public_transfer(
                &mnemonic,
                from_acct.derivation_index,
                &from_acct.account_id,
                &to_acct.account_id,
                amount,
            )
            .await;
        self.autolock.end_critical_op(Instant::now());

        let receipt = transfer_res.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("LZR-3003") {
                LezerError::new(ErrorCode::SimulationReverted, msg)
            } else {
                LezerError::new(ErrorCode::SequencerUnreachable, msg)
            }
        })?;

        // Record in write-ahead transaction journal and update local cache
        let now_unix = current_unix_timestamp();
        if let Some(ref mut state) = self.unlocked_state {
            state.tx_journal.push(TxJournalEntry {
                tx_hash: receipt.tx_hash.clone(),
                from_account: receipt.from_account.clone(),
                to_account: receipt.to_account.clone(),
                amount: receipt.amount.to_string(),
                kind: "public_native_transfer".to_string(),
                status: receipt.broadcast_status.clone(),
                locked_nullifiers: Vec::new(),
                created_at: now_unix,
            });
        }
        self.persist_unlocked_state()?;

        // Update cached balances optimistically if present in cache.json
        let block = self.cache.last_seen_block;
        if let Some((from_bal, _)) = self.cache.get_cached_native_balance(&from_acct.account_id) {
            self.cache.update_native_balance(
                &from_acct.account_id,
                from_bal.saturating_sub(amount),
                receipt.nonce,
                block,
            );
        }
        let (to_bal, _) = self
            .cache
            .get_cached_native_balance(&to_acct.account_id)
            .unwrap_or((0, block));
        self.cache.update_native_balance(
            &to_acct.account_id,
            to_bal.saturating_add(amount),
            0,
            block,
        );
        self.cache.save(&self.cache_path);
        self.revision += 1;

        self.logger.record(
            "INFO",
            "public_native_transfer",
            json!({
                "tx_hash": receipt.tx_hash,
                "from": receipt.from_account,
                "to": receipt.to_account,
                "amount": receipt.amount.to_string(),
            }),
        );

        Ok(receipt)
    }

    /// Generate the canonical [`WalletStateSnapshot`] for UI and CLI rendering.
    pub fn snapshot(&self) -> WalletStateSnapshot {
        let sequencer_url = self.adapter.profile().sequencer_url.clone();
        let network = match self.online_block_height {
            Some(block_height) => NetworkStatusView::Online {
                block_height,
                sequencer_url: sequencer_url.clone(),
            },
            None => NetworkStatusView::Offline {
                last_seen_block: self.cache.last_seen_block,
                sequencer_url: sequencer_url.clone(),
            },
        };

        let mut accounts = Vec::new();
        let mut active_account_id = None;
        let mut recent_activity = Vec::new();

        if let Some(ref state) = self.unlocked_state {
            active_account_id = state.active_account_id.clone();
            for acct in state.account_metadata.values() {
                let (bal, _) = self
                    .cache
                    .get_cached_native_balance(&acct.account_id)
                    .unwrap_or((0, 0));
                accounts.push(AccountSummaryView {
                    account_id: acct.account_id.clone(),
                    label: acct.label.clone(),
                    privacy: acct.privacy,
                    native_balance: bal.to_string(),
                    is_cached_offline: self.online_block_height.is_none(),
                    receive_uri: acct.receive_uri.clone(),
                });
            }
            for tx in state.tx_journal.iter().rev().take(20) {
                recent_activity.push(ActivityItemView {
                    tx_hash: tx.tx_hash.clone(),
                    from_account: tx.from_account.clone(),
                    to_account: tx.to_account.clone(),
                    amount: tx.amount.clone(),
                    kind: tx.kind.clone(),
                    status: tx.status.clone(),
                    created_at: tx.created_at,
                });
            }
        }

        WalletStateSnapshot {
            revision: self.revision,
            lock_state: self.autolock.state(),
            profile_id: self.adapter.profile().id.clone(),
            sequencer_url,
            network,
            active_account_id,
            accounts,
            active_sessions: Vec::new(),
            pending_requests: Vec::new(),
            prover_queue: ProverQueueStatusView::Idle,
            recent_activity,
        }
    }
}

fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_phase1_wallet_lifecycle_init_unlock_accounts_offline_balance_and_transfer() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
            std::env::set_var("LEZER_SIMULATE_BROADCAST", "1");
        }
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.enc");

        let mut engine = WalletEngine::with_vault_and_zone(vault_path.clone(), Some("lez-testnet-0.4")).unwrap();
        assert_eq!(engine.lock_state(), LockState::Uninitialized);

        // 1. Init vault
        let init_res = engine.init_vault("phase1-secret-pw").unwrap();
        assert_eq!(engine.lock_state(), LockState::Unlocked);
        assert!(init_res.main_public_account.account_id.starts_with("Public/"));
        assert!(init_res.shielded_vault_account.account_id.starts_with("Private/"));

        // 2. Lock and verify wrong password fails with LZR-1003
        assert_eq!(engine.lock(), LockState::Locked);
        let bad_pw = engine.unlock("wrong-password").unwrap_err();
        assert_eq!(bad_pw.code, ErrorCode::InvalidPassword);

        // 3. Unlock with right password & create labeled public accounts alice and bob
        engine.unlock("phase1-secret-pw").unwrap();
        let alice = engine.create_account("alice", false).unwrap();
        let _bob = engine.create_account("bob", false).unwrap();
        assert_eq!(engine.list_accounts().unwrap().len(), 4);

        // 4. Seed cached balance for alice and verify Offline Mode balance lookup
        engine.update_cached_balance(&alice.account_id, 200, 0, 36316);
        unsafe {
            std::env::set_var("LEZER_OFFLINE", "1");
        }
        let bal_alice = engine.query_balance("alice").await.unwrap();
        assert_eq!(bal_alice.balance, "200");
        assert!(bal_alice.is_cached_offline);
        assert!(bal_alice.badge.contains("Cached (Offline"));
        unsafe {
            std::env::remove_var("LEZER_OFFLINE");
        }

        // 5. Execute public native transfer from alice -> bob (50 LEZ)
        let receipt = engine
            .transfer_public_native("alice", "bob", 50)
            .await
            .unwrap();
        assert_eq!(receipt.amount, 50);

        unsafe {
            std::env::set_var("LEZER_OFFLINE", "1");
        }
        let bal_alice_after = engine.query_balance("alice").await.unwrap();
        let bal_bob_after = engine.query_balance("bob").await.unwrap();
        assert_eq!(bal_alice_after.balance, "150");
        assert_eq!(bal_bob_after.balance, "50");
        unsafe {
            std::env::remove_var("LEZER_OFFLINE");
        }
    }
}
