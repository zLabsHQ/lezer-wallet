//! Shared types and PDA derivations for `lezer_faucet` (LP-0021 on-chain faucet program).
//!
//! Model:
//! - The faucet program holds native LEZ in its treasury PDA (`VAULT_SEED`) and dispenses
//!   [`DEFAULT_DRIP_AMOUNT`] (50 LEZ) to a recipient via a chained `custody_transfer` call
//!   to the built-in `native_token` program.
//! - Rate-limiting is enforced on-chain: each recipient has a deterministic claim-ticket PDA
//!   (`claim_seed(recipient)`) on the faucet program's shard. If a [`ClaimRecord`] already
//!   exists on that shard, both the wallet's local pre-check and the guest's `apply` reject
//!   a duplicate claim.

#![no_std]
extern crate alloc;

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest as _, Sha256};

/// Default faucet drip: 50 LEZ (`50 * 10^9` atomic units).
pub const DEFAULT_DRIP_AMOUNT: u128 = 50_000_000_000;

/// Fixed PDA seed of the faucet's native-LEZ treasury account.
pub const VAULT_SEED: [u8; 32] = *b"lezer-faucet-treasury-vault-v001";

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Dispense [`DEFAULT_DRIP_AMOUNT`] native LEZ from the faucet treasury PDA to `recipient`.
    /// Expected input accounts:
    ///   0 - `vault_pda` (`native_token` shard)
    ///   1 - `recipient` (authorized / signs, `native_token` shard)
    ///   2 - `claim_ticket_pda` (`lezer_faucet` shard)
    Claim,
    /// Self-register `lezer_faucet` in `lezer_registry` via chained call.
    /// Expected input accounts: `[entry, index_page]` (registry PDAs).
    RegisterSelf(registry_core::RegisterSelfParams),
}

/// Stored on the `claim_ticket_pda` shard after a successful claim.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ClaimRecord {
    pub version: u8,
    pub recipient: [u8; 32],
    pub claims: u32,
    pub total_claimed: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    RecordClaim { recipient: [u8; 32], amount: u128 },
}

/// Deterministic PDA seed for `recipient`'s claim-ticket account.
pub fn claim_seed(recipient: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lezer-faucet-claim-v1");
    h.update(recipient);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_seeds_are_deterministic_and_distinct() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(claim_seed(&a), claim_seed(&a));
        assert_ne!(claim_seed(&a), claim_seed(&b));
        assert_ne!(claim_seed(&a), VAULT_SEED);
    }

    #[test]
    fn claim_record_round_trips() {
        let rec = ClaimRecord {
            version: 1,
            recipient: [9u8; 32],
            claims: 1,
            total_claimed: DEFAULT_DRIP_AMOUNT,
        };
        let bytes = borsh::to_vec(&rec).expect("serialize");
        let decoded = ClaimRecord::try_from_slice(&bytes).expect("deserialize");
        assert_eq!(decoded, rec);
    }
}
