//! `WalletStateSnapshot` — the single view of wallet state rendered by the CLI and QML UI.

use lezer_adapter::AccountKind;
use serde::{Deserialize, Serialize};

use crate::autolock::LockState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummaryView {
    pub account_id: String,
    pub kind: AccountKind,
    pub labels: Vec<String>,
    /// Last known native balance (decimal string), `None` if never read.
    pub native_balance: Option<String>,
    /// Block at which `native_balance` was observed (public) or synced (private).
    pub balance_block: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummaryView {
    /// Non-secret identifier (SHA-256 of the token, truncated) used to revoke from the UI.
    pub session_id: String,
    pub dapp_name: String,
    pub dapp_origin: String,
    pub authorized_accounts: Vec<String>,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRequestView {
    pub request_id: String,
    pub kind: String,
    pub dapp_name: String,
    pub summary_json: serde_json::Value,
}

/// An operation that is currently building/proving/submitting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlightView {
    pub description: String,
    pub requires_zk_proof: bool,
    pub started_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityItemView {
    pub tx_hash: String,
    pub kind: String,
    pub from_account: String,
    pub to_account: String,
    pub amount: String,
    pub status: String,
    pub block_id: Option<u64>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletStateSnapshot {
    pub revision: u64,
    pub lock_state: LockState,
    pub network_id: String,
    pub sequencer_url: String,
    pub last_seen_block: Option<u64>,
    pub active_account_id: Option<String>,
    pub accounts: Vec<AccountSummaryView>,
    pub active_sessions: Vec<SessionSummaryView>,
    pub pending_requests: Vec<PendingRequestView>,
    pub in_flight: Option<InFlightView>,
    pub recent_activity: Vec<ActivityItemView>,
}
