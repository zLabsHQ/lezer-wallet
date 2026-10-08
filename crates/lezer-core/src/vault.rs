//! `SealedVaultEnvelopeV1` (`~/.lezer/vault.enc`) — Encrypted key & authority storage.
//!
//! - Uses `Argon2id` (default: 64 MiB memory, 3 iterations, 4 parallelism; configurable
//!   in envelope header) + `XChaCha20-Poly1305` AEAD with a 192-bit (24-byte) random nonce.
//! - Enforces crash-safe **write-temp-fsync-rename** atomic persistence (`vault.enc.tmp.<pid>`).
//! - Separates critical key state (`vault.enc`) from rebuildable cache (`cache.json`).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use bip39::Mnemonic;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{ErrorCode, LezerError, LezerResult};

/// Current canonical vault envelope schema version.
pub const CURRENT_VAULT_SCHEMA_VERSION: u32 = 1;

/// On-disk encrypted vault envelope (`~/.lezer/vault.enc`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedVaultEnvelopeV1 {
    /// Envelope schema version (1 for LP-0021).
    pub schema_version: u32,
    /// KDF parameters used to derive the 256-bit wrapping key from the user's password.
    pub kdf: Argon2ParamsRecord,
    /// 192-bit (24-byte) random nonce for XChaCha20-Poly1305, base64-encoded.
    pub nonce_b64: String,
    /// Authenticated ciphertext of `CanonicalWalletStateV1`, base64-encoded.
    pub ciphertext_b64: String,
    /// SHA-256 integrity fingerprint of the associated header metadata (`schema_version` + `kdf` + `nonce_b64`).
    pub header_mac_hex: String,
}

/// Argon2id parameters stored in the envelope header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2ParamsRecord {
    pub algorithm: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt_b64: String,
}

impl Argon2ParamsRecord {
    /// Production default parameters (`65536 KiB` = 64 MiB, `3` iterations, `4` lanes).
    /// Can be overridden via `LEZER_KDF_MEMORY_KIB` in resource-constrained CI tests.
    pub fn new_random_salt() -> Self {
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        let memory_kib = std::env::var("LEZER_KDF_MEMORY_KIB")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(65_536);
        Self::with_salt(&salt, memory_kib, 3, 4)
    }

    pub fn with_salt(salt: &[u8; 32], memory_kib: u32, iterations: u32, parallelism: u32) -> Self {
        Self {
            algorithm: "argon2id".to_string(),
            memory_kib,
            iterations,
            parallelism,
            salt_b64: base64::engine::general_purpose::STANDARD.encode(salt),
        }
    }
}

/// Privacy classification of a wallet account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountPrivacy {
    Public,
    Private,
}

/// Extended account metadata stored inside `CanonicalWalletStateV1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMetadataRecord {
    /// Canonical account identifier (`"Public/<base58>"` or `"Private/<base58>"`).
    pub account_id: String,
    /// Human-readable label (e.g. `"Main Public"`, `"alice"`).
    pub label: String,
    /// Public vs. Private account type.
    pub privacy: AccountPrivacy,
    /// Deterministic derivation index on `m/44'/9999'/<privacy>'/0/i`.
    pub derivation_index: u32,
    /// Hex-encoded 32-byte public verifying key (`pk_pub`) or commitment.
    pub public_key_hex: String,
    /// Optional `lezer:priv1...` receive URI for private accounts.
    pub receive_uri: Option<String>,
    /// Unix timestamp of creation.
    pub created_at: u64,
}

/// Permission scope granted to a connected dApp session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum PermissionScope {
    AccountUse { account_id: String },
    ReadPublicBalance { account_id: String },
    ReadPrivateBalance { account_id: String },
    ReadPrivateReceiveBundle { account_id: String },
    ProposeTransaction { account_id: String },
}

/// Active dApp session grant persisted in the vault.
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

/// Tracked fungible token definition metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedTokenRecord {
    pub definition_id: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: String,
    pub mint_authority: Option<String>,
}

/// Write-ahead transaction & nullifier journal entry (prevents double-spends on crash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxJournalEntry {
    pub tx_hash: String,
    pub from_account: String,
    pub to_account: String,
    pub amount: String,
    pub kind: String,
    pub status: String,
    pub locked_nullifiers: Vec<String>,
    pub created_at: u64,
}

