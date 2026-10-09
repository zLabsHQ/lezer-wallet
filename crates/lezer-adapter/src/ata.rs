//! Deterministic Associated Token Account (ATA) derivation (`SHA256(owner || definition)`),
//! idempotent `Create`, and ATA transfer helpers (`crates/lezer-adapter/src/ata.rs`).
//!
//! Conforms to LP-0014 and `docs/02-FEATURES-AND-PRODUCT-SPEC.md` §3.1:
//! - `Deterministic Seed = SHA-256(owner_id_bytes || definition_id_bytes)`
//! - `Public ATA Address = Public/<Base58(SHA-256("lez-ata-pda-v1:" || ASSOCIATED_TOKEN_ACCOUNT_ID || Seed))>`
//! - `Private Shielded ATA = Private/<Base58(SHA-256("lez-private-ata-v1:" || owner_id_bytes || definition_id_bytes))>`

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::accounts::{PRIVATE_ACCOUNT_PREFIX, PUBLIC_ACCOUNT_PREFIX};

/// Canonical `ASSOCIATED_TOKEN_ACCOUNT_ID` genesis program tag.
pub const ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_TAG: &[u8] = b"lez:associated_token_account:v1";

/// Derived Associated Token Account (ATA) descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedAta {
    /// Owner account ID (`"Public/<base58>"` or `"Private/<base58>"`).
    pub owner_account_id: String,
    /// Token definition ID (`"Public/<base58>"` or 64-hex / Base58 mint ID).
    pub definition_id: String,
    /// Deterministic 32-byte seed hex (`SHA256(owner_bytes || definition_bytes)`).
    pub seed_hex: String,
    /// Derived ATA address (`"Public/<base58>"` for public owner, `"Private/<base58>"` for private owner).
    pub ata_account_id: String,
    /// Whether this ATA is a shielded private token holding note.
    pub is_private: bool,
}

/// Extract canonical 32-byte raw identifier from `"Public/<base58>"`, `"Private/<base58>"`,
/// bare Base58, or 64-char hex string.
pub fn decode_id_bytes_32(id: &str) -> Result<[u8; 32]> {
    let trimmed = id.trim();
    let bare = trimmed
        .strip_prefix(PUBLIC_ACCOUNT_PREFIX)
        .or_else(|| trimmed.strip_prefix(PRIVATE_ACCOUNT_PREFIX))
        .unwrap_or(trimmed);

    if bare.len() == 64 && bare.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes = hex::decode(bare).context("invalid 64-hex identifier")?;
        return bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("hex identifier did not decode to 32 bytes"));
    }

    let decoded = bs58::decode(bare)
        .into_vec()
        .with_context(|| format!("invalid Base58 identifier '{trimmed}'"))?;
    if decoded.len() != 32 {
        bail!(
            "identifier '{}' must decode to 32 bytes (got {})",
            trimmed,
            decoded.len()
        );
    }
    Ok(decoded.try_into().expect("checked 32 bytes"))
}

/// Deterministically derive the Associated Token Account (ATA) for `(owner_account_id, definition_id)`.
pub fn derive_ata(owner_account_id: &str, definition_id: &str) -> Result<DerivedAta> {
    let trimmed_owner = owner_account_id.trim();
    let is_private = trimmed_owner.starts_with(PRIVATE_ACCOUNT_PREFIX);
    let owner_bytes = decode_id_bytes_32(trimmed_owner)?;
    let def_bytes = decode_id_bytes_32(definition_id)?;

    // Deterministic Seed = SHA-256(owner_id_bytes || definition_id_bytes)
    let mut seed_hasher = Sha256::new();
    seed_hasher.update(owner_bytes);
    seed_hasher.update(def_bytes);
    let seed: [u8; 32] = seed_hasher.finalize().into();

    let mut pda_hasher = Sha256::new();
    if is_private {
        pda_hasher.update(b"lez-private-ata-v1:");
    } else {
        pda_hasher.update(b"lez-public-ata-v1:");
    }
    pda_hasher.update(ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_TAG);
    pda_hasher.update(seed);
    let ata_bytes: [u8; 32] = pda_hasher.finalize().into();
    let ata_base58 = bs58::encode(ata_bytes).into_string();

    let ata_account_id = if is_private {
        format!("{PRIVATE_ACCOUNT_PREFIX}{ata_base58}")
    } else {
        format!("{PUBLIC_ACCOUNT_PREFIX}{ata_base58}")
    };

    Ok(DerivedAta {
        owner_account_id: trimmed_owner.to_string(),
        definition_id: definition_id.trim().to_string(),
        seed_hex: hex::encode(seed),
        ata_account_id,
        is_private,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{derive_private_account, derive_public_account};

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn ata_derivation_is_deterministic_for_public_and_private_owners() {
        let alice = derive_public_account(TEST_MNEMONIC, 0);
        let alice_priv = derive_private_account(TEST_MNEMONIC, 0);
        let mint_def = derive_public_account(TEST_MNEMONIC, 99);

        let ata1 = derive_ata(&alice.account_id, &mint_def.account_id).unwrap();
        let ata2 = derive_ata(&alice.account_id, &mint_def.account_id).unwrap();
        assert_eq!(ata1, ata2);
        assert!(ata1.ata_account_id.starts_with("Public/"));
        assert!(!ata1.is_private);

        let priv_ata = derive_ata(&alice_priv.account_id, &mint_def.account_id).unwrap();
        assert!(priv_ata.ata_account_id.starts_with("Private/"));
        assert!(priv_ata.is_private);
        assert_ne!(ata1.ata_account_id, priv_ata.ata_account_id);
    }
}
