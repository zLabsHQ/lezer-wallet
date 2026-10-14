//! `lezer-core` — Canonical Native Rust Wallet Engine.
//!
//! **Architectural Boundary Rule #2:** `lezer-core` NEVER imports upstream `wallet` or
//! `lee_core` crates directly. All protocol interactions route through `lezer_adapter::ChainAdapter`.

pub mod autolock;
pub mod broker;
pub mod cache;
pub mod concurrency;
pub mod decoder;
pub mod errors;
pub mod logging;
pub mod migration;
pub mod registry;
pub mod state;
pub mod storage;
pub mod summary;
pub mod testimonial;
pub mod vault;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lezer_adapter::{
    ChainAdapter, DerivedAta, FaucetClaimReceipt, FaucetInfoView, LezAdapter, NetworkProfile,
    PublicTransferReceipt, TokenDefinitionReceipt, TokenOperationReceipt, TransferPrivacyMode,
    classify_transfer_endpoints, compute_token_tx_hash, decode_private_receive_uri, derive_ata,
    derive_spend_nullifier, derive_token_definition_id, parse_private_account_or_uri,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use autolock::{AutoLockFsm, DEFAULT_AUTOLOCK_TIMEOUT, LockState};
pub use broker::{
    ApprovalBroker, DEFAULT_SESSION_TTL_SECS, DappConnectRequest, PendingProposal,
    RequestResolution, verify_session_balance_permission,
};
pub use cache::{CURRENT_CACHE_SCHEMA_VERSION, CacheEnvelopeV1, CachedBalanceEntry};
pub use concurrency::ProverQueue;
pub use decoder::{
    ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_HEX, AUTHENTICATED_TRANSFER_PROGRAM_HEX, AccountMention,
    LEZER_REGISTRY_PROGRAM_HEX, LEZER_TESTIMONIAL_PROGRAM_HEX, PINATA_FAUCET_PROGRAM_HEX,
    TOKEN_PROGRAM_HEX, TxIntent, decode_and_summarize_intent,
};
pub use errors::{ErrorCategory, ErrorCode, ErrorEnvelope, LezerError, LezerResult};
pub use logging::{AuditLogger, Redacted, StructuredLogEntry};
pub use migration::{create_pre_migration_backup, unlock_and_migrate_if_needed};
pub use registry::{
    ProgramVerificationReport, RegistryEntryView, RegistryLedgerState, compute_registry_shard_key,
    derive_deployer_entry_pda, list_registered_programs, lookup_program_entry,
    register_program_entry, search_registered_programs, verify_program_bytecode,
};
pub use state::{
    AccountSummaryView, ActivityItemView, NetworkStatusView, PendingRequestView,
    ProverQueueStatusView, SessionSummaryView, WalletStateSnapshot,
};
pub use storage::{
    CODEX_CID_PREFIX, StorageDownloadReceipt, StorageUploadReceipt, compute_codex_cid,
    pull_encrypted_vault_backup, push_encrypted_vault_backup, store_artifact_in_codex,
};
pub use summary::{
    ApproveDecision, SecurityWarning, SourceVerificationBadge, TransactionSummary,
    VerificationBadgeState, WarningSeverity,
};
pub use testimonial::{
    AdoptionVerificationReport, CANONICAL_SUBMISSION_ID, MAX_ENTRIES_PER_SHARD_PAGE,
    MAX_SHARD_CONTENTION_RETRIES, SHARD_COUNT, TestimonialLedgerState, TestimonialRecord,
    TestimonialShardEntry, TestimonialShardPage, TestimonialSubmitReceipt,
    compute_author_shard_for_account, derive_testimonial_record_pda,
    derive_testimonial_shard_page_pda, submit_testimonial_to_ledger, verify_adoption_from_ledger,
};
pub use vault::{
    AccountMetadataRecord, AccountPrivacy, Argon2ParamsRecord, BackupMetadata,
    CURRENT_VAULT_SCHEMA_VERSION, CanonicalWalletStateV1, PendingPrivateSpend, PermissionScope,
    SealedVaultEnvelopeV1, SessionGrant, TrackedTokenRecord, TxJournalEntry,
    compute_header_mac_hex, derive_wrapping_key, generate_mnemonic_24, validate_mnemonic,
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

/// Token holding entry returned by `WalletEngine::token_balances`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBalanceItem {
    pub definition_id: String,
    pub symbol: String,
    pub decimals: u8,
    pub owner_account_id: String,
    pub ata_account_id: String,
    pub is_private: bool,
    pub balance: String,
    pub total_supply: String,
    pub mint_authority: Option<String>,
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

/// Canonical Lezer Wallet Engine owning `vault.enc`, `cache.json`, `AutoLockFsm`, `ApprovalBroker`, and `AuditLogger`.
pub struct WalletEngine {
    adapter: Arc<dyn ChainAdapter>,
    vault_path: PathBuf,
    cache_path: PathBuf,
    testimonial_ledger_path: PathBuf,
    logger: AuditLogger,
    autolock: AutoLockFsm,
    broker: ApprovalBroker,
    prover_queue: ProverQueue,
    unlocked_state: Option<CanonicalWalletStateV1>,
    unlocked_password: Option<Redacted<String>>,
    cache: CacheEnvelopeV1,
    revision: u64,
    online_block_height: Option<u64>,
}

impl WalletEngine {
    /// Construct a [`WalletEngine`] with an explicit `adapter` and `vault_path`.
    pub fn with_paths(adapter: Arc<dyn ChainAdapter>, vault_path: PathBuf) -> Self {
        let home_dir = vault_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_lezer_home);
        let cache_path = home_dir.join("cache.json");
        let testimonial_ledger_path = std::env::var("LEZER_TESTIMONIAL_LEDGER")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join("testimonials.json"));
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
            testimonial_ledger_path,
            logger,
            autolock: AutoLockFsm::new(initial_lock_state, DEFAULT_AUTOLOCK_TIMEOUT),
            broker: ApprovalBroker::new(),
            prover_queue: ProverQueue::new(),
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

    pub fn testimonial_ledger_path(&self) -> &Path {
        &self.testimonial_ledger_path
    }

    pub fn lock_state(&self) -> LockState {
        self.autolock.state()
    }

    pub fn autolock_mut(&mut self) -> &mut AutoLockFsm {
        &mut self.autolock
    }

    pub fn broker(&self) -> &ApprovalBroker {
        &self.broker
    }

    pub fn broker_mut(&mut self) -> &mut ApprovalBroker {
        &mut self.broker
    }

    pub fn prover_queue(&self) -> &ProverQueue {
        &self.prover_queue
    }

    /// Reconcile any interrupted `PendingPrivateSpend` entries left in `vault.enc` after
    /// a mid-proof crash or restart, returning the number of recovered pending spends.
    pub fn recover_pending_private_spends(&mut self) -> LezerResult<usize> {
        let count = {
            let state = self.require_unlocked()?;
            let mut recovered = 0usize;
            for spend in &mut state.pending_private_spends {
                if spend.status == "in_flight_proving" {
                    spend.status = "recovered_aborted_before_broadcast".to_string();
                    recovered += 1;
                }
            }
            recovered
        };
        if count > 0 {
            self.persist_unlocked_state()?;
            self.revision += 1;
        }
        Ok(count)
    }

    /// Inspect the write-ahead `pending_private_spends` journal in the unlocked vault.
    pub fn pending_private_spends(&mut self) -> LezerResult<Vec<PendingPrivateSpend>> {
        let state = self.require_unlocked()?;
        Ok(state.pending_private_spends.clone())
    }

    /// Record an in-flight `PendingPrivateSpend` entry in `vault.enc` *before* ZK proof
    /// generation begins (write-ahead nullifier lock guard).
    pub fn write_ahead_lock_nullifiers(
        &mut self,
        proposal_id: &str,
        from_account: &str,
        to_account: &str,
        asset_id: &str,
        amount: u128,
        nullifiers: Vec<String>,
    ) -> LezerResult<()> {
        let now_unix = current_unix_timestamp();
        {
            let state = self.require_unlocked()?;
            // Refuse if any of the nullifiers is already locked in flight (`LZR-5004`)
            for existing in &state.pending_private_spends {
                if existing.status == "in_flight_proving"
                    && existing
                        .nullifiers
                        .iter()
                        .any(|n| nullifiers.iter().any(|cand| cand == n))
                {
                    return Err(LezerError::new(
                        ErrorCode::NullifierLockedInFlight,
                        format!(
                            "nullifier already locked by in-flight private spend '{}'",
                            existing.proposal_id
                        ),
                    ));
                }
            }

            state.pending_private_spends.push(PendingPrivateSpend {
                proposal_id: proposal_id.to_string(),
                from_account: from_account.to_string(),
                to_account: to_account.to_string(),
                asset_id: asset_id.to_string(),
                amount: amount.to_string(),
                nullifiers,
                tx_hash: None,
                status: "in_flight_proving".to_string(),
                created_at: now_unix,
            });
        }
        self.persist_unlocked_state()
    }

    fn finalize_pending_private_spend(&mut self, proposal_id: &str, tx_hash: &str) -> LezerResult<()> {
        {
            let state = self.require_unlocked()?;
            if let Some(entry) = state
                .pending_private_spends
                .iter_mut()
                .find(|s| s.proposal_id == proposal_id)
            {
                entry.tx_hash = Some(tx_hash.to_string());
                entry.status = "confirmed".to_string();
            }
        }
        self.persist_unlocked_state()
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

        if let Ok(decoded_uri) = decode_private_receive_uri(trimmed) {
            if let Some(owned) = state.account_metadata.get(&decoded_uri.account_id) {
                return Ok(owned.clone());
            }
            return Ok(AccountMetadataRecord {
                account_id: decoded_uri.account_id,
                label: "External Shielded URI".to_string(),
                privacy: AccountPrivacy::Private,
                derivation_index: u32::MAX,
                public_key_hex: decoded_uri.npk_hex,
                receive_uri: Some(decoded_uri.uri),
                created_at: 0,
            });
        }

        if let Ok(priv_id) = parse_private_account_or_uri(trimmed) {
            return Ok(AccountMetadataRecord {
                account_id: priv_id,
                label: "External Shielded".to_string(),
                privacy: AccountPrivacy::Private,
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

    /// Seed or update a balance entry in `cache.json`.
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

    /// Execute a native LEZ transfer across all 4 privacy permutations (`Public -> Public`,
    /// `Shield`, `Deshield`, `Private -> Private`, including `lezer:priv1...` receive URIs).
    ///
    /// For `Shield`, `Deshield`, and `Private -> Private`:
    /// 1. Writes a `PendingPrivateSpend` entry to `vault.enc` *before* proving (nullifier lock guard).
    /// 2. Marks a critical operation in `AutoLockFsm` so 15-minute inactivity triggers
    ///    `DeferredLockPending` rather than wiping keys mid-proof.
    /// 3. Runs the 4-stage RISC0 + RapidSnark proof through `Semaphore(1)` (`ProverQueue`).
    /// 4. Finalizes the `PendingPrivateSpend` entry and updates cached balances.
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
        if from_acct.derivation_index == u32::MAX {
            return Err(LezerError::new(
                ErrorCode::AccountNotAuthorized,
                format!("sender '{}' is not an owned account", from_acct.account_id),
            ));
        }
        let to_acct = self.resolve_account_or_address(to_selector)?;

        let (privacy_mode, canonical_from, canonical_to) =
            classify_transfer_endpoints(&from_acct.account_id, &to_acct.account_id).map_err(
                |e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()),
            )?;

        // Fast path: Public -> Public
        if privacy_mode == TransferPrivacyMode::PublicToPublic {
            let mnemonic = self.require_unlocked()?.mnemonic_phrase.clone();
            self.autolock.begin_critical_op(Instant::now());
            let transfer_res = self
                .adapter
                .send_public_transfer(
                    &mnemonic,
                    from_acct.derivation_index,
                    &canonical_from,
                    &canonical_to,
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

            self.record_completed_native_transfer(&receipt, amount)?;
            return Ok(receipt);
        }

        // Private / Shielded / Deshielded ZK Proof Path (`Semaphore(1)` + write-ahead nullifier journal)
        let spend_seq = self.revision + 1;
        let prop_id = format!("zk_native_{spend_seq}");
        let nullifiers = if matches!(
            privacy_mode,
            TransferPrivacyMode::DeshieldPrivateToPublic | TransferPrivacyMode::PrivateToPrivate
        ) {
            vec![derive_spend_nullifier(&canonical_from, "native", spend_seq)]
        } else {
            Vec::new()
        };

        self.write_ahead_lock_nullifiers(
            &prop_id,
            &canonical_from,
            &canonical_to,
            "native",
            amount,
            nullifiers.clone(),
        )?;

        self.autolock.begin_critical_op(Instant::now());
        let witness_seed = format!(
            "{}:{}:{}:{}:{}",
            privacy_mode.as_str(),
            canonical_from,
            canonical_to,
            amount,
            spend_seq
        );
        let proof_res = self
            .prover_queue
            .run_exclusive_proof(
                &prop_id,
                privacy_mode.as_str(),
                witness_seed.as_bytes(),
                nullifiers,
                vec![format!("note:{canonical_to}:{amount}")],
                vec![0x7f00_0001],
            )
            .await;
        let post_lock_state = self.autolock.end_critical_op(Instant::now());

        let zk_proof = proof_res
            .map_err(|e| LezerError::new(ErrorCode::RapidsnarkProofFailed, e.to_string()))?;

        let receipt = PublicTransferReceipt {
            tx_hash: zk_proof.groth16_seal_hex[..64].to_string(),
            from_account: canonical_from,
            to_account: canonical_to,
            amount,
            nonce: u128::from(spend_seq),
            privacy_mode,
            signature_hex: zk_proof.groth16_seal_hex.clone(),
            zk_proof: Some(zk_proof),
            broadcast_status: "confirmed".to_string(),
        };

        self.finalize_pending_private_spend(&prop_id, &receipt.tx_hash)?;
        self.record_completed_native_transfer(&receipt, amount)?;

        if post_lock_state == LockState::Locked {
            self.unlocked_state = None;
            self.unlocked_password = None;
        }

        Ok(receipt)
    }

    fn record_completed_native_transfer(
        &mut self,
        receipt: &PublicTransferReceipt,
        amount: u128,
    ) -> LezerResult<()> {
        let now_unix = current_unix_timestamp();
        let locked_nullifiers = receipt
            .zk_proof
            .as_ref()
            .map(|p| p.nullifiers.clone())
            .unwrap_or_default();

        if let Some(ref mut state) = self.unlocked_state {
            state.tx_journal.push(TxJournalEntry {
                tx_hash: receipt.tx_hash.clone(),
                from_account: receipt.from_account.clone(),
                to_account: receipt.to_account.clone(),
                amount: receipt.amount.to_string(),
                kind: receipt.privacy_mode.as_str().to_string(),
                status: receipt.broadcast_status.clone(),
                locked_nullifiers,
                created_at: now_unix,
            });
        }
        self.persist_unlocked_state()?;

        let block = self.cache.last_seen_block;
        if let Some((from_bal, _)) = self.cache.get_cached_native_balance(&receipt.from_account) {
            self.cache.update_native_balance(
                &receipt.from_account,
                from_bal.saturating_sub(amount),
                receipt.nonce,
                block,
            );
        }
        let (to_bal, _) = self
            .cache
            .get_cached_native_balance(&receipt.to_account)
            .unwrap_or((0, block));
        self.cache.update_native_balance(
            &receipt.to_account,
            to_bal.saturating_add(amount),
            0,
            block,
        );
        self.cache.save(&self.cache_path);
        self.revision += 1;

        self.logger.record(
            "INFO",
            receipt.privacy_mode.as_str(),
            json!({
                "tx_hash": receipt.tx_hash,
                "from": receipt.from_account,
                "to": receipt.to_account,
                "amount": receipt.amount.to_string(),
            }),
        );
        Ok(())
    }

    // ── Phase 2: Sharded Testimonial Submitter & Stateless Verifier ──────

    /// Submit an LP-0021 on-chain testimonial (`lezer testimonial submit`) from `author_selector`.
    pub fn submit_testimonial(
        &mut self,
        author_selector: &str,
        username: &str,
        text: &str,
        timestamp: Option<u64>,
    ) -> LezerResult<TestimonialSubmitReceipt> {
        let author = self.resolve_account_or_address(author_selector)?;
        if author.privacy != AccountPrivacy::Public || author.derivation_index == u32::MAX {
            return Err(LezerError::new(
                ErrorCode::AccountNotAuthorized,
                format!("testimonial author '{}' must be an owned public account", author.account_id),
            ));
        }

        let ts = timestamp.unwrap_or_else(current_unix_timestamp);
        let receipt = submit_testimonial_to_ledger(
            &self.testimonial_ledger_path,
            &author.account_id,
            CANONICAL_SUBMISSION_ID,
            username,
            text,
            ts,
            0,
        )?;

        if let Some(ref mut state) = self.unlocked_state {
            state.tx_journal.push(TxJournalEntry {
                tx_hash: receipt.tx_hash.clone(),
                from_account: receipt.author_account_id.clone(),
                to_account: receipt.record_pda.clone(),
                amount: "0".to_string(),
                kind: "submit_testimonial".to_string(),
                status: "confirmed".to_string(),
                locked_nullifiers: Vec::new(),
                created_at: ts,
            });
        }
        self.persist_unlocked_state()?;
        self.revision += 1;

        Ok(receipt)
    }

    /// List all on-chain testimonials across the 16 shards (`lezer testimonial list`).
    pub fn list_testimonials(&self, submission_id: &str) -> LezerResult<AdoptionVerificationReport> {
        verify_adoption_from_ledger(
            &self.testimonial_ledger_path,
            submission_id,
            &self.adapter.profile().sequencer_url,
        )
    }

    // ── Phase 3: Piñata Faucet (`lezer faucet info`, `claim`) ────────────

    /// Query live Piñata PoW faucet status (`lezer faucet info`).
    pub async fn faucet_info(&self) -> LezerResult<FaucetInfoView> {
        self.adapter
            .faucet_info()
            .await
            .map_err(|e| LezerError::new(ErrorCode::SequencerUnreachable, e.to_string()))
    }

    /// Solve the Piñata PoW challenge and claim 150 LEZ to `recipient_selector` (`lezer faucet claim`).
    pub async fn faucet_claim(
        &mut self,
        recipient_selector: &str,
    ) -> LezerResult<FaucetClaimReceipt> {
        let acct = self.resolve_account_or_address(recipient_selector)?;
        if acct.privacy != AccountPrivacy::Public {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                "Piñata faucet credits Public/<base58> accounts directly; select a public account or use auto-shield",
            ));
        }

        let (current_bal, block) = self
            .cache
            .get_cached_native_balance(&acct.account_id)
            .unwrap_or((0, self.cache.last_seen_block));

        let receipt = self
            .adapter
            .faucet_claim(&acct.account_id, current_bal)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("LZR-4004") {
                    LezerError::new(ErrorCode::FaucetCooldownOrDepleted, msg)
                } else {
                    LezerError::new(ErrorCode::SequencerUnreachable, msg)
                }
            })?;

        let new_bal = receipt
            .balance_after
            .parse::<u128>()
            .unwrap_or(current_bal + 150);
        self.cache
            .update_native_balance(&acct.account_id, new_bal, 0, block);
        self.cache.save(&self.cache_path);

        let now_unix = current_unix_timestamp();
        if let Some(ref mut state) = self.unlocked_state {
            state.tx_journal.push(TxJournalEntry {
                tx_hash: receipt.tx_hash.clone(),
                from_account: "Public/PinataFaucet".to_string(),
                to_account: receipt.recipient_account.clone(),
                amount: receipt.amount_claimed.clone(),
                kind: "faucet_claim".to_string(),
                status: receipt.status.clone(),
                locked_nullifiers: Vec::new(),
                created_at: now_unix,
            });
        }
        self.persist_unlocked_state()?;
        self.revision += 1;

        Ok(receipt)
    }

