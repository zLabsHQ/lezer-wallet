//! Encrypted vault (`~/.lezer/vault.enc`).
//!
//! - Argon2id (64 MiB, 3 iterations, 4 lanes) derives a 256-bit key **once** per unlock;
//!   the password itself is never retained. Re-sealing reuses the key with a fresh nonce.
//! - XChaCha20-Poly1305 with a random 192-bit nonce; the header MAC is bound as AAD.
//! - Crash-safe write-temp → fsync → rename, file mode 0600, directory 0700.
//! - Payload (`WalletStateV2`) wraps the official upstream wallet storage (keys + private
//!   account state) — which upstream itself keeps in plaintext — plus Lezer metadata.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::errors::{ErrorCode, LezerError, LezerResult};

/// Payload schema version. v1 vaults were produced by pre-release builds whose keys were
/// not LEZ-compatible and are rejected with a clear message.
pub const CURRENT_VAULT_SCHEMA_VERSION: u32 = 2;

const KDF_MEMORY_KIB: u32 = if cfg!(test) { 8 * 1024 } else { 64 * 1024 };
const KDF_ITERATIONS: u32 = 3;
const KDF_LANES: u32 = 4;
/// Lowest Argon2id memory cost accepted when opening a vault (guards against downgraded files).
const KDF_MIN_MEMORY_KIB: u32 = if cfg!(test) { 8 * 1024 } else { 64 * 1024 };

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn corrupted(msg: impl Into<String>) -> LezerError {
    LezerError::new(ErrorCode::VaultCorrupted, msg)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedVaultEnvelopeV1 {
    pub schema_version: u32,
    pub kdf: Argon2ParamsRecord,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
    pub header_mac_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2ParamsRecord {
    pub algorithm: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt_b64: String,
}

impl Argon2ParamsRecord {
    fn new_random_salt() -> Self {
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        Self {
            algorithm: "argon2id".to_string(),
            memory_kib: KDF_MEMORY_KIB,
            iterations: KDF_ITERATIONS,
            parallelism: KDF_LANES,
            salt_b64: b64().encode(salt),
        }
    }
}

/// Wrapping key held in memory while the vault is unlocked (zeroized on drop).
pub struct VaultKey {
    key: Zeroizing<[u8; 32]>,
    kdf: Argon2ParamsRecord,
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultKey([REDACTED])")
    }
}

impl VaultKey {
    /// Derive a key for a brand-new vault (fresh random salt).
    pub fn derive_new(password: &str) -> LezerResult<Self> {
        let kdf = Argon2ParamsRecord::new_random_salt();
        Self::derive(password, kdf)
    }

    fn derive(password: &str, kdf: Argon2ParamsRecord) -> LezerResult<Self> {
        if kdf.algorithm != "argon2id" {
            return Err(corrupted(format!("unsupported KDF '{}'", kdf.algorithm)));
        }
        if kdf.memory_kib < KDF_MIN_MEMORY_KIB {
            return Err(corrupted(format!(
                "vault KDF memory cost {} KiB is below the required minimum {KDF_MIN_MEMORY_KIB} KiB",
                kdf.memory_kib
            )));
        }
        let salt = b64().decode(&kdf.salt_b64).map_err(|e| corrupted(format!("invalid KDF salt: {e}")))?;
        let params = Params::new(kdf.memory_kib, kdf.iterations, kdf.parallelism, Some(32))
            .map_err(|e| corrupted(format!("invalid Argon2id parameters: {e}")))?;
        let mut key = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &salt, key.as_mut())
            .map_err(|e| corrupted(format!("Argon2id derivation failed: {e}")))?;
        Ok(Self { key, kdf })
    }
}

/// Dapp session grant persisted in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum PermissionScope {
    AccountUse { account_id: String },
    ReadPublicBalance { account_id: String },
    ReadPrivateBalance { account_id: String },
    ReadPrivateReceiveBundle { account_id: String },
    ProposeTransaction { account_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrant {
    pub session_token: String,
    pub dapp_name: String,
    pub dapp_origin: String,
    pub authorized_accounts: Vec<String>,
    pub scopes: Vec<PermissionScope>,
    pub created_at: u64,
    pub expires_at: u64,
}

/// Record of a transaction this wallet submitted. `status` is `"included"` only once the
/// sequencer reported the block; `"submitted"` entries are reconciled on next start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxJournalEntry {
    pub tx_hash: String,
    pub kind: String,
    pub from_account: String,
    pub to_account: String,
    pub amount: String,
    pub status: String,
    pub block_id: Option<u64>,
    pub created_at: u64,
}

