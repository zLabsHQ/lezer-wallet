//! `LezEngine` — thin wrapper over the official `wallet::WalletCore` (logos-execution-zone v0.3.1).
//!
//! Lezer never re-implements key derivation, transaction encoding, signing, or proving:
//! all of that is delegated to upstream. This module adds:
//! - an in-memory session whose plaintext upstream storage only touches a mode-0700 temp dir
//!   for the duration of a single save (the caller encrypts the exported bytes at rest),
//! - a stable, serializable API for `lezer-core` (string account IDs, receipts),
//! - honest receipts: a transaction is only reported after the sequencer includes it.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use lee::AccountId;
use lee_core::{
    Identifier, NullifierPublicKey, account::ProgramShardSelector, encryption::ViewingPublicKey,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use wallet::{
    AccDecodeData, AccountIdentity, WalletCore,
    account::{AccountIdWithPrivacy, Label},
    config::{SequencerConnectionData, WalletConfigOverrides},
    program_facades::native_token_transfer::NativeTokenTransfer,
};
use zeroize::Zeroizing;

use crate::profile::NetworkProfile;
use crate::rpc::AllowedEndpointsGuard;

/// Upper bound on the fee a wallet-built public transaction may be charged.
pub const DEFAULT_MAX_FEE: u128 = wallet::DEFAULT_MAX_FEE;

/// RISC0 image id of the privacy circuit this build proves against. Must equal the
/// sequencer's `getProgramIds().privacy_preserving_circuit` for private transfers to verify.
pub fn privacy_circuit_id() -> [u32; 8] {
    lee::PRIVACY_PRESERVING_CIRCUIT_ID
}

/// Program id of the built-in native token program.
pub fn native_token_program_id() -> String {
    lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID.to_string()
}

/// Prefix of a Lezer shielded receive address (`npk` + ML-KEM-768 `vpk`, checksummed).
pub const PRIVATE_RECEIVE_PREFIX: &str = "lezer:priv1";
const RECEIVE_VERSION: u8 = 1;
const NPK_LEN: usize = 32;
const VPK_LEN: usize = ViewingPublicKey::LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Public,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedAccount {
    /// `Public/<base58>` or `Private/<base58>`.
    pub account_id: String,
    pub kind: AccountKind,
    pub labels: Vec<String>,
    /// BIP-32-style path inside the upstream key tree (e.g. `/0`), when tree-derived.
    pub chain_index: Option<String>,
    /// For private accounts discovered by sync (funds sent to a receive address): the label
    /// of the account whose keys received them.
    pub received_via: Option<String>,
}

/// Keys a sender needs to pay a private account (as printed by upstream `account show-keys`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateReceiveKeys {
    pub account_id: String,
    pub npk_hex: String,
    pub vpk_hex: String,
    /// Single-string form: `lezer:priv1<base58(version|npk|vpk|sha256[..4])>`.
    pub receive_address: String,
}

/// Where a transfer goes.
#[derive(Debug, Clone)]
pub enum TransferTarget {
    /// An account id (`Public/..` or an owned `Private/..`).
    Account(AccountIdWithPrivacy),
    /// A private account owned by someone else, identified by its receive keys.
    ForeignPrivate {
        npk: NullifierPublicKey,
        vpk: ViewingPublicKey,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    Public,
    Shielded,
    Deshielded,
    Private,
}

impl TransferMode {
    pub const fn requires_proof(self) -> bool {
        !matches!(self, Self::Public)
    }
}

/// Inclusion status reported by the sequencer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TxStatus {
    Included { block_id: u64 },
    /// Accepted by the sequencer but not seen in a block within the polling window.
    Submitted { detail: String },
}

/// Receipt for a submitted transaction. Never constructed before the sequencer accepted it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_hash: String,
    pub status: TxStatus,
    pub mode: TransferMode,
    pub from: String,
    pub to: String,
    pub amount: String,
    /// Wall-clock time spent building + proving + submitting (ms).
    pub build_ms: u64,
    /// Wall-clock time from submission until inclusion (ms).
    pub inclusion_ms: Option<u64>,
}

