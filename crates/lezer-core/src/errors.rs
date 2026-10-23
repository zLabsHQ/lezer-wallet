//! Unified error taxonomy (`LZR-1xxx` through `LZR-6xxx`) for Lezer Wallet.
//!
//! All surfaces (`lezer-core`, `lezer_wallet_provider.lidl`, `lezer` CLI,
//! `LezerWalletView.qml`, and `@lezer/provider`) share one structured error
//! envelope `{ code: "LZR-xxxx", category, message, retryable }`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error category matching the `LZR-1xxx` .. `LZR-6xxx` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    VaultAndAuth,
    SessionAndAcl,
    ApprovalAndSimulation,
    NetworkAndSequencer,
    ProverAndCrypto,
    RegistryAndStorage,
}

/// Canonical error codes across all Lezer surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    // ── LZR-1xxx: Vault & Auth ───────────────────────────────────────────
    #[serde(rename = "LZR-1001")]
    VaultUninitialized,
    #[serde(rename = "LZR-1002")]
    VaultLocked,
    #[serde(rename = "LZR-1003")]
    InvalidPassword,
    #[serde(rename = "LZR-1004")]
    InvalidMnemonic,
    #[serde(rename = "LZR-1005")]
    MigrationFailed,
    #[serde(rename = "LZR-1006")]
    VaultCorrupted,
    #[serde(rename = "LZR-1007")]
    DuplicateAccountLabel,
    #[serde(rename = "LZR-1008")]
    AccountNotFound,
    #[serde(rename = "LZR-1009")]
    VaultAlreadyExists,
    #[serde(rename = "LZR-1010")]
    InvalidAddress,

    // ── LZR-2xxx: Session & ACL ──────────────────────────────────────────
    #[serde(rename = "LZR-2001")]
    SessionExpiredOrInvalid,
    #[serde(rename = "LZR-2002")]
    AccountNotAuthorized,
    #[serde(rename = "LZR-2003")]
    PrivateBalanceForbidden,
    #[serde(rename = "LZR-2004")]
    UiControlTokenInvalid,

    // ── LZR-3xxx: Approval & Simulation ──────────────────────────────────
    #[serde(rename = "LZR-3001")]
    UserRejected,
    #[serde(rename = "LZR-3002")]
    ProposalExpired,
    #[serde(rename = "LZR-3003")]
    SimulationReverted,
    #[serde(rename = "LZR-3004")]
    UnacknowledgedRiskGate,
    #[serde(rename = "LZR-3005")]
    ProposalDigestMismatch,
    #[serde(rename = "LZR-3006")]
    TransactionRejected,

    // ── LZR-4xxx: Network & Sequencer ────────────────────────────────────
    #[serde(rename = "LZR-4001")]
    SequencerUnreachable,
    #[serde(rename = "LZR-4002")]
    DisallowedEndpointBlocked,
    #[serde(rename = "LZR-4003")]
    ShardContentionExhausted,
    #[serde(rename = "LZR-4004")]
    FaucetCooldownOrDepleted,

    // ── LZR-5xxx: Prover & Crypto ────────────────────────────────────────
    #[serde(rename = "LZR-5001")]
    ProverQueueBusy,
    #[serde(rename = "LZR-5002")]
    Risc0WitnessFailed,
    #[serde(rename = "LZR-5003")]
    RapidsnarkProofFailed,
    #[serde(rename = "LZR-5004")]
    NullifierLockedInFlight,

    // ── LZR-6xxx: Registry & Storage ─────────────────────────────────────
    #[serde(rename = "LZR-6001")]
    RegistryEntryNotFound,
    #[serde(rename = "LZR-6002")]
    IdlFetchOrParseFailed,
    #[serde(rename = "LZR-6003")]
    BytecodeVerificationMismatch,
    #[serde(rename = "LZR-6004")]
    StorageNodeUnreachable,
    #[serde(rename = "LZR-6005")]
    ProgramNotDeployed,
}

