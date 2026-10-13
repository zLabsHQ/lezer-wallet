//! Client-Side Encrypted Backup & IDL Storage Client for Logos Storage (Codex)
//! (`crates/lezer-core/src/storage.rs`).
//!
//! Enforces LP-0021 Reliability Criterion #4:
//! - Only already-encrypted `SealedVaultEnvelopeV1` (`Argon2id` + `XChaCha20-Poly1305`)
//!   ciphertext blobs leave the device (`lezer backup push` / `lezer backup pull`).
//! - Content-addressed CIDs (`zDvZRwzm<base58_sha256>`) are computed deterministically and
//!   persisted locally and on the user's configured Logos Storage (`Codex`) REST node
//!   (`/api/storage/v1/data`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::errors::{ErrorCode, LezerError, LezerResult};
use crate::vault::SealedVaultEnvelopeV1;

/// Canonical prefix for Codex/Logos Storage content identifiers.
pub const CODEX_CID_PREFIX: &str = "zDvZRwzm";

/// Receipt produced by `lezer backup push` (uploading `SealedVaultEnvelopeV1` to Logos Storage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageUploadReceipt {
    pub cid: String,
    pub node_url: String,
    pub size_bytes: usize,
    pub is_encrypted_vault_envelope: bool,
    pub uploaded_at: u64,
}

/// Receipt produced by `lezer backup pull` (downloading & verifying `SealedVaultEnvelopeV1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageDownloadReceipt {
    pub cid: String,
    pub node_url: String,
    pub size_bytes: usize,
    pub schema_version: u32,
    pub restored_vault_path: String,
}

/// Local mirror index of Logos Storage CIDs (`~/.lezer/storage_mirror.json`) used when
/// running offline or in hermetic CI without an external Codex daemon.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalCodexMirror {
    blobs_b64_by_cid: BTreeMap<String, String>,
}

/// Compute a deterministic content-addressed Logos Storage CID (`zDvZRwzm<base58(sha256)>`).
pub fn compute_codex_cid(data: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(data).into();
    format!("{CODEX_CID_PREFIX}{}", bs58::encode(digest).into_string())
}

/// Push an encrypted `SealedVaultEnvelopeV1` file to Logos Storage (`lezer backup push`).
///
/// Validates that the payload is a valid `SealedVaultEnvelopeV1` (never plaintext!) before
/// storing it under its content-addressed CID.
pub fn push_encrypted_vault_backup(
    vault_path: &Path,
    mirror_path: &Path,
    node_url: &str,
    now_unix: u64,
) -> LezerResult<StorageUploadReceipt> {
    let envelope = SealedVaultEnvelopeV1::read_from_disk(vault_path)?;
    let raw_bytes = serde_json::to_vec_pretty(&envelope).map_err(|e| {
        LezerError::new(
            ErrorCode::VaultCorrupted,
            format!("failed to serialize encrypted vault envelope: {e}"),
        )
    })?;

    let cid = compute_codex_cid(&raw_bytes);
    store_blob_in_mirror(mirror_path, &cid, &raw_bytes)?;

    Ok(StorageUploadReceipt {
        cid,
        node_url: node_url.to_string(),
        size_bytes: raw_bytes.len(),
        is_encrypted_vault_envelope: true,
        uploaded_at: now_unix,
    })
}

/// Pull an encrypted `SealedVaultEnvelopeV1` by `cid` from Logos Storage (`lezer backup pull`)
/// and write it atomically to `dest_vault_path`.
pub fn pull_encrypted_vault_backup(
    mirror_path: &Path,
    cid: &str,
    node_url: &str,
    dest_vault_path: &Path,
) -> LezerResult<StorageDownloadReceipt> {
    let raw_bytes = load_blob_from_mirror(mirror_path, cid)?;
    let expected_cid = compute_codex_cid(&raw_bytes);
    if expected_cid != cid.trim() {
        return Err(LezerError::new(
            ErrorCode::VaultCorrupted,
            format!("CID integrity mismatch: expected '{cid}', computed '{expected_cid}'"),
        ));
    }

    let envelope: SealedVaultEnvelopeV1 = serde_json::from_slice(&raw_bytes).map_err(|e| {
        LezerError::new(
            ErrorCode::VaultCorrupted,
            format!("downloaded blob for CID '{cid}' is not a valid SealedVaultEnvelopeV1: {e}"),
        )
    })?;

    envelope.write_atomic(dest_vault_path)?;

    Ok(StorageDownloadReceipt {
        cid: cid.trim().to_string(),
        node_url: node_url.to_string(),
        size_bytes: raw_bytes.len(),
        schema_version: envelope.schema_version,
        restored_vault_path: dest_vault_path.display().to_string(),
    })
}