/// Decrypted vault payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct WalletStateV2 {
    /// Official upstream wallet storage (JSON), base64-encoded. Contains secret keys.
    pub upstream_storage_b64: String,
    #[zeroize(skip)]
    pub network_id: String,
    #[zeroize(skip)]
    pub sequencer_url: String,
    #[zeroize(skip)]
    pub active_account_id: Option<String>,
    #[zeroize(skip)]
    pub sessions: Vec<SessionGrant>,
    #[zeroize(skip)]
    pub tx_journal: Vec<TxJournalEntry>,
    /// Token ids (definition accounts) this wallet has defined/used — non-secret.
    ///  keeps vaults written before this field readable.
    #[serde(default)]
    #[zeroize(skip)]
    pub tracked_tokens: Vec<String>,
    #[zeroize(skip)]
    pub created_at: u64,
}

impl WalletStateV2 {
    pub fn upstream_storage(&self) -> LezerResult<Zeroizing<Vec<u8>>> {
        b64()
            .decode(&self.upstream_storage_b64)
            .map(Zeroizing::new)
            .map_err(|e| corrupted(format!("invalid upstream storage encoding: {e}")))
    }

    pub fn set_upstream_storage(&mut self, bytes: &[u8]) {
        self.upstream_storage_b64.zeroize();
        self.upstream_storage_b64 = b64().encode(bytes);
    }
}

fn header_mac_hex(schema_version: u32, kdf: &Argon2ParamsRecord, nonce_b64: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"lezer-vault-header-v1:");
    h.update(schema_version.to_le_bytes());
    h.update(kdf.algorithm.as_bytes());
    h.update(kdf.memory_kib.to_le_bytes());
    h.update(kdf.iterations.to_le_bytes());
    h.update(kdf.parallelism.to_le_bytes());
    h.update(kdf.salt_b64.as_bytes());
    h.update(nonce_b64.as_bytes());
    hex::encode(h.finalize())
}

impl SealedVaultEnvelopeV1 {
    /// Encrypt `state` with an already-derived key and a fresh random nonce.
    pub fn seal(state: &WalletStateV2, key: &VaultKey) -> LezerResult<Self> {
        let mut nonce = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut nonce);
        let nonce_b64 = b64().encode(nonce);
        let schema_version = CURRENT_VAULT_SCHEMA_VERSION;
        let mac = header_mac_hex(schema_version, &key.kdf, &nonce_b64);
        let plaintext = Zeroizing::new(
            serde_json::to_vec(state).map_err(|e| corrupted(format!("serialize vault: {e}")))?,
        );
        let ciphertext = XChaCha20Poly1305::new(key.key.as_ref().into())
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &plaintext, aad: mac.as_bytes() })
            .map_err(|_| corrupted("vault encryption failed"))?;
        Ok(Self {
            schema_version,
            kdf: key.kdf.clone(),
            nonce_b64,
            ciphertext_b64: b64().encode(ciphertext),
            header_mac_hex: mac,
        })
    }

    /// Decrypt with `password`; returns the state and the derived key for later re-sealing.
    pub fn unseal(&self, password: &str) -> LezerResult<(WalletStateV2, VaultKey)> {
        if header_mac_hex(self.schema_version, &self.kdf, &self.nonce_b64) != self.header_mac_hex {
            return Err(corrupted("vault header integrity check failed"));
        }
        if self.schema_version != CURRENT_VAULT_SCHEMA_VERSION {
            return Err(LezerError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "vault schema v{} was written by a pre-release build whose keys are not LEZ-compatible; \
                     move it aside and create a new wallet",
                    self.schema_version
                ),
            ));
        }
        let nonce = b64().decode(&self.nonce_b64).map_err(|e| corrupted(format!("invalid nonce: {e}")))?;
        if nonce.len() != 24 {
            return Err(corrupted("invalid nonce length"));
        }
        let ciphertext =
            b64().decode(&self.ciphertext_b64).map_err(|e| corrupted(format!("invalid ciphertext: {e}")))?;
        let key = VaultKey::derive(password, self.kdf.clone())?;
        let plaintext = Zeroizing::new(
            XChaCha20Poly1305::new(key.key.as_ref().into())
                .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ciphertext, aad: self.header_mac_hex.as_bytes() })
                .map_err(|_| LezerError::new(ErrorCode::InvalidPassword, "wrong password"))?,
        );
        let state = serde_json::from_slice(&plaintext).map_err(|e| corrupted(format!("malformed vault payload: {e}")))?;
        Ok((state, key))
    }

    pub fn write_atomic(&self, path: &Path) -> LezerResult<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| corrupted(format!("serialize envelope: {e}")))?;
        write_private_atomic(path, &bytes).map_err(|e| corrupted(format!("failed to write vault: {e}")))
    }

    pub fn read_from_disk(path: &Path) -> LezerResult<Self> {
        if !path.exists() {
            return Err(LezerError::new(
                ErrorCode::VaultUninitialized,
                format!("no vault at '{}' — run `lezer init` or `lezer restore` first", path.display()),
            ));
        }
        let bytes = fs::read(path).map_err(|e| corrupted(format!("failed to read vault: {e}")))?;
        serde_json::from_slice(&bytes).map_err(|e| corrupted(format!("not a Lezer vault file: {e}")))
    }
}

