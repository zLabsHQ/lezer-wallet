//! Canonical `TransactionSummary` builder, 4-state LP-0023 `SourceVerificationBadge`,
//! and security warning gates (`crates/lezer-core/src/summary.rs`).
//!
//! Enforces the 5-Step Immutable Transaction & Approval Pipeline (`docs/08` §2.2):
//! - Computes `summary_digest_sha256` (`SHA-256` over canonical `TransactionSummary` + `TxIntent`).
//! - Attaches the 4-state `SourceVerificationBadge` (`VerifiedSource`, `KnownButUnverified`,
//!   `NoRegistryEntry`, `MismatchOrError`).
//! - Attaches mandatory gates (`WARN_PUBLIC_LINKABILITY`, `WARN_AUTHORITY_OR_UNLIMITED_GRANT`,
//!   `WARN_UNVERIFIED_PROGRAM`).

use std::collections::BTreeMap;

use lezer_adapter::{ComputeEstimate, SimulatedBalanceDelta, SimulationOutcome};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// 4-state LP-0023 Source Verification Status (`docs/04` §3.3 & `docs/08` §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBadgeState {
    VerifiedSource,
    KnownButUnverified,
    NoRegistryEntry,
    MismatchOrError,
}

/// Rich LP-0023 verification badge attached to every `TransactionSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceVerificationBadge {
    pub state: VerificationBadgeState,
    pub label: String,
    pub program_name: String,
    pub program_version: Option<String>,
    pub program_id_hex: String,
    pub repo_url: Option<String>,
    pub commit_hash: Option<String>,
    pub idl_cid: Option<String>,
    pub source_cid: Option<String>,
    pub requires_unverified_acknowledgement: bool,
    pub blocked_by_mismatch: bool,
}

/// Severity of a security warning on the Approval Sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Info,
    AmberGate,
    CriticalRedOverride,
}

/// High-visibility security or privacy notice rendered in `ApprovalSheet.qml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityWarning {
    pub code: String,
    pub severity: WarningSeverity,
    pub title: String,
    pub message: String,
}

/// Canonical `TransactionSummary` rendered by stateless UI layers (`ApprovalSheet.qml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub proposal_id: String,
    pub originating_dapp: String,
    pub signing_account: String,
    pub privacy_mode: String,
    pub program_id_hex: String,
    pub program_name: String,
    pub instruction_name: String,
    /// Human-readable one-line headline, e.g. `"Transfer 250 LEZERUSD from alice to bob (ATA)"`.
    pub headline: String,
    /// Pre-flight simulation outcome & balance deltas.
    pub simulation: SimulationOutcome,
    /// Explicit balance outflows/inflows.
    pub balance_deltas: Vec<SimulatedBalanceDelta>,
    /// Authority or approval mutations (e.g. `SetAuthority`, `Mint`).
    pub authority_changes: Vec<String>,
    /// Decoded key-value instruction arguments.
    pub decoded_args: BTreeMap<String, String>,
    /// 4-state LP-0023 Source Verification Badge.
    pub verification_badge: SourceVerificationBadge,
    /// Estimated RISC0 execution cycles and fee.
    pub compute_estimate: ComputeEstimate,
    /// High-visibility security and privacy warnings.
    pub warnings: Vec<SecurityWarning>,
    /// Whether signing requires `acknowledged_unverified == true` in `ApproveDecision`.
    pub requires_unverified_ack: bool,
    /// Whether signing requires `critical_override == true` in `ApproveDecision` (Red Override Gate).
    pub requires_critical_override: bool,
    /// Raw hex fallback of instruction data.
    pub raw_instruction_hex: String,
    /// Canonical SHA-256 hex digest over `(proposal_id, signing_account, headline, decoded_args, raw_instruction_hex)`.
    pub summary_digest_hex: String,
}

impl TransactionSummary {
    /// Compute the deterministic `summary_digest_hex` (`SHA-256`) binding the previewed
    /// summary to the underlying unsigned intent.
    pub fn compute_digest_hex(
        proposal_id: &str,
        signing_account: &str,
        program_id_hex: &str,
        instruction_name: &str,
        headline: &str,
        decoded_args: &BTreeMap<String, String>,
        raw_instruction_hex: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"lezer-tx-summary-digest-v1:");
        hasher.update(proposal_id.as_bytes());
        hasher.update(b"|");
        hasher.update(signing_account.as_bytes());
        hasher.update(b"|");
        hasher.update(program_id_hex.as_bytes());
        hasher.update(b"|");
        hasher.update(instruction_name.as_bytes());
        hasher.update(b"|");
        hasher.update(headline.as_bytes());
        hasher.update(b"|");
        for (k, v) in decoded_args {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b";");
        }
        hasher.update(raw_instruction_hex.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Small decision object returned by `ApprovalSheet.qml` / CLI when the user clicks Approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApproveDecision {
    pub proposal_id: String,
    pub summary_digest_hex: String,
    pub acknowledged_unverified: bool,
    pub critical_override: bool,
}
