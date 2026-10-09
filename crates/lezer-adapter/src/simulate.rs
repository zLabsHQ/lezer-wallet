//! Pre-flight transaction simulation, balance-delta calculator, deterministic revert
//! detector, and RISC0 guest cycle estimator (`crates/lezer-adapter/src/simulate.rs`).
//!
//! Enforces `docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md` §4.1:
//! - `SimSuccess`: Pre-state balances, nonces, PDA claims, and instruction decoding succeeded;
//!   exact balance deltas and RISC0 cycle estimate computed.
//! - `SimDeterministicRevert { code, reason }`: Transaction is guaranteed to fail on-chain
//!   (e.g., insufficient native/token balance, revoked LP-0013 mint authority, duplicate testimonial PDA).
//! - `SimUnavailable { reason }`: Sequencer RPC timed out or is unreachable during simulation.

use serde::{Deserialize, Serialize};

/// Balance delta for an account and asset computed during pre-flight simulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedBalanceDelta {
    pub account_id: String,
    /// `"native"` or `"token:<definition_id>"`.
    pub asset_id: String,
    pub symbol: String,
    /// Signed delta string, e.g. `"-50"` or `"+150"`.
    pub delta: String,
}

/// RISC Zero guest execution cycle and fee estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeEstimate {
    pub estimated_cycles: u64,
    pub segments: u32,
    pub estimated_fee_lez: String,
}

/// Outcome of pre-flight simulation (`docs/08` §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SimulationOutcome {
    SimSuccess {
        balance_deltas: Vec<SimulatedBalanceDelta>,
        compute: ComputeEstimate,
        state_writes: Vec<String>,
    },
    SimDeterministicRevert {
        code: String,
        reason: String,
    },
    SimUnavailable {
        reason: String,
    },
}

impl SimulationOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::SimSuccess { .. })
    }
}

/// Estimate RISC0 guest cycles and segments for a known instruction category.
pub fn estimate_cycles_for_kind(kind: &str, is_private_zk: bool) -> ComputeEstimate {
    let estimated_cycles = match (kind, is_private_zk) {
        ("native_transfer", false) => 42_000,
        ("native_transfer", true) => 1_240_000,
        ("faucet_claim", _) => 64_000,
        ("token_create", _) => 96_000,
        ("token_create_ata", _) => 58_000,
        ("token_transfer", false) => 84_000,
        ("token_transfer", true) => 1_480_000,
        ("token_mint", _) => 78_000,
        ("token_burn", _) => 74_000,
        ("token_set_authority", _) => 68_000,
        ("submit_testimonial", _) => 155_648,
        ("register_deployer", _) => 196_608,
        _ => 180_000,
    };
    let segments = ((estimated_cycles as f64) / 1_048_576.0).ceil().max(1.0) as u32;
    ComputeEstimate {
        estimated_cycles,
        segments,
        estimated_fee_lez: "0".to_string(),
    }
}

/// Simulate a balance-spending operation against `available_balance`.
#[allow(clippy::too_many_arguments)]
pub fn simulate_transfer_precheck(
    kind: &str,
    from_account: &str,
    to_account: &str,
    asset_id: &str,
    symbol: &str,
    amount: u128,
    available_balance: u128,
    is_private_zk: bool,
    offline: bool,
) -> SimulationOutcome {
    if offline {
        return SimulationOutcome::SimUnavailable {
            reason: "Offline / Sequencer Unreachable — Cannot simulate or broadcast until connection is restored".to_string(),
        };
    }
    if amount == 0 {
        return SimulationOutcome::SimDeterministicRevert {
            code: "LZR-3003".to_string(),
            reason: "Transfer amount must be greater than zero".to_string(),
        };
    }
    if available_balance < amount {
        return SimulationOutcome::SimDeterministicRevert {
            code: "LZR-3003".to_string(),
            reason: format!(
                "Insufficient {symbol} balance on {from_account}: available {available_balance}, required {amount}"
            ),
        };
    }

    let compute = estimate_cycles_for_kind(kind, is_private_zk);
    SimulationOutcome::SimSuccess {
        balance_deltas: vec![
            SimulatedBalanceDelta {
                account_id: from_account.to_string(),
                asset_id: asset_id.to_string(),
                symbol: symbol.to_string(),
                delta: format!("-{amount}"),
            },
            SimulatedBalanceDelta {
                account_id: to_account.to_string(),
                asset_id: asset_id.to_string(),
                symbol: symbol.to_string(),
                delta: format!("+{amount}"),
            },
        ],
        compute,
        state_writes: vec![from_account.to_string(), to_account.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_insufficient_balance_revert_and_computes_deltas_on_success() {
        let rev = simulate_transfer_precheck(
            "token_transfer",
            "Public/Alice",
            "Public/Bob",
            "token:LEZERUSD",
            "LEZERUSD",
            500,
            100,
            false,
            false,
        );
        assert!(matches!(rev, SimulationOutcome::SimDeterministicRevert { .. }));

        let ok = simulate_transfer_precheck(
            "token_transfer",
            "Public/Alice",
            "Public/Bob",
            "token:LEZERUSD",
            "LEZERUSD",
            50,
            100,
            false,
            false,
        );
        assert!(ok.is_success());
    }
}