/// Logos Storage (Codex) encrypted vault backup metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub latest_cid: String,
    pub storage_node_url: String,
    pub uploaded_at: u64,
}

/// Canonical decrypted wallet state (`CanonicalWalletStateV1`).
/// Zeroizes mnemonic and seed material on drop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct CanonicalWalletStateV1 {
    /// 24-word BIP-39 mnemonic phrase.
    pub mnemonic_phrase: String,
    /// Serialized upstream `WalletCore` PersistentStorage blob (keys, chain tree, private notes).
    pub upstream_storage_blob: Vec<u8>,
    /// Next derivation index for public accounts (`m/44'/9999'/0'/0/i`).
    pub next_public_index: u32,
    /// Next derivation index for private accounts (`m/44'/9999'/1'/0/i`).
    pub next_private_index: u32,
    /// Active selected account ID (`Public/<base58>` or `Private/<base58>`).
    #[zeroize(skip)]
    pub active_account_id: Option<String>,
    /// Extended account metadata keyed by `account_id`.
    #[zeroize(skip)]
    pub account_metadata: BTreeMap<String, AccountMetadataRecord>,
    /// Active dApp session grants.
    #[zeroize(skip)]
    pub active_sessions: Vec<SessionGrant>,
    /// Tracked fungible token definitions.
    #[zeroize(skip)]
    pub tracked_tokens: Vec<TrackedTokenRecord>,
    /// Write-ahead transaction & nullifier journal.
    #[zeroize(skip)]
    pub tx_journal: Vec<TxJournalEntry>,
    /// Configured network/zone profile (`"lez-testnet-0.4"`, `"lez-localnet"`, etc.).
    #[zeroize(skip)]
    pub active_zone: String,
    /// Optional Logos Storage backup metadata.
    #[zeroize(skip)]
    pub backup_meta: Option<BackupMetadata>,
}

impl CanonicalWalletStateV1 {
    /// Construct a fresh `CanonicalWalletStateV1` from a validated BIP-39 mnemonic phrase.
    pub fn new(mnemonic_phrase: String, active_zone: String) -> Self {
        Self {
            mnemonic_phrase,
            upstream_storage_blob: Vec::new(),
            next_public_index: 0,
            next_private_index: 0,
            active_account_id: None,
            account_metadata: BTreeMap::new(),
            active_sessions: Vec::new(),
            tracked_tokens: Vec::new(),
            tx_journal: Vec::new(),
            active_zone,
            backup_meta: None,
        }
    }
}

/// Generate a fresh 24-word BIP-39 mnemonic (256 bits of cryptographic entropy).
pub fn generate_mnemonic_24() -> String {
    let mut entropy = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = Mnemonic::from_entropy(&entropy).expect("32 bytes is valid BIP-39 entropy");
    entropy.zeroize();
    mnemonic.to_string()
}

/// Validate and normalize a BIP-39 mnemonic phrase (12 or 24 words).
pub fn validate_mnemonic(phrase: &str) -> LezerResult<String> {
    let normalized = phrase
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let parsed: Mnemonic = normalized.parse().map_err(|e| {
        LezerError::new(
            ErrorCode::InvalidMnemonic,
            format!("BIP-39 mnemonic validation failed: {e}"),
        )
    })?;
    Ok(parsed.to_string())
}

/// Compute the canonical header MAC (`SHA-256` over `schema_version`, `kdf`, and `nonce_b64`).
pub fn compute_header_mac_hex(
    schema_version: u32,
    kdf: &Argon2ParamsRecord,
    nonce_b64: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lezer-vault-header-v1:");
    hasher.update(schema_version.to_le_bytes());
    hasher.update(kdf.algorithm.as_bytes());
    hasher.update(kdf.memory_kib.to_le_bytes());
    hasher.update(kdf.iterations.to_le_bytes());
    hasher.update(kdf.parallelism.to_le_bytes());
    hasher.update(kdf.salt_b64.as_bytes());
    hasher.update(nonce_b64.as_bytes());
    hex::encode(hasher.finalize())
}