impl TxReceipt {
    pub fn is_included(&self) -> bool {
        matches!(self.status, TxStatus::Included { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainStatus {
    pub sequencer_url: String,
    pub reachable: bool,
    pub last_block_id: Option<u64>,
    pub last_synced_block: u64,
}

/// One unlocked wallet session backed by upstream `WalletCore`.
pub struct LezEngine {
    core: WalletCore,
    storage_path: PathBuf,
    profile: NetworkProfile,
    _dir: TempDir,
}

fn private_tempdir() -> Result<TempDir> {
    let dir = tempfile::Builder::new().prefix("lezer-session-").tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

fn overrides_for(profile: &NetworkProfile) -> Result<WalletConfigOverrides> {
    Ok(WalletConfigOverrides {
        sequencer: Some(SequencerConnectionData {
            sequencer_addr: profile
                .sequencer_url
                .parse()
                .with_context(|| format!("invalid sequencer URL '{}'", profile.sequencer_url))?,
            basic_auth: None,
        }),
        ..Default::default()
    })
}

fn shred(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::write(path, vec![0u8; meta.len() as usize]);
        let _ = std::fs::remove_file(path);
    }
}

impl LezEngine {
    fn paths(dir: &TempDir) -> (PathBuf, PathBuf) {
        (dir.path().join("wallet_config.json"), dir.path().join("storage.json"))
    }

    fn check_profile(profile: &NetworkProfile) -> Result<()> {
        AllowedEndpointsGuard::from_profile(profile).check_url(&profile.sequencer_url)
    }

    /// Create a brand-new wallet. Returns the engine and the 24-word recovery phrase
    /// (shown to the user exactly once; never persisted by Lezer).
    pub fn create(profile: NetworkProfile) -> Result<(Self, Zeroizing<String>)> {
        Self::check_profile(&profile)?;
        let dir = private_tempdir()?;
        let (config_path, storage_path) = Self::paths(&dir);
        let (core, mnemonic) = WalletCore::new_init_storage(
            config_path,
            storage_path.clone(),
            Some(overrides_for(&profile)?),
            "",
        )?;
        let engine = Self { core, storage_path, profile, _dir: dir };
        Ok((engine, Zeroizing::new(mnemonic.to_string())))
    }

    /// Restore keys from a recovery phrase. Use [`Self::discover_accounts`] afterwards to
    /// re-find previously used accounts on chain.
    pub fn restore(profile: NetworkProfile, mnemonic_phrase: &str) -> Result<Self> {
        let mnemonic = bip39::Mnemonic::from_str(mnemonic_phrase.trim())
            .map_err(|e| anyhow!("invalid recovery phrase: {e}"))?;
        let (mut engine, _discarded) = Self::create(profile)?;
        engine.core.restore_storage(&mnemonic, "")?;
        Ok(engine)
    }

    /// Open an existing wallet from upstream storage bytes (decrypted by the caller).
    pub fn open(profile: NetworkProfile, storage_json: &[u8]) -> Result<Self> {
        Self::check_profile(&profile)?;
        let dir = private_tempdir()?;
        let (config_path, storage_path) = Self::paths(&dir);
        std::fs::write(&storage_path, storage_json).context("failed to stage wallet storage")?;
        let opened = WalletCore::new_update_chain(
            config_path,
            storage_path.clone(),
            Some(overrides_for(&profile)?),
        );
        shred(&storage_path);
        let core = opened.context("wallet storage is not valid upstream storage")?;
        Ok(Self { core, storage_path, profile, _dir: dir })
    }

    /// Serialize the upstream storage (secret keys + private account state) for the caller
    /// to encrypt. The plaintext file is shredded immediately after reading.
    pub fn export_storage(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.core.store_persistent_data()?;
        let bytes = std::fs::read(&self.storage_path);
        shred(&self.storage_path);
        Ok(Zeroizing::new(bytes.context("failed to read exported storage")?))
    }

    pub fn profile(&self) -> &NetworkProfile {
        &self.profile
    }

    pub(crate) const fn core(&self) -> &WalletCore {
        &self.core
    }

    pub(crate) const fn core_mut(&mut self) -> &mut WalletCore {
        &mut self.core
    }

    // ── Accounts ───────────────────────────────────────────────────────────

    pub fn new_account(&mut self, kind: AccountKind, label: Option<&str>) -> Result<OwnedAccount> {
        let label = label.map(str::trim).filter(|l| !l.is_empty()).map(Label::new);
        if let Some(l) = &label {
            self.core.storage().check_label_availability(l)?;
        }
        let id = match kind {
            AccountKind::Public => AccountIdWithPrivacy::Public(self.core.create_new_account_public(None).0),
            AccountKind::Private => AccountIdWithPrivacy::Private(self.core.create_new_account_private(None).0),
        };
        if let Some(l) = label {
            self.core.storage_mut().add_label(l, id)?;
        }
        self.accounts()
            .into_iter()
            .find(|a| a.account_id == id.to_string())
            .context("account missing right after creation")
    }

    /// Import a public account by its 32-byte signing key (hex), as upstream `account import`.
    pub fn import_public_key(&mut self, secret_hex: &str, label: Option<&str>) -> Result<OwnedAccount> {
        let bytes: Zeroizing<Vec<u8>> = Zeroizing::new(hex::decode(secret_hex.trim()).context("private key must be hex")?);
        let raw: [u8; 32] = bytes.as_slice().try_into().map_err(|_| anyhow!("private key must be 32 bytes"))?;
        let key = lee::PrivateKey::try_new(raw).map_err(|e| anyhow!("invalid private key: {e}"))?;
        let id = AccountIdWithPrivacy::Public(AccountId::from(&lee::PublicKey::new_from_private_key(&key)));
        if self.is_owned(id) {
            bail!("{id} is already in this wallet");
        }
        let label = label.map(str::trim).filter(|l| !l.is_empty()).map(Label::new);
        if let Some(l) = &label {
            self.core.storage().check_label_availability(l)?;
        }
        self.core.storage_mut().key_chain_mut().add_imported_public_account(key);
        if let Some(l) = label {
            self.core.storage_mut().add_label(l, id)?;
        }
        self.accounts()
            .into_iter()
            .find(|a| a.account_id == id.to_string())
            .context("account missing right after import")
    }

    pub fn accounts(&self) -> Vec<OwnedAccount> {
        let storage = self.core.storage();
        let mut out: Vec<OwnedAccount> = storage
            .key_chain()
            .account_ids()
            .map(|(id, idx)| OwnedAccount {
                account_id: id.to_string(),
                kind: match id {
                    AccountIdWithPrivacy::Public(_) => AccountKind::Public,
                    AccountIdWithPrivacy::Private(_) => AccountKind::Private,
                },
                labels: storage.labels_for_account(id).map(ToString::to_string).collect(),
                chain_index: idx.map(ToString::to_string),
                received_via: None,
            })
            .collect();
        let labeled_nodes: std::collections::BTreeMap<String, String> = out
            .iter()
            .filter(|a| a.kind == AccountKind::Private)
            .filter_map(|a| Some((a.chain_index.clone()?, a.labels.first()?.clone())))
            .collect();
        for a in out.iter_mut().filter(|a| a.kind == AccountKind::Private && a.labels.is_empty()) {
            a.received_via = a.chain_index.as_ref().and_then(|c| labeled_nodes.get(c).cloned());
        }
        out
    }

    pub fn set_label(&mut self, account: &str, label: &str) -> Result<()> {
        let id = self.resolve(account)?;
        self.core.storage_mut().add_label(Label::new(label.trim()), id)
    }

    /// Resolve a label or a `Public/..`/`Private/..` id to an upstream account id.
    pub fn resolve(&self, selector: &str) -> Result<AccountIdWithPrivacy> {
        let s = selector.trim();
        if let Ok(id) = AccountIdWithPrivacy::from_str(s) {
            return Ok(id);
        }
        self.core
            .storage()
            .resolve_label(&Label::new(s))
            .ok_or_else(|| anyhow!("no account with label or id '{s}'"))
    }

    fn is_owned(&self, id: AccountIdWithPrivacy) -> bool {
        self.core.storage().key_chain().account_ids().any(|(owned, _)| owned == id)
    }

    /// Native balance. Public: read from the sequencer. Private: decrypted local state
    /// (call [`Self::sync`] first to pick up incoming notes).
    pub async fn balance(&self, selector: &str) -> Result<u128> {
        match self.resolve(selector)? {
            AccountIdWithPrivacy::Public(id) => {
                let account = self
                    .core
                    .get_account_view(ProgramShardSelector::native_balance(id))
                    .await?;
                account
                    .data
                    .native_balance()
                    .map_err(|e| anyhow!("malformed native balance for Public/{id}: {e:?}"))
            }
            AccountIdWithPrivacy::Private(_) => self.private_balance(selector),
        }
    }

    /// Native balance of an owned private account from decrypted local state (no I/O).
    pub fn private_balance(&self, selector: &str) -> Result<u128> {
        let AccountIdWithPrivacy::Private(id) = self.resolve(selector)? else {
            bail!("'{selector}' is not a private account");
        };
        let account = self
            .core
            .get_account_private(id)
            .ok_or_else(|| anyhow!("Private/{id} is not owned by this wallet"))?;
        account
            .data
            .native_balance()
            .map_err(|e| anyhow!("malformed native balance for Private/{id}: {e:?}"))
    }

    pub fn private_receive_keys(&self, selector: &str) -> Result<PrivateReceiveKeys> {
        let AccountIdWithPrivacy::Private(id) = self.resolve(selector)? else {
            bail!("receive keys exist only for private accounts");
        };
        let found = self
            .core
            .storage()
            .key_chain()
            .private_account(id)
            .ok_or_else(|| anyhow!("Private/{id} is not owned by this wallet"))?;
        let npk = found.key_chain.nullifier_public_key.0;
        let vpk = found.key_chain.viewing_public_key.to_bytes().to_vec();
        Ok(PrivateReceiveKeys {
            account_id: format!("Private/{id}"),
            npk_hex: hex::encode(npk),
            vpk_hex: hex::encode(&vpk),
            receive_address: encode_receive_address(&npk, &vpk),
        })
    }

    // ── Chain ──────────────────────────────────────────────────────────────

    pub async fn status(&self) -> ChainStatus {
        let last = self.core.get_last_block_id().await.ok();
        ChainStatus {
            sequencer_url: self.profile.sequencer_url.clone(),
            reachable: last.is_some(),
            last_block_id: last,
            last_synced_block: self.core.storage().last_synced_block(),
        }
    }

    /// Scan new blocks and decrypt incoming private notes. Returns the synced block height.
    pub async fn sync(&mut self) -> Result<u64> {
        self.core.sync_to_latest_block().await
    }

    /// After restoring from a phrase: derive key trees to `depth` and keep only accounts
    /// that exist on chain (mirrors upstream `wallet restore-keys`).
    pub async fn discover_accounts(&mut self, depth: u32) -> Result<()> {
        self.core.storage_mut().key_chain_mut().generate_trees_for_depth(depth);
        self.core.sync_to_latest_block().await?;
        let client = self.core.client_owned();
        self.core
            .storage_mut()
            .key_chain_mut()
            .cleanup_trees_remove_uninit_layered(depth, |account_id| {
                use sequencer_service_rpc::RpcClient as _;
                let client = client.clone();
                async move { client.get_account(account_id).await.map_err(Into::into) }
            })
            .await
    }

    /// Native transfer across all four privacy modes. Returns only once the sequencer has
    /// included the transaction; private recipients/senders are updated from the included tx.
    pub async fn transfer(
        &mut self,
        from_selector: &str,
        target: TransferTarget,
        amount: u128,
    ) -> Result<TxReceipt> {
        if amount == 0 {
            bail!("amount must be greater than zero");
        }
        let from = self.resolve(from_selector)?;
        if !self.is_owned(from) {
            bail!("sender {from} is not owned by this wallet");
        }
        let started = Instant::now();
        let ntt = NativeTokenTransfer(&self.core);

        let (mode, to_display, tx_hash, decode) = match (from, target) {
            (AccountIdWithPrivacy::Public(f), TransferTarget::Account(AccountIdWithPrivacy::Public(t))) => {
                let hash = ntt
                    .send_public_transfer(AccountIdentity::Public(f), AccountIdentity::PublicNoSign(t), amount)
                    .await?;
                (TransferMode::Public, format!("Public/{t}"), hash, Vec::new())
            }
            (AccountIdWithPrivacy::Public(f), TransferTarget::Account(AccountIdWithPrivacy::Private(t))) => {
                if !self.is_owned(AccountIdWithPrivacy::Private(t)) {
                    bail!("Private/{t} is not owned by this wallet; pay foreign private accounts by receive address");
                }
                let (hash, secret) = ntt.send_shielded_transfer(AccountIdentity::Public(f), t, amount).await?;
                (TransferMode::Shielded, format!("Private/{t}"), hash, vec![AccDecodeData::Decode(secret, t)])
            }
            (AccountIdWithPrivacy::Public(f), TransferTarget::ForeignPrivate { npk, vpk }) => {
                let (hash, _) = ntt
                    .send_shielded_transfer_to_outer_account(
                        AccountIdentity::Public(f),
                        npk,
                        vpk,
                        random_identifier(),
                        amount,
                    )
                    .await?;
                (TransferMode::Shielded, "foreign private account".to_string(), hash, Vec::new())
            }
            (AccountIdWithPrivacy::Private(f), TransferTarget::Account(AccountIdWithPrivacy::Public(t))) => {
                let (hash, secret) = ntt.send_deshielded_transfer(f, t, amount).await?;
                (TransferMode::Deshielded, format!("Public/{t}"), hash, vec![AccDecodeData::Decode(secret, f)])
            }
            (AccountIdWithPrivacy::Private(f), TransferTarget::Account(AccountIdWithPrivacy::Private(t))) => {
                if !self.is_owned(AccountIdWithPrivacy::Private(t)) {
                    bail!("Private/{t} is not owned by this wallet; pay foreign private accounts by receive address");
                }
                let (hash, [sf, st]) = ntt.send_private_transfer_to_owned_account(f, t, amount).await?;
                (
                    TransferMode::Private,
                    format!("Private/{t}"),
                    hash,
                    vec![AccDecodeData::Decode(sf, f), AccDecodeData::Decode(st, t)],
                )
            }
            (AccountIdWithPrivacy::Private(f), TransferTarget::ForeignPrivate { npk, vpk }) => {
                let (hash, [sf, _]) = ntt
                    .send_private_transfer_to_outer_account(f, npk, vpk, random_identifier(), amount)
                    .await?;
                (
                    TransferMode::Private,
                    "foreign private account".to_string(),
                    hash,
                    vec![AccDecodeData::Decode(sf, f)],
                )
            }
        };
        let build_ms = started.elapsed().as_millis() as u64;

        let submitted = Instant::now();
        let (status, inclusion_ms) = match self.core.poll_transaction(tx_hash).await {
            Ok((tx, block_id)) => {
                if let common::transaction::LeeTransaction::PrivacyPreserving(pp) = &tx {
                    self.core
                        .decode_insert_privacy_preserving_transaction_results(pp, &decode)?;
                }
                (TxStatus::Included { block_id }, Some(submitted.elapsed().as_millis() as u64))
            }
            Err(e) => (TxStatus::Submitted { detail: format!("{e:#}") }, None),
        };

        Ok(TxReceipt {
            tx_hash: tx_hash.to_string(),
            status,
            mode,
            from: from.to_string(),
            to: to_display,
            amount: amount.to_string(),
            build_ms,
            inclusion_ms,
        })
    }
}

fn random_identifier() -> Identifier {
    Identifier::new(rand::random())
}

/// Encode `npk | vpk` into a single checksummed receive address.
pub fn encode_receive_address(npk: &[u8; NPK_LEN], vpk: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let mut payload = Vec::with_capacity(1 + NPK_LEN + vpk.len() + 4);
    payload.push(RECEIVE_VERSION);
    payload.extend_from_slice(npk);
    payload.extend_from_slice(vpk);
    let checksum = Sha256::digest(&payload);
    payload.extend_from_slice(&checksum[..4]);
    format!("{PRIVATE_RECEIVE_PREFIX}{}", bs58::encode(payload).into_string())
}

/// Decode a `lezer:priv1…` receive address into upstream key types.
pub fn decode_receive_address(address: &str) -> Result<(NullifierPublicKey, ViewingPublicKey)> {
    use sha2::{Digest as _, Sha256};
    let body = address
        .trim()
        .strip_prefix(PRIVATE_RECEIVE_PREFIX)
        .ok_or_else(|| anyhow!("receive address must start with '{PRIVATE_RECEIVE_PREFIX}'"))?;
    let bytes = bs58::decode(body).into_vec().context("receive address is not valid base58")?;
    if bytes.len() != 1 + NPK_LEN + VPK_LEN + 4 {
        bail!("receive address has wrong length");
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - 4);
    if Sha256::digest(payload)[..4] != *checksum {
        bail!("receive address checksum mismatch");
    }
    if payload[0] != RECEIVE_VERSION {
        bail!("unsupported receive address version {}", payload[0]);
    }
    let npk: [u8; NPK_LEN] = payload[1..1 + NPK_LEN].try_into().expect("length checked");
    let vpk = ViewingPublicKey::from_bytes(payload[1 + NPK_LEN..].to_vec())
        .map_err(|e| anyhow!("invalid viewing key: {e}"))?;
    Ok((NullifierPublicKey(npk), vpk))
}

/// Parse a transfer destination: receive address, `Public/..`, `Private/..`, or a label
/// (labels are resolved by the engine).
pub fn parse_target(engine: &LezEngine, to: &str) -> Result<TransferTarget> {
    let to = to.trim();
    if to.starts_with(PRIVATE_RECEIVE_PREFIX) {
        let (npk, vpk) = decode_receive_address(to)?;
        return Ok(TransferTarget::ForeignPrivate { npk, vpk });
    }
    Ok(TransferTarget::Account(engine.resolve(to)?))
}

/// Parse a bare or `Public/`-prefixed account id.
pub fn parse_public_id(s: &str) -> Result<AccountId> {
    let bare = s.trim().strip_prefix("Public/").unwrap_or(s.trim());
    AccountId::from_str(bare).map_err(|e| anyhow!("invalid account id '{s}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_export_open_roundtrip_keeps_accounts_and_labels() {
        let (mut engine, mnemonic) = LezEngine::create(NetworkProfile::localnet()).unwrap();
        assert_eq!(mnemonic.split_whitespace().count(), 24);
        let public = engine.new_account(AccountKind::Public, Some("alice")).unwrap();
        let private = engine.new_account(AccountKind::Private, Some("vault")).unwrap();
        assert!(public.account_id.starts_with("Public/"));
        assert!(private.account_id.starts_with("Private/"));

        let bytes = engine.export_storage().unwrap();
        let reopened = LezEngine::open(NetworkProfile::localnet(), &bytes).unwrap();
        let ids: Vec<_> = reopened.accounts().into_iter().map(|a| a.account_id).collect();
        assert!(ids.contains(&public.account_id));
        assert!(ids.contains(&private.account_id));
        assert_eq!(reopened.resolve("alice").unwrap().to_string(), public.account_id);
    }

    #[test]
    fn receive_address_roundtrips_and_rejects_tampering() {
        let (mut engine, _) = LezEngine::create(NetworkProfile::localnet()).unwrap();
        engine.new_account(AccountKind::Private, Some("vault")).unwrap();
        let keys = engine.private_receive_keys("vault").unwrap();
        let (npk, vpk) = decode_receive_address(&keys.receive_address).unwrap();
        assert_eq!(hex::encode(npk.0), keys.npk_hex);
        assert_eq!(hex::encode(vpk.to_bytes()), keys.vpk_hex);

        let mut bad = keys.receive_address.clone();
        bad.pop();
        bad.push(if keys.receive_address.ends_with('2') { '3' } else { '2' });
        assert!(decode_receive_address(&bad).is_err());
    }

    #[test]
    fn same_phrase_restores_same_first_accounts() {
        let (mut a, phrase) = LezEngine::create(NetworkProfile::localnet()).unwrap();
        let a_pub = a.new_account(AccountKind::Public, None).unwrap();
        let mut b = LezEngine::restore(NetworkProfile::localnet(), &phrase).unwrap();
        let b_pub = b.new_account(AccountKind::Public, None).unwrap();
        assert_eq!(a_pub.account_id, b_pub.account_id);
    }
}