/// Create `dir` (and parents) with mode 0700.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Crash-safe write (temp → fsync → rename → fsync dir) with file mode 0600.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent directory"))?;
    ensure_private_dir(parent)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{name}.tmp.{}", std::process::id()));
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> WalletStateV2 {
        let mut s = WalletStateV2 {
            upstream_storage_b64: String::new(),
            network_id: "lez-localnet".into(),
            sequencer_url: "http://127.0.0.1:3040".into(),
            active_account_id: None,
            sessions: vec![],
            tx_journal: vec![],
            tracked_tokens: vec![],
            created_at: 1,
        };
        s.set_upstream_storage(b"{\"secret\":true}");
        s
    }

    #[test]
    fn seal_unseal_roundtrip_and_wrong_password() {
        let key = VaultKey::derive_new("correct horse").unwrap();
        let env = SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap();
        assert_eq!(env.unseal("wrong").unwrap_err().code, ErrorCode::InvalidPassword);
        let (state, _) = env.unseal("correct horse").unwrap();
        assert_eq!(&*state.upstream_storage().unwrap(), b"{\"secret\":true}");
    }

    #[test]
    fn reseal_with_cached_key_uses_fresh_nonce_and_same_password() {
        let key = VaultKey::derive_new("pw").unwrap();
        let a = SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap();
        let b = SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap();
        assert_ne!(a.nonce_b64, b.nonce_b64);
        assert!(b.unseal("pw").is_ok());
    }

    #[test]
    fn rejects_downgraded_kdf_and_v1_schema() {
        let key = VaultKey::derive_new("pw").unwrap();
        let mut env = SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap();
        env.kdf.memory_kib = 1024;
        env.header_mac_hex = header_mac_hex(env.schema_version, &env.kdf, &env.nonce_b64);
        assert_eq!(env.unseal("pw").unwrap_err().code, ErrorCode::VaultCorrupted);

        let mut v1 = SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap();
        v1.schema_version = 1;
        v1.header_mac_hex = header_mac_hex(1, &v1.kdf, &v1.nonce_b64);
        assert_eq!(v1.unseal("pw").unwrap_err().code, ErrorCode::MigrationFailed);
    }

    #[test]
    fn atomic_write_is_private_and_survives_stale_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lezer").join("vault.enc");
        let key = VaultKey::derive_new("pw").unwrap();
        SealedVaultEnvelopeV1::seal(&sample_state(), &key).unwrap().write_atomic(&path).unwrap();
        fs::write(path.parent().unwrap().join(".vault.enc.tmp.99999"), b"{trunc").unwrap();
        assert!(SealedVaultEnvelopeV1::read_from_disk(&path).unwrap().unseal("pw").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
}
