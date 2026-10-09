//! Fungible Token Program (`TOKEN_ID`) facade (`crates/lezer-adapter/src/token.rs`).
//!
//! Implements LP-0013 and LP-0014 operations:
//! - `NewFungibleDefinition` (`TokenCreate`) with optional `mint_authority`
//! - `Mint` additional supply signed by `mint_authority`
//! - `Transfer` across ATAs and holding accounts
//! - `Burn` fungible tokens and decrement supply
//! - `SetAuthority` to rotate or permanently revoke (`None`) `mint_authority`

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::accounts:: PUBLIC_ACCOUNT_PREFIX;
use crate::ata::{DerivedAta, decode_id_bytes_32, derive_ata};

/// Canonical `TOKEN_ID` genesis program tag.
pub const TOKEN_PROGRAM_TAG: &[u8] = b"lez:token_program:v1";

/// Receipt produced when creating a new fungible token definition (`TOKEN_ID::NewFungibleDefinition`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenDefinitionReceipt {
    pub definition_id: String,
    pub symbol: String,
    pub decimals: u8,
    pub initial_supply: String,
    pub creator_account: String,
    pub creator_ata_id: String,
    pub mint_authority: Option<String>,
    pub tx_hash: String,
}

/// Receipt produced by a token or ATA mutation (`CreateAta`, `Transfer`, `Mint`, `Burn`, `SetAuthority`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOperationReceipt {
    pub operation: String,
    pub definition_id: String,
    pub from_account: Option<String>,
    pub to_account: Option<String>,
    pub ata_account_id: Option<String>,
    pub amount: Option<String>,
    pub new_authority: Option<String>,
    pub tx_hash: String,
}

/// Deterministically derive a new `TokenDefinition` address (`Public/<base58>`) from
/// `(creator_account, symbol, nonce_seed)`.
pub fn derive_token_definition_id(
    creator_account: &str,
    symbol: &str,
    initial_supply: u128,
) -> Result<(String, DerivedAta)> {
    let symbol_trimmed = symbol.trim();
    if symbol_trimmed.is_empty() || symbol_trimmed.len() > 16 {
        bail!("token symbol must be 1..=16 characters");
    }
    let creator_bytes = decode_id_bytes_32(creator_account)?;

    let mut hasher = Sha256::new();
    hasher.update(TOKEN_PROGRAM_TAG);
    hasher.update(creator_bytes);
    hasher.update(symbol_trimmed.as_bytes());
    hasher.update(initial_supply.to_le_bytes());
    let def_bytes: [u8; 32] = hasher.finalize().into();
    let def_id = format!(
        "{PUBLIC_ACCOUNT_PREFIX}{}",
        bs58::encode(def_bytes).into_string()
    );

    let creator_ata = derive_ata(creator_account, &def_id)?;
    Ok((def_id, creator_ata))
}

/// Compute a deterministic transaction hash for a token operation.
pub fn compute_token_tx_hash(op: &str, definition_id: &str, actor: &str, payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lez-token-tx-v1:");
    hasher.update(op.as_bytes());
    hasher.update(b":");
    hasher.update(definition_id.as_bytes());
    hasher.update(b":");
    hasher.update(actor.as_bytes());
    hasher.update(b":");
    hasher.update(payload);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::derive_public_account;

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn derives_token_definition_and_creator_ata() {
        let alice = derive_public_account(TEST_MNEMONIC, 0);
        let (def_id, ata) =
            derive_token_definition_id(&alice.account_id, "LEZERUSD", 1_000_000).unwrap();
        assert!(def_id.starts_with("Public/"));
        assert!(ata.ata_account_id.starts_with("Public/"));
        assert_eq!(ata.definition_id, def_id);
    }
}