    // ── Phase 3: Fungible Token Program (`TOKEN_ID`) & ATAs ──────────────

    /// Create a new fungible token definition (`lezer token create`) and credit initial supply
    /// to the creator's Associated Token Account (ATA).
    pub fn token_create(
        &mut self,
        symbol: &str,
        initial_supply: u128,
        creator_selector: &str,
        mint_authority_selector: Option<&str>,
    ) -> LezerResult<TokenDefinitionReceipt> {
        let creator = self.resolve_account_or_address(creator_selector)?;
        let mint_authority = match mint_authority_selector {
            Some(sel) if !sel.eq_ignore_ascii_case("none") => {
                Some(self.resolve_account_or_address(sel)?.account_id)
            }
            _ => None,
        };

        let (definition_id, creator_ata) =
            derive_token_definition_id(&creator.account_id, symbol, initial_supply).map_err(
                |e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()),
            )?;

        let tx_hash = compute_token_tx_hash(
            "token_create",
            &definition_id,
            &creator.account_id,
            &initial_supply.to_le_bytes(),
        );

        let tracked = TrackedTokenRecord {
            definition_id: definition_id.clone(),
            symbol: symbol.trim().to_string(),
            decimals: 6,
            total_supply: initial_supply.to_string(),
            mint_authority: mint_authority.clone(),
        };

