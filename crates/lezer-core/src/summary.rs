//! Canonical `TransactionSummary` (what the user approves) and the `ApproveDecision` the UI
//! returns. The digest binds every displayed field; the UI cannot alter what gets signed.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Program provenance shown on the approval sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBadgeState {
    /// Built into the LEZ protocol (e.g. native token); shipped with the node.
    ProtocolBuiltin,
    /// LP-0023 registry entry exists and a reproducible rebuild matches the deployed ID.
    VerifiedSource,
    /// Registry entry / IDL exists, but source has not been verified.
    KnownButUnverified,
    /// No registry entry — requires explicit acknowledgement.
    NoRegistryEntry,
    /// Rebuild contradicts the deployed ID — blocked unless critically overridden.
    MismatchOrError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceVerificationBadge {
    pub state: VerificationBadgeState,
    pub label: String,
    pub program_name: String,
    pub program_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Info,
    AmberGate,
    CriticalRedOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityWarning {
    pub code: String,
    pub severity: WarningSeverity,
    pub title: String,
    pub message: String,
}

/// Result of checking the sender can cover the amount before asking the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Precheck {
    Passed { available: String },
    InsufficientFunds { available: String, required: String },
    /// Balance could not be read (e.g. offline). Approval is still possible; the network
    /// will reject the transaction if funds are insufficient.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub proposal_id: String,
    pub originating_dapp: String,
    pub signing_account: String,
    /// `public`, `shielded`, `deshielded`, or `private`.
    pub privacy_mode: String,
    pub requires_zk_proof: bool,
    pub instruction_name: String,
    pub headline: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    /// Upper bound on the fee the sender may be charged (public transactions).
    pub max_fee: Option<String>,
    pub precheck: Precheck,
    pub verification_badge: SourceVerificationBadge,
    pub warnings: Vec<SecurityWarning>,
    pub requires_unverified_ack: bool,
    pub requires_critical_override: bool,
    /// SHA-256 over the canonical JSON of every field above.
    pub summary_digest_hex: String,
}

impl TransactionSummary {
    /// Compute and set `summary_digest_hex` from all other fields.
    pub fn seal_digest(mut self) -> Self {
        self.summary_digest_hex = String::new();
        let canonical = serde_json::to_vec(&self).expect("summary serializes");
        let mut h = Sha256::new();
        h.update(b"lezer-tx-summary-v2:");
        h.update(&canonical);
        self.summary_digest_hex = hex::encode(h.finalize());
        self
    }

    pub fn digest_is_valid(&self) -> bool {
        self.clone().seal_digest().summary_digest_hex == self.summary_digest_hex
    }

    pub fn blocks_approval(&self) -> bool {
        matches!(self.precheck, Precheck::InsufficientFunds { .. })
    }
}

/// Returned by the approval UI / CLI. Carries no transaction data, only the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApproveDecision {
    pub proposal_id: String,
    pub summary_digest_hex: String,
    pub acknowledged_unverified: bool,
    pub critical_override: bool,
}
