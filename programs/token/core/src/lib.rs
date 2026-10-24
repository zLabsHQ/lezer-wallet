//! Types and derivations shared by the `lezer_token` guest program and the wallet.
//!
//! A fungible token in LEZ v0.3 terms: this program writes only its own shard.
//! - **Token definition** lives in the program's shard on a *definition account* — a fresh
//!   public account the creator signs; `token id` = that account id.
//! - **Holdings** live in the program's shard on per-(token, owner) holding PDAs derived
//!   with [`holding_seed`]; balances are enumerable by exact-account reads.
//! - Spending requires the *owner* account to authorize the transaction (guests cannot see
//!   signatures — authorized accounts are how a program knows who approved).

#![no_std]
extern crate alloc;

use alloc::string::String;

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest as _, Sha256};

pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_SYMBOL_BYTES: usize = 16;

/// `Instruction` sent to the program (borsh). Account layouts are documented per variant.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Accounts: `[definition (signs), creator_holding]`.
    /// `authority: None` = renounced at definition → only `initial_supply` ever exists.
    Define {
        name: String,
        symbol: String,
        decimals: u8,
        authority: Option<[u8; 32]>,
        initial_supply: u128,
    },
    /// Accounts: `[authority (signs), definition, recipient_holding]`.
    Mint { owner: [u8; 32], amount: u128 },
    /// Accounts: `[owner (signs), definition, from_holding, to_holding]`.
    /// `definition` carries no effect; it is needed so the guest can derive both holdings.
    Transfer { to_owner: [u8; 32], amount: u128 },
    /// Accounts: `[owner (signs), definition, holding]`.
    Burn { amount: u128 },
    /// Accounts: `[current_authority (signs), definition]`; `None` renounces permanently.
    SetAuthority { new_authority: Option<[u8; 32]> },
    /// Accounts: `[entry, index_page]` — PDAs OF THE REGISTRY for THIS program.
    /// Emits a chained call to `params.registry`; no wallet signature required for the
    /// listing itself (the program authenticates itself), only a fee payer.
    RegisterSelf(registry_core::RegisterSelfParams),
}

/// Program's shard data on the definition account.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TokenDefinition {
    pub version: u8,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    /// Account allowed to mint and rotate authority; `None` = fixed supply.
    pub authority: Option<[u8; 32]>,
    /// Total ever minted minus burned.
    pub total_supply: u128,
}

/// Program's shard data on a holding account.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Holding {
    /// Definition account this holding belongs to (guards cross-token writes).
    pub definition: [u8; 32],
    pub balance: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    Define { def: TokenDefinition, initial_supply: u128 },
    /// `authority` is the account that authorized the change; apply checks it equals the
    /// definition's authority (or the definition itself for the initial mint).
    UpdateSupply { authority: [u8; 32], delta_minted: i128, delta_burned: i128 },
    Credit { definition: [u8; 32], amount: u128 },
    Spend { definition: [u8; 32], amount: u128 },
    SetAuthority { current: [u8; 32], new: Option<[u8; 32]> },
}

/// PDA seed of the holding account for (token `definition`, `owner`).
pub fn holding_seed(definition: &[u8; 32], owner: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lezer-token-holding-v1");
    h.update(definition);
    h.update(owner);
    h.finalize().into()
}

pub fn validate_definition(name: &str, symbol: &str, decimals: u8) -> Result<(), &'static str> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err("invalid token name");
    }
    if symbol.trim().is_empty() || symbol.len() > MAX_SYMBOL_BYTES {
        return Err("invalid token symbol");
    }
    if decimals > 18 {
        return Err("decimals out of range");
    }
    Ok(())
}

/// Apply a supply delta; refuses overflow / negative supply.
/// (Deltas are `i128` because a guest effect describes a signed change; amounts that do not
/// fit `i128` are rejected before a transaction is even planned.)
pub fn apply_supply_delta(current: u128, minted: i128, burned: i128) -> Result<u128, &'static str> {
    if minted < 0 || burned < 0 {
        return Err("negative delta");
    }
    let next = current
        .checked_add(minted as u128)
        .and_then(|v| v.checked_sub(burned as u128))
        .ok_or("supply overflow or burn exceeds supply")?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn seeds_are_stable_and_distinct() {
        let d = [1u8; 32];
        assert_eq!(holding_seed(&d, &[7; 32]), holding_seed(&d, &[7; 32]));
        assert_ne!(holding_seed(&d, &[7; 32]), holding_seed(&d, &[8; 32]));
        assert_ne!(holding_seed(&d, &[7; 32]), holding_seed(&[2; 32], &[7; 32]));
    }

    #[test]
    fn supply_math() {
        assert_eq!(apply_supply_delta(10, 5, 3), Ok(12));
        assert!(apply_supply_delta(10, 0, 11).is_err());
        assert!(apply_supply_delta(u128::MAX - 1, 5, 0).is_err());
    }

    #[test]
    fn definition_validation() {
        assert!(validate_definition("Lezer", "LZR", 9).is_ok());
        assert!(validate_definition("", "LZR", 9).is_err());
        assert!(validate_definition("Lezer", "LZR", 19).is_err());
    }
}