/// Derive the 256-bit wrapping key from `password` and `kdf` using `Argon2id`.
pub fn derive_wrapping_key(password: &str, kdf: &Argon2ParamsRecord) -> LezerResult<[u8; 32]> {
    if kdf.algorithm != "argon2id" {
        return Err(LezerError::new(
            ErrorCode::VaultCorrupted,
            format!("unsupported KDF algorithm '{}'", kdf.algorithm),
        ));
    }
    let salt = base64::engine::general_purpose::STANDARD
        .decode(&kdf.salt_b64)
        .map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("invalid KDF salt base64: {e}"),
            )
        })?;
    let params = Params::new(
        kdf.memory_kib,
        kdf.iterations,
        kdf.parallelism,
        Some(32),
    )
    .map_err(|e| {
        LezerError::new(
            ErrorCode::VaultCorrupted,
            format!("invalid Argon2id parameters: {e}"),
        )
    })?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("Argon2id key derivation failed: {e}"),
            )
        })?;
    Ok(key)
}

impl SealedVaultEnvelopeV1 {
    /// Seal a [`CanonicalWalletStateV1`] with `password` using random salt and 192-bit nonce.
    pub fn seal(state: &CanonicalWalletStateV1, password: &str) -> LezerResult<Self> {
        let kdf = Argon2ParamsRecord::new_random_salt();
        let mut nonce = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut nonce);
        Self::seal_deterministic(state, password, kdf, &nonce)
    }

    /// Deterministic seal with caller-provided `kdf` and 24-byte `nonce` (used for golden test vectors).
    pub fn seal_deterministic(
        state: &CanonicalWalletStateV1,
        password: &str,
        kdf: Argon2ParamsRecord,
        nonce_bytes: &[u8; 24],
    ) -> LezerResult<Self> {
        Self::seal_with_version(
            CURRENT_VAULT_SCHEMA_VERSION,
            state,
            password,
            kdf,
            nonce_bytes,
        )
    }

    /// Seal a wallet state at an explicit `schema_version` (used by migration tests).
    pub fn seal_with_version(
        schema_version: u32,
        state: &CanonicalWalletStateV1,
        password: &str,
        kdf: Argon2ParamsRecord,
        nonce_bytes: &[u8; 24],
    ) -> LezerResult<Self> {
        let mut key = derive_wrapping_key(password, &kdf)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        key.zeroize();

        let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce_bytes);
        let header_mac_hex = compute_header_mac_hex(schema_version, &kdf, &nonce_b64);

        let mut plaintext = serde_json::to_vec(state).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("failed to serialize wallet state: {e}"),
            )
        })?;

        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(nonce_bytes),
                Payload {
                    msg: &plaintext,
                    aad: header_mac_hex.as_bytes(),
                },
            )
            .map_err(|_| {
                LezerError::new(
                    ErrorCode::VaultCorrupted,
                    "XChaCha20-Poly1305 encryption failed",
                )
            });
        plaintext.zeroize();
        let ciphertext = ciphertext?;

        Ok(Self {
            schema_version,
            kdf,
            nonce_b64,
            ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
            header_mac_hex,
        })
    }

    /// Unseal and authenticate a [`SealedVaultEnvelopeV1`] using `password`.
    /// Returns `LZR-1003` (`InvalidPassword`) if the password or authentication tag fails.
    pub fn unseal(&self, password: &str) -> LezerResult<CanonicalWalletStateV1> {
        let expected_mac =
            compute_header_mac_hex(self.schema_version, &self.kdf, &self.nonce_b64);
        if expected_mac != self.header_mac_hex {
            return Err(LezerError::new(
                ErrorCode::VaultCorrupted,
                "vault envelope header_mac_hex integrity check failed",
            ));
        }

        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.nonce_b64)
            .map_err(|e| {
                LezerError::new(
                    ErrorCode::VaultCorrupted,
                    format!("invalid vault nonce base64: {e}"),
                )
            })?;
        if nonce_bytes.len() != 24 {
            return Err(LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("expected 24-byte XChaCha20 nonce, got {}", nonce_bytes.len()),
            ));
        }

        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&self.ciphertext_b64)
            .map_err(|e| {
                LezerError::new(
                    ErrorCode::VaultCorrupted,
                    format!("invalid vault ciphertext base64: {e}"),
                )
            })?;

        let mut key = derive_wrapping_key(password, &self.kdf)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        key.zeroize();

        let mut plaintext = cipher
            .decrypt(
                XNonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &ciphertext,
                    aad: self.header_mac_hex.as_bytes(),
                },
            )
            .map_err(|_| {
                LezerError::new(
                    ErrorCode::InvalidPassword,
                    "failed to unlock vault: invalid password or authentication tag mismatch",
                )
            })?;

        let result = serde_json::from_slice::<CanonicalWalletStateV1>(&plaintext).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("decrypted vault payload is malformed JSON: {e}"),
            )
        });
        plaintext.zeroize();
        result
    }

    /// Write envelope to `path` using crash-safe **write-temp-fsync-rename** atomic persistence.
    pub fn write_atomic(&self, path: &Path) -> LezerResult<()> {
        let parent = path.parent().ok_or_else(|| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("vault path '{}' has no parent directory", path.display()),
            )
        })?;
        fs::create_dir_all(parent).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("failed to create vault directory '{}': {e}", parent.display()),
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }

        let pid = std::process::id();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("vault.enc");
        let tmp_path = parent.join(format!("{file_name}.tmp.{pid}"));

        let serialized = serde_json::to_vec_pretty(self).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("failed to serialize SealedVaultEnvelopeV1: {e}"),
            )
        })?;

        {
            let mut tmp_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .map_err(|e| {
                    LezerError::new(
                        ErrorCode::VaultCorrupted,
                        format!("failed to open temp vault file '{}': {e}", tmp_path.display()),
                    )
                })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = tmp_file.set_permissions(fs::Permissions::from_mode(0o600));
            }
            tmp_file.write_all(&serialized).map_err(|e| {
                LezerError::new(
                    ErrorCode::VaultCorrupted,
                    format!("failed to write temp vault file: {e}"),
                )
            })?;
            tmp_file.sync_all().map_err(|e| {
                LezerError::new(
                    ErrorCode::VaultCorrupted,
                    format!("failed to fsync temp vault file: {e}"),
                )
            })?;
        }

        fs::rename(&tmp_path, path).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!(
                    "failed to atomically rename '{}' -> '{}': {e}",
                    tmp_path.display(),
                    path.display()
                ),
            )
        })?;

        // Sync parent directory to make rename durable across power loss.
        if let Ok(dir_file) = File::open(parent) {
            let _ = dir_file.sync_all();
        }

        Ok(())
    }

    /// Read envelope from `path`. Returns `LZR-1001` (`VaultUninitialized`) if file does not exist.
    pub fn read_from_disk(path: &Path) -> LezerResult<Self> {
        if !path.exists() {
            return Err(LezerError::new(
                ErrorCode::VaultUninitialized,
                format!(
                    "no vault found at '{}' — run `lezer init` or `lezer restore` first",
                    path.display()
                ),
            ));
        }
        let bytes = fs::read(path).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("failed to read vault file '{}': {e}", path.display()),
            )
        })?;
        serde_json::from_slice::<Self>(&bytes).map_err(|e| {
            LezerError::new(
                ErrorCode::VaultCorrupted,
                format!("vault file '{}' is not a valid SealedVaultEnvelopeV1: {e}", path.display()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_unseal_roundtrip_and_wrong_password_rejected_with_lzr_1003() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        }
        let mnemonic = generate_mnemonic_24();
        let state = CanonicalWalletStateV1::new(mnemonic.clone(), "lez-testnet-0.4".to_string());
        let env = SealedVaultEnvelopeV1::seal(&state, "correct-horse-battery-staple").unwrap();

        let err = env.unseal("wrong-password").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidPassword);
        assert_eq!(err.code.as_str(), "LZR-1003");

        let recovered = env.unseal("correct-horse-battery-staple").unwrap();
        assert_eq!(recovered.mnemonic_phrase, mnemonic);
    }

    #[test]
    fn crash_mid_write_leaves_existing_vault_intact() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        }
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.enc");

        let state1 = CanonicalWalletStateV1::new(
            generate_mnemonic_24(),
            "lez-testnet-0.4".to_string(),
        );
        let env1 = SealedVaultEnvelopeV1::seal(&state1, "pw1").unwrap();
        env1.write_atomic(&vault_path).unwrap();

        // Simulate a crashed process leaving a truncated `.tmp.<pid>` file next to `vault.enc`
        let crashed_tmp = dir.path().join("vault.enc.tmp.99999");
        fs::write(&crashed_tmp, b"{\"schema_version\": 1, \"truncated").unwrap();

        // Reading `vault.enc` still succeeds and returns `state1` intact
        let disk_env = SealedVaultEnvelopeV1::read_from_disk(&vault_path).unwrap();
        let unlocked = disk_env.unseal("pw1").unwrap();
        assert_eq!(unlocked.mnemonic_phrase, state1.mnemonic_phrase);
    }
}