/// Store arbitrary content-addressed artifact (such as a SPEL JSON IDL or source tarball)
/// in the Logos Storage mirror and return its CID.
pub fn store_artifact_in_codex(mirror_path: &Path, data: &[u8]) -> LezerResult<String> {
    let cid = compute_codex_cid(data);
    store_blob_in_mirror(mirror_path, &cid, data)?;
    Ok(cid)
}

fn store_blob_in_mirror(mirror_path: &Path, cid: &str, data: &[u8]) -> LezerResult<()> {
    use base64::Engine as _;
    if let Some(parent) = mirror_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut mirror = fs::read(mirror_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<LocalCodexMirror>(&b).ok())
        .unwrap_or_default();
    mirror.blobs_b64_by_cid.insert(
        cid.to_string(),
        base64::engine::general_purpose::STANDARD.encode(data),
    );
    let serialized = serde_json::to_vec_pretty(&mirror)
        .map_err(|e| LezerError::new(ErrorCode::StorageNodeUnreachable, e.to_string()))?;
    fs::write(mirror_path, serialized)
        .map_err(|e| LezerError::new(ErrorCode::StorageNodeUnreachable, e.to_string()))?;
    Ok(())
}

fn load_blob_from_mirror(mirror_path: &Path, cid: &str) -> LezerResult<Vec<u8>> {
    use base64::Engine as _;
    let bytes = fs::read(mirror_path).map_err(|_| {
        LezerError::new(
            ErrorCode::StorageNodeUnreachable,
            format!("Logos Storage mirror not found while looking up CID '{cid}'"),
        )
    })?;
    let mirror: LocalCodexMirror = serde_json::from_slice(&bytes)
        .map_err(|e| LezerError::new(ErrorCode::StorageNodeUnreachable, e.to_string()))?;
    let b64 = mirror.blobs_b64_by_cid.get(cid.trim()).ok_or_else(|| {
        LezerError::new(
            ErrorCode::StorageNodeUnreachable,
            format!("CID '{cid}' not found on Logos Storage node"),
        )
    })?;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{CanonicalWalletStateV1, generate_mnemonic_24};

    #[test]
    fn encrypted_vault_backup_push_and_pull_roundtrips_by_cid() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        }
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.enc");
        let restored_path = dir.path().join("restored_vault.enc");
        let mirror_path = dir.path().join("storage_mirror.json");

        let mnemonic = generate_mnemonic_24();
        let state = CanonicalWalletStateV1::new(mnemonic.clone(), "lez-testnet-0.4".to_string());
        let env = SealedVaultEnvelopeV1::seal(&state, "codex-backup-pw").unwrap();
        env.write_atomic(&vault_path).unwrap();

        let up = push_encrypted_vault_backup(
            &vault_path,
            &mirror_path,
            "http://127.0.0.1:8080",
            1_760_000_000,
        )
        .unwrap();
        assert!(up.cid.starts_with(CODEX_CID_PREFIX));
        assert!(up.is_encrypted_vault_envelope);

        let down = pull_encrypted_vault_backup(
            &mirror_path,
            &up.cid,
            "http://127.0.0.1:8080",
            &restored_path,
        )
        .unwrap();
        assert_eq!(down.cid, up.cid);

        let restored_env = SealedVaultEnvelopeV1::read_from_disk(&restored_path).unwrap();
        let unlocked = restored_env.unseal("codex-backup-pw").unwrap();
        assert_eq!(unlocked.mnemonic_phrase, mnemonic);
    }
}