impl ErrorCode {
    /// Canonical `"LZR-xxxx"` string identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VaultUninitialized => "LZR-1001",
            Self::VaultLocked => "LZR-1002",
            Self::InvalidPassword => "LZR-1003",
            Self::InvalidMnemonic => "LZR-1004",
            Self::MigrationFailed => "LZR-1005",
            Self::VaultCorrupted => "LZR-1006",
            Self::DuplicateAccountLabel => "LZR-1007",
            Self::AccountNotFound => "LZR-1008",
            Self::VaultAlreadyExists => "LZR-1009",
            Self::InvalidAddress => "LZR-1010",
            Self::SessionExpiredOrInvalid => "LZR-2001",
            Self::AccountNotAuthorized => "LZR-2002",
            Self::PrivateBalanceForbidden => "LZR-2003",
            Self::UiControlTokenInvalid => "LZR-2004",
            Self::UserRejected => "LZR-3001",
            Self::ProposalExpired => "LZR-3002",
            Self::SimulationReverted => "LZR-3003",
            Self::UnacknowledgedRiskGate => "LZR-3004",
            Self::ProposalDigestMismatch => "LZR-3005",
            Self::TransactionRejected => "LZR-3006",
            Self::SequencerUnreachable => "LZR-4001",
            Self::DisallowedEndpointBlocked => "LZR-4002",
            Self::ShardContentionExhausted => "LZR-4003",
            Self::FaucetCooldownOrDepleted => "LZR-4004",
            Self::ProverQueueBusy => "LZR-5001",
            Self::Risc0WitnessFailed => "LZR-5002",
            Self::RapidsnarkProofFailed => "LZR-5003",
            Self::NullifierLockedInFlight => "LZR-5004",
            Self::RegistryEntryNotFound => "LZR-6001",
            Self::IdlFetchOrParseFailed => "LZR-6002",
            Self::BytecodeVerificationMismatch => "LZR-6003",
            Self::StorageNodeUnreachable => "LZR-6004",
            Self::ProgramNotDeployed => "LZR-6005",
        }
    }

    /// Canonical symbolic name (e.g. `"InvalidPassword"`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::VaultUninitialized => "VaultUninitialized",
            Self::VaultLocked => "VaultLocked",
            Self::InvalidPassword => "InvalidPassword",
            Self::InvalidMnemonic => "InvalidMnemonic",
            Self::MigrationFailed => "MigrationFailed",
            Self::VaultCorrupted => "VaultCorrupted",
            Self::DuplicateAccountLabel => "DuplicateAccountLabel",
            Self::AccountNotFound => "AccountNotFound",
            Self::VaultAlreadyExists => "VaultAlreadyExists",
            Self::InvalidAddress => "InvalidAddress",
            Self::SessionExpiredOrInvalid => "SessionExpiredOrInvalid",
            Self::AccountNotAuthorized => "AccountNotAuthorized",
            Self::PrivateBalanceForbidden => "PrivateBalanceForbidden",
            Self::UiControlTokenInvalid => "UiControlTokenInvalid",
            Self::UserRejected => "UserRejected",
            Self::ProposalExpired => "ProposalExpired",
            Self::SimulationReverted => "SimulationReverted",
            Self::UnacknowledgedRiskGate => "UnacknowledgedRiskGate",
            Self::ProposalDigestMismatch => "ProposalDigestMismatch",
            Self::TransactionRejected => "TransactionRejected",
            Self::SequencerUnreachable => "SequencerUnreachable",
            Self::DisallowedEndpointBlocked => "DisallowedEndpointBlocked",
            Self::ShardContentionExhausted => "ShardContentionExhausted",
            Self::FaucetCooldownOrDepleted => "FaucetCooldownOrDepleted",
            Self::ProverQueueBusy => "ProverQueueBusy",
            Self::Risc0WitnessFailed => "Risc0WitnessFailed",
            Self::RapidsnarkProofFailed => "RapidsnarkProofFailed",
            Self::NullifierLockedInFlight => "NullifierLockedInFlight",
            Self::RegistryEntryNotFound => "RegistryEntryNotFound",
            Self::IdlFetchOrParseFailed => "IdlFetchOrParseFailed",
            Self::BytecodeVerificationMismatch => "BytecodeVerificationMismatch",
            Self::StorageNodeUnreachable => "StorageNodeUnreachable",
            Self::ProgramNotDeployed => "ProgramNotDeployed",
        }
    }

    /// Category for this error code.
    pub const fn category(self) -> ErrorCategory {
        match self {
            Self::VaultUninitialized
            | Self::VaultLocked
            | Self::InvalidPassword
            | Self::InvalidMnemonic
            | Self::MigrationFailed
            | Self::VaultCorrupted
            | Self::DuplicateAccountLabel
            | Self::AccountNotFound
            | Self::VaultAlreadyExists
            | Self::InvalidAddress => ErrorCategory::VaultAndAuth,
            Self::SessionExpiredOrInvalid
            | Self::AccountNotAuthorized
            | Self::PrivateBalanceForbidden
            | Self::UiControlTokenInvalid => ErrorCategory::SessionAndAcl,
            Self::UserRejected
            | Self::ProposalExpired
            | Self::SimulationReverted
            | Self::UnacknowledgedRiskGate
            | Self::ProposalDigestMismatch
            | Self::TransactionRejected => ErrorCategory::ApprovalAndSimulation,
            Self::SequencerUnreachable
            | Self::DisallowedEndpointBlocked
            | Self::ShardContentionExhausted
            | Self::FaucetCooldownOrDepleted => ErrorCategory::NetworkAndSequencer,
            Self::ProverQueueBusy
            | Self::Risc0WitnessFailed
            | Self::RapidsnarkProofFailed
            | Self::NullifierLockedInFlight => ErrorCategory::ProverAndCrypto,
            Self::RegistryEntryNotFound
            | Self::IdlFetchOrParseFailed
            | Self::BytecodeVerificationMismatch
            | Self::StorageNodeUnreachable
            | Self::ProgramNotDeployed => ErrorCategory::RegistryAndStorage,
        }
    }

    /// Whether the caller can safely retry the operation without changing inputs.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::SequencerUnreachable
                | Self::ShardContentionExhausted
                | Self::ProverQueueBusy
                | Self::StorageNodeUnreachable
        )
    }

    /// Lookup an `ErrorCode` from its `"LZR-xxxx"` code string.
    pub fn from_code_str(code: &str) -> Option<Self> {
        match code {
            "LZR-1001" => Some(Self::VaultUninitialized),
            "LZR-1002" => Some(Self::VaultLocked),
            "LZR-1003" => Some(Self::InvalidPassword),
            "LZR-1004" => Some(Self::InvalidMnemonic),
            "LZR-1005" => Some(Self::MigrationFailed),
            "LZR-1006" => Some(Self::VaultCorrupted),
            "LZR-1007" => Some(Self::DuplicateAccountLabel),
            "LZR-1008" => Some(Self::AccountNotFound),
            "LZR-1009" => Some(Self::VaultAlreadyExists),
            "LZR-1010" => Some(Self::InvalidAddress),
            "LZR-2001" => Some(Self::SessionExpiredOrInvalid),
            "LZR-2002" => Some(Self::AccountNotAuthorized),
            "LZR-2003" => Some(Self::PrivateBalanceForbidden),
            "LZR-2004" => Some(Self::UiControlTokenInvalid),
            "LZR-3001" => Some(Self::UserRejected),
            "LZR-3002" => Some(Self::ProposalExpired),
            "LZR-3003" => Some(Self::SimulationReverted),
            "LZR-3004" => Some(Self::UnacknowledgedRiskGate),
            "LZR-3005" => Some(Self::ProposalDigestMismatch),
            "LZR-3006" => Some(Self::TransactionRejected),
            "LZR-4001" => Some(Self::SequencerUnreachable),
            "LZR-4002" => Some(Self::DisallowedEndpointBlocked),
            "LZR-4003" => Some(Self::ShardContentionExhausted),
            "LZR-4004" => Some(Self::FaucetCooldownOrDepleted),
            "LZR-5001" => Some(Self::ProverQueueBusy),
            "LZR-5002" => Some(Self::Risc0WitnessFailed),
            "LZR-5003" => Some(Self::RapidsnarkProofFailed),
            "LZR-5004" => Some(Self::NullifierLockedInFlight),
            "LZR-6001" => Some(Self::RegistryEntryNotFound),
            "LZR-6002" => Some(Self::IdlFetchOrParseFailed),
            "LZR-6003" => Some(Self::BytecodeVerificationMismatch),
            "LZR-6004" => Some(Self::StorageNodeUnreachable),
            "LZR-6005" => Some(Self::ProgramNotDeployed),
            _ => None,
        }
    }
}

/// Structured error envelope serialized across CLI, LIDL, QML, and TypeScript SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub code: String,
    pub name: String,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
}

/// Unified `LezerError` returned by all `lezer-core` operations.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{}: {} ({message})", .code.as_str(), .code.name())]
pub struct LezerError {
    pub code: ErrorCode,
    pub message: String,
}

impl LezerError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn to_envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            code: self.code.as_str().to_string(),
            name: self.code.name().to_string(),
            category: self.code.category(),
            message: self.message.clone(),
            retryable: self.code.retryable(),
        }
    }
}

pub type LezerResult<T> = std::result::Result<T, LezerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_roundtrips_and_preserves_metadata() {
        let err = LezerError::new(
            ErrorCode::InvalidPassword,
            "AEAD decryption failed: wrong vault password",
        );
        let env = err.to_envelope();
        assert_eq!(env.code, "LZR-1003");
        assert_eq!(env.name, "InvalidPassword");
        assert_eq!(env.category, ErrorCategory::VaultAndAuth);
        assert!(!env.retryable);
        assert!(err.to_string().contains("LZR-1003"));
    }
}
