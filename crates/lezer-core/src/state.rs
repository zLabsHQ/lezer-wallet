//! Canonical `WalletStateSnapshot` — single source of truth emitted by `lezer-core`
//! to Basecamp QML UI views (`lezer_wallet_ui`) and the `lezer` CLI.

use serde::{Deserialize, Serialize};

use crate::autolock::LockState;
use crate::vault::AccountPrivacy;

/// Network connectivity status view for UI and CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NetworkStatusView {
    Online {
        block_height: u64,
        sequencer_url: String,
    },
    Offline {
        last_seen_block: u64,
        sequencer_url: String,
    },
}

/// Account summary rendered in the portfolio switcher and CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummaryView {
    pub account_id: String,
    pub label: String,
    pub privacy: AccountPrivacy,
    pub native_balance: String,
    pub is_cached_offline: bool,
    pub receive_uri: Option<String>,
}

/// Active dApp session summary view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummaryView {
    pub session_token_preview: String,
    pub dapp_name: String,
    pub dapp_origin: String,
    pub authorized_accounts: Vec<String>,
    pub expires_at: u64,
}

/// Pending connection or transaction approval request view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRequestView {
    pub request_id: String,
    pub kind: String,
    pub dapp_name: String,
    pub summary_json: serde_json::Value,
}

/// Background `Semaphore(1)` ZK prover queue status view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProverQueueStatusView {
    Idle,
    Proving {
        proposal_id: String,
        stage: String,
        elapsed_ms: u64,
    },
}

/// Historical / pending transaction journal activity item view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityItemView {
    pub tx_hash: String,
    pub from_account: String,
    pub to_account: String,
    pub amount: String,
    pub kind: String,
    pub status: String,
    pub created_at: u64,
}

/// Canonical `WalletStateSnapshot` produced exclusively by Rust (`lezer-core`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletStateSnapshot {
    pub revision: u64,
    pub lock_state: LockState,
    pub profile_id: String,
    pub sequencer_url: String,
    pub network: NetworkStatusView,
    pub active_account_id: Option<String>,
    pub accounts: Vec<AccountSummaryView>,
    pub active_sessions: Vec<SessionSummaryView>,
    pub pending_requests: Vec<PendingRequestView>,
    pub prover_queue: ProverQueueStatusView,
    pub recent_activity: Vec<ActivityItemView>,
}
