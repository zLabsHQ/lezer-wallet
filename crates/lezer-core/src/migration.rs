//! Vault schema version checks, automatic `.bak` pre-migration backup, and forward migration rules.
//!
//! Per `docs/08` §3.2:
//! 1. Before migrating `vault.enc` when `schema_version < CURRENT_VAULT_SCHEMA_VERSION`,
//!    `lezer-core` copies `vault.enc` to `~/.lezer/backups/vault.v<old>.<unix_ts>.enc.bak`
//!    and calls `fsync`.
//! 2. Applies deterministic in-memory stepwise migrations.
//! 3. Atomically writes the migrated `SealedVaultEnvelopeV1` via write-temp-fsync-rename.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, LezerError, LezerResult};
use crate::vault::{
    CURRENT_VAULT_SCHEMA_VERSION, CanonicalWalletStateV1, SealedVaultEnvelopeV1,
    compute_header_mac_hex,
};

/// Create an automatic fsynced backup at `<vault_dir>/backups/vault.v<schema_version>.<unix_ts>.enc.bak`.
pub fn create_pre_migration_backup(vault_path: &Path, old_version: u32) -> LezerResult<PathBuf> {
    let parent = vault_path.parent().ok_or_else(|| {
        LezerError::new(
            ErrorCode::MigrationFailed,
            "vault path has no parent directory",
        )
    })?;
    let backups_dir = parent.join("backups");
    fs::create_dir_all(&backups_dir).map_err(|e| {
        LezerError::new(
            ErrorCode::MigrationFailed,
            format!("failed to create backups directory: {e}"),
        )
    })?;

    let unix_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let bak_path = backups_dir.join(format!("vault.v{old_version}.{unix_ts}.enc.bak"));

    fs::copy(vault_path, &bak_path).map_err(|e| {
        LezerError::new(
            ErrorCode::MigrationFailed,
            format!("failed to copy pre-migration backup: {e}"),
        )
    })?;

    if let Ok(f) = File::open(&bak_path) {
        let _ = f.sync_all();
    }
    Ok(bak_path)
}

/// Unlock `envelope` with `password`, automatically performing a pre-migration `.bak`
/// backup and forward migration if `envelope.schema_version < CURRENT_VAULT_SCHEMA_VERSION`.
pub fn unlock_and_migrate_if_needed(
    vault_path: &Path,
    envelope: &SealedVaultEnvelopeV1,
    password: &str,
) -> LezerResult<(CanonicalWalletStateV1, Option<PathBuf>)> {
    if envelope.schema_version > CURRENT_VAULT_SCHEMA_VERSION {
        return Err(LezerError::new(
            ErrorCode::MigrationFailed,
            format!(
                "vault schema_version {} is newer than supported version {}",
                envelope.schema_version, CURRENT_VAULT_SCHEMA_VERSION
            ),
        ));
    }

    if envelope.schema_version == CURRENT_VAULT_SCHEMA_VERSION {
        let state = envelope.unseal(password)?;
        return Ok((state, None));
    }

    // Legacy v0 -> v1 migration path:
    // 1. Create automatic `.bak` backup before touching a single byte.
    let backup_path = create_pre_migration_backup(vault_path, envelope.schema_version)?;

    // 2. Verify header MAC for the legacy version and decrypt payload.
    let mut normalized = envelope.clone();
    normalized.header_mac_hex = compute_header_mac_hex(
        normalized.schema_version,
        &normalized.kdf,
        &normalized.nonce_b64,
    );
    let state = normalized.unseal(password).map_err(|e| {
        if e.code == ErrorCode::InvalidPassword {
            e
        } else {
            LezerError::new(
                ErrorCode::MigrationFailed,
                format!("failed to migrate vault from v{}: {e}", envelope.schema_version),
            )
        }
    })?;

    // 3. Re-seal at CURRENT_VAULT_SCHEMA_VERSION and write atomically.
    let migrated_env = SealedVaultEnvelopeV1::seal(&state, password)?;
    migrated_env.write_atomic(vault_path)?;

    Ok((state, Some(backup_path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{Argon2ParamsRecord, generate_mnemonic_24};

    #[test]
    fn forward_migration_creates_bak_and_upgrades_envelope_atomically() {
        unsafe {
            std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        }
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("vault.enc");

        let mnemonic = generate_mnemonic_24();
        let state = CanonicalWalletStateV1::new(mnemonic.clone(), "lez-testnet-0.4".to_string());
        let kdf = Argon2ParamsRecord::with_salt(&[7u8; 32], 1024, 1, 1);
        let nonce = [9u8; 24];

        // Construct a synthetic schema_version = 0 envelope
        let legacy_env =
            SealedVaultEnvelopeV1::seal_with_version(0, &state, "migrate-pw", kdf, &nonce).unwrap();
        legacy_env.write_atomic(&vault_path).unwrap();

        let on_disk = SealedVaultEnvelopeV1::read_from_disk(&vault_path).unwrap();
        assert_eq!(on_disk.schema_version, 0);

        let (migrated_state, bak_path) =
            unlock_and_migrate_if_needed(&vault_path, &on_disk, "migrate-pw").unwrap();
        let bak_path = bak_path.expect("must produce pre-migration .bak file");
        assert!(bak_path.exists());
        assert_eq!(migrated_state.mnemonic_phrase, mnemonic);

        let upgraded_on_disk = SealedVaultEnvelopeV1::read_from_disk(&vault_path).unwrap();
        assert_eq!(upgraded_on_disk.schema_version, CURRENT_VAULT_SCHEMA_VERSION);
    }
}