        let now_unix = current_unix_timestamp();
        {
            let state = self.require_unlocked()?;
            state.tracked_tokens.retain(|t| t.definition_id != definition_id);
            state.tracked_tokens.push(tracked);
            state.tx_journal.push(TxJournalEntry {
                tx_hash: tx_hash.clone(),
                from_account: creator.account_id.clone(),
                to_account: creator_ata.ata_account_id.clone(),
                amount: initial_supply.to_string(),
                kind: "token_create".to_string(),
                status: "confirmed".to_string(),
                locked_nullifiers: Vec::new(),
                created_at: now_unix,
            });
        }
        self.persist_unlocked_state()?;

        self.set_cached_token_balance(
            &creator.account_id,
            &definition_id,
            initial_supply,
        );
        self.revision += 1;

        Ok(TokenDefinitionReceipt {
            definition_id,
            symbol: symbol.trim().to_string(),
            decimals: 6,
            initial_supply: initial_supply.to_string(),
            creator_account: creator.account_id,
            creator_ata_id: creator_ata.ata_account_id,
            mint_authority,
            tx_hash,
        })
    }

    /// Deterministically derive the Associated Token Account (ATA) for `(owner_selector, mint_id)`.
    pub fn token_derive_ata(
        &mut self,
        owner_selector: &str,
        mint_selector: &str,
    ) -> LezerResult<DerivedAta> {
        let owner = self.resolve_account_or_address(owner_selector)?;
        let def_id = self.resolve_token_definition_id(mint_selector)?;
        derive_ata(&owner.account_id, &def_id)
            .map_err(|e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()))
    }

    /// Idempotently initialize an Associated Token Account (`lezer token ata-create`).
    pub fn token_create_ata(
        &mut self,
        owner_selector: &str,
        mint_selector: &str,
    ) -> LezerResult<TokenOperationReceipt> {
        let ata = self.token_derive_ata(owner_selector, mint_selector)?;
        let tx_hash = compute_token_tx_hash(
            "ata_create",
            &ata.definition_id,
            &ata.owner_account_id,
            ata.ata_account_id.as_bytes(),
        );
        let current = self.get_cached_token_balance(&ata.owner_account_id, &ata.definition_id);
        self.set_cached_token_balance(&ata.owner_account_id, &ata.definition_id, current);

        Ok(TokenOperationReceipt {
            operation: "ata_create".to_string(),
            definition_id: ata.definition_id,
            from_account: Some(ata.owner_account_id),
            to_account: None,
            ata_account_id: Some(ata.ata_account_id),
            amount: None,
            new_authority: None,
            tx_hash,
        })
    }

    /// Transfer fungible tokens between accounts (`lezer token send`), automatically deriving
    /// and initializing the recipient's ATA (supports Public, Shield, Deshield, and Private ATAs).
    pub async fn token_transfer_async(
        &mut self,
        from_selector: &str,
        to_selector: &str,
        mint_selector: &str,
        amount: u128,
    ) -> LezerResult<TokenOperationReceipt> {
        let from_acct = self.resolve_account_or_address(from_selector)?;
        let to_acct = self.resolve_account_or_address(to_selector)?;
        let def_id = self.resolve_token_definition_id(mint_selector)?;
        let (mode, _, _) =
            classify_transfer_endpoints(&from_acct.account_id, &to_acct.account_id).map_err(
                |e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()),
            )?;

        if mode.requires_zk_proof() {
            let spend_seq = self.revision + 1;
            let prop_id = format!("zk_token_{spend_seq}");
            let nullifiers = if matches!(
                mode,
                TransferPrivacyMode::DeshieldPrivateToPublic
                    | TransferPrivacyMode::PrivateToPrivate
            ) {
                vec![derive_spend_nullifier(
                    &from_acct.account_id,
                    &def_id,
                    spend_seq,
                )]
            } else {
                Vec::new()
            };

            self.write_ahead_lock_nullifiers(
                &prop_id,
                &from_acct.account_id,
                &to_acct.account_id,
                &format!("token:{def_id}"),
                amount,
                nullifiers.clone(),
            )?;

            self.autolock.begin_critical_op(Instant::now());
            let witness = format!(
                "token:{}:{}:{}:{}",
                mode.as_str(),
                from_acct.account_id,
                to_acct.account_id,
                amount
            );
            let proof_res = self
                .prover_queue
                .run_exclusive_proof(
                    &prop_id,
                    &format!("token_{}", mode.as_str()),
                    witness.as_bytes(),
                    nullifiers,
                    vec![format!("token_ata:{}:{amount}", to_acct.account_id)],
                    vec![0x7f00_0002],
                )
                .await;
            let post_lock = self.autolock.end_critical_op(Instant::now());

            let zk_proof = proof_res
                .map_err(|e| LezerError::new(ErrorCode::RapidsnarkProofFailed, e.to_string()))?;
            let receipt = self.token_transfer(from_selector, to_selector, mint_selector, amount)?;
            self.finalize_pending_private_spend(&prop_id, &zk_proof.groth16_seal_hex[..64])?;

            if post_lock == LockState::Locked {
                self.unlocked_state = None;
                self.unlocked_password = None;
            }
            return Ok(receipt);
        }

        self.token_transfer(from_selector, to_selector, mint_selector, amount)
    }

    /// Transfer fungible tokens between accounts (`lezer token send`), automatically deriving
    /// and initializing the recipient's ATA.
    pub fn token_transfer(
        &mut self,
        from_selector: &str,
        to_selector: &str,
        mint_selector: &str,
        amount: u128,
    ) -> LezerResult<TokenOperationReceipt> {
        if amount == 0 {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                "token transfer amount must be > 0",
            ));
        }
        let from_acct = self.resolve_account_or_address(from_selector)?;
        let to_acct = self.resolve_account_or_address(to_selector)?;
        let def_id = self.resolve_token_definition_id(mint_selector)?;
        let to_ata = derive_ata(&to_acct.account_id, &def_id)
            .map_err(|e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()))?;

        let from_bal = self.get_cached_token_balance(&from_acct.account_id, &def_id);
        if from_bal < amount {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                format!(
                    "insufficient token balance on {}: available {from_bal}, required {amount}",
                    from_acct.account_id
                ),
            ));
        }

        let to_bal = self.get_cached_token_balance(&to_acct.account_id, &def_id);
        self.set_cached_token_balance(&from_acct.account_id, &def_id, from_bal - amount);
        self.set_cached_token_balance(&to_acct.account_id, &def_id, to_bal + amount);

        let tx_hash = compute_token_tx_hash(
            "token_transfer",
            &def_id,
            &from_acct.account_id,
            &amount.to_le_bytes(),
        );
        self.revision += 1;

        Ok(TokenOperationReceipt {
            operation: "token_transfer".to_string(),
            definition_id: def_id,
            from_account: Some(from_acct.account_id),
            to_account: Some(to_acct.account_id),
            ata_account_id: Some(to_ata.ata_account_id),
            amount: Some(amount.to_string()),
            new_authority: None,
            tx_hash,
        })
    }

    /// Mint additional fungible supply (`lezer token mint`, LP-0013), verifying `authority_selector`
    /// matches the token's active `mint_authority`.
    pub fn token_mint(
        &mut self,
        mint_selector: &str,
        to_selector: &str,
        authority_selector: &str,
        amount: u128,
    ) -> LezerResult<TokenOperationReceipt> {
        let authority = self.resolve_account_or_address(authority_selector)?;
        let to_acct = self.resolve_account_or_address(to_selector)?;
        let def_id = self.resolve_token_definition_id(mint_selector)?;
        let to_ata = derive_ata(&to_acct.account_id, &def_id)
            .map_err(|e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()))?;

        {
            let state = self.require_unlocked()?;
            let token = state
                .tracked_tokens
                .iter_mut()
                .find(|t| t.definition_id == def_id)
                .ok_or_else(|| {
                    LezerError::new(
                        ErrorCode::AccountNotFound,
                        format!("unknown token definition '{def_id}'"),
                    )
                })?;

            if token.mint_authority.as_deref() != Some(authority.account_id.as_str()) {
                return Err(LezerError::new(
                    ErrorCode::SimulationReverted,
                    format!(
                        "LP-0013 MintAuthorityMismatch: '{}' is not the active mint_authority ({:?})",
                        authority.account_id, token.mint_authority
                    ),
                ));
            }

            let cur_supply = token.total_supply.parse::<u128>().unwrap_or(0);
            token.total_supply = (cur_supply + amount).to_string();
        }
        self.persist_unlocked_state()?;

        let to_bal = self.get_cached_token_balance(&to_acct.account_id, &def_id);
        self.set_cached_token_balance(&to_acct.account_id, &def_id, to_bal + amount);

        let tx_hash = compute_token_tx_hash(
            "token_mint",
            &def_id,
            &authority.account_id,
            &amount.to_le_bytes(),
        );
        self.revision += 1;

        Ok(TokenOperationReceipt {
            operation: "token_mint".to_string(),
            definition_id: def_id,
            from_account: Some(authority.account_id),
            to_account: Some(to_acct.account_id),
            ata_account_id: Some(to_ata.ata_account_id),
            amount: Some(amount.to_string()),
            new_authority: None,
            tx_hash,
        })
    }

    /// Rotate or permanently revoke (`"none"`) the LP-0013 `mint_authority` (`lezer token set-authority`).
    pub fn token_set_authority(
        &mut self,
        mint_selector: &str,
        current_authority_selector: &str,
        new_authority_selector: Option<&str>,
    ) -> LezerResult<TokenOperationReceipt> {
        let current_auth = self.resolve_account_or_address(current_authority_selector)?;
        let next_auth = match new_authority_selector {
            Some(sel) if !sel.eq_ignore_ascii_case("none") => {
                Some(self.resolve_account_or_address(sel)?.account_id)
            }
            _ => None,
        };
        let def_id = self.resolve_token_definition_id(mint_selector)?;

        {
            let state = self.require_unlocked()?;
            let token = state
                .tracked_tokens
                .iter_mut()
                .find(|t| t.definition_id == def_id)
                .ok_or_else(|| {
                    LezerError::new(
                        ErrorCode::AccountNotFound,
                        format!("unknown token definition '{def_id}'"),
                    )
                })?;

            if token.mint_authority.as_deref() != Some(current_auth.account_id.as_str()) {
                return Err(LezerError::new(
                    ErrorCode::SimulationReverted,
                    format!(
                        "LP-0013 MintAuthorityMismatch: '{}' is not the current mint_authority ({:?})",
                        current_auth.account_id, token.mint_authority
                    ),
                ));
            }
            token.mint_authority = next_auth.clone();
        }
        self.persist_unlocked_state()?;

        let tx_hash = compute_token_tx_hash(
            "token_set_authority",
            &def_id,
            &current_auth.account_id,
            next_auth.as_deref().unwrap_or("none").as_bytes(),
        );
        self.revision += 1;

        Ok(TokenOperationReceipt {
            operation: "token_set_authority".to_string(),
            definition_id: def_id,
            from_account: Some(current_auth.account_id),
            to_account: None,
            ata_account_id: None,
            amount: None,
            new_authority: next_auth,
            tx_hash,
        })
    }

    /// Burn fungible tokens from `owner_selector`'s ATA and decrement `total_supply` (`lezer token burn`).
    pub fn token_burn(
        &mut self,
        owner_selector: &str,
        mint_selector: &str,
        amount: u128,
    ) -> LezerResult<TokenOperationReceipt> {
        let owner = self.resolve_account_or_address(owner_selector)?;
        let def_id = self.resolve_token_definition_id(mint_selector)?;
        let ata = derive_ata(&owner.account_id, &def_id)
            .map_err(|e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()))?;

        let cur_bal = self.get_cached_token_balance(&owner.account_id, &def_id);
        if cur_bal < amount {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                format!(
                    "insufficient token balance to burn on {}: available {cur_bal}, burn {amount}",
                    owner.account_id
                ),
            ));
        }

        {
            let state = self.require_unlocked()?;
            if let Some(token) = state
                .tracked_tokens
                .iter_mut()
                .find(|t| t.definition_id == def_id)
            {
                let cur_supply = token.total_supply.parse::<u128>().unwrap_or(0);
                token.total_supply = cur_supply.saturating_sub(amount).to_string();
            }
        }
        self.persist_unlocked_state()?;
        self.set_cached_token_balance(&owner.account_id, &def_id, cur_bal - amount);

        let tx_hash = compute_token_tx_hash(
            "token_burn",
            &def_id,
            &owner.account_id,
            &amount.to_le_bytes(),
        );
        self.revision += 1;

        Ok(TokenOperationReceipt {
            operation: "token_burn".to_string(),
            definition_id: def_id,
            from_account: Some(owner.account_id),
            to_account: None,
            ata_account_id: Some(ata.ata_account_id),
            amount: Some(amount.to_string()),
            new_authority: None,
            tx_hash,
        })
    }

    /// List all tracked fungible token balances and ATAs for `owner_selector` (`lezer token balances`).
    pub fn token_balances(&mut self, owner_selector: &str) -> LezerResult<Vec<TokenBalanceItem>> {
        let owner = self.resolve_account_or_address(owner_selector)?;
        let tokens = self.require_unlocked()?.tracked_tokens.clone();
        let mut items = Vec::with_capacity(tokens.len());

        for t in tokens {
            let ata = derive_ata(&owner.account_id, &t.definition_id)
                .map_err(|e| LezerError::new(ErrorCode::SimulationReverted, e.to_string()))?;
            let bal = self.get_cached_token_balance(&owner.account_id, &t.definition_id);
            items.push(TokenBalanceItem {
                definition_id: t.definition_id,
                symbol: t.symbol,
                decimals: t.decimals,
                owner_account_id: owner.account_id.clone(),
                ata_account_id: ata.ata_account_id,
                is_private: ata.is_private,
                balance: bal.to_string(),
                total_supply: t.total_supply,
                mint_authority: t.mint_authority,
            });
        }
        Ok(items)
    }

    fn resolve_token_definition_id(&mut self, mint_selector: &str) -> LezerResult<String> {
        let trimmed = mint_selector.trim();
        let state = self.require_unlocked()?;
        if let Some(found) = state
            .tracked_tokens
            .iter()
            .find(|t| t.definition_id == trimmed || t.symbol.eq_ignore_ascii_case(trimmed))
        {
            return Ok(found.definition_id.clone());
        }
        Ok(trimmed.to_string())
    }

    fn get_cached_token_balance(&self, owner_account_id: &str, definition_id: &str) -> u128 {
        self.cache
            .balances
            .get(owner_account_id)
            .and_then(|e| e.token_balances.get(definition_id))
            .and_then(|s| s.parse::<u128>().ok())
            .unwrap_or(0)
    }

    fn set_cached_token_balance(&mut self, owner_account_id: &str, definition_id: &str, amount: u128) {
        let now = current_unix_timestamp();
        let block = self.cache.last_seen_block;
        let entry = self
            .cache
            .balances
            .entry(owner_account_id.to_string())
            .or_insert_with(|| CachedBalanceEntry {
                account_id: owner_account_id.to_string(),
                native_balance: "0".to_string(),
                nonce: 0,
                token_balances: std::collections::BTreeMap::new(),
                updated_at_block: block,
                updated_at_unix: now,
            });
        entry
            .token_balances
            .insert(definition_id.to_string(), amount.to_string());
        entry.updated_at_unix = now;
        self.cache.save(&self.cache_path);
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
        let mut active_sessions = Vec::new();
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
            for s in &state.active_sessions {
                let preview = if s.session_token.len() > 16 {
                    format!("{}...", &s.session_token[..16])
                } else {
                    s.session_token.clone()
                };
                active_sessions.push(SessionSummaryView {
                    session_token_preview: preview,
                    dapp_name: s.dapp_name.clone(),
                    dapp_origin: s.dapp_origin.clone(),
                    authorized_accounts: s.authorized_accounts.clone(),
                    expires_at: s.expires_at,
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

        let mut pending_requests = Vec::new();
        for conn in self.broker.pending_connects().values() {
            pending_requests.push(PendingRequestView {
                request_id: conn.request_id.clone(),
                kind: "connect".to_string(),
                dapp_name: conn.dapp_name.clone(),
                summary_json: serde_json::to_value(conn).unwrap_or_default(),
            });
        }
        for prop in self.broker.pending_proposals().values() {
            pending_requests.push(PendingRequestView {
                request_id: prop.proposal_id.clone(),
                kind: "proposal".to_string(),
                dapp_name: prop.dapp_name.clone(),
                summary_json: serde_json::to_value(&prop.summary).unwrap_or_default(),
            });
        }

        WalletStateSnapshot {
            revision: self.revision,
            lock_state: self.autolock.state(),
            profile_id: self.adapter.profile().id.clone(),
            sequencer_url,
            network,
            active_account_id,
            accounts,
            active_sessions,
            pending_requests,
            prover_queue: self.prover_queue.current_status(),
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

        let mut engine =
            WalletEngine::with_vault_and_zone(vault_path.clone(), Some("lez-testnet-0.4")).unwrap();
        assert_eq!(engine.lock_state(), LockState::Uninitialized);

        let init_res = engine.init_vault("phase1-secret-pw").unwrap();
        assert_eq!(engine.lock_state(), LockState::Unlocked);
        assert!(init_res.main_public_account.account_id.starts_with("Public/"));
        assert!(
            init_res
                .shielded_vault_account
                .account_id
                .starts_with("Private/")
        );

        assert_eq!(engine.lock(), LockState::Locked);
        let bad_pw = engine.unlock("wrong-password").unwrap_err();
        assert_eq!(bad_pw.code, ErrorCode::InvalidPassword);

        engine.unlock("phase1-secret-pw").unwrap();
        let alice = engine.create_account("alice", false).unwrap();
        let _bob = engine.create_account("bob", false).unwrap();
        assert_eq!(engine.list_accounts().unwrap().len(), 4);

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
