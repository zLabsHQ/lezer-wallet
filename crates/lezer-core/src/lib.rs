//! `lezer-core` — Canonical Native Rust Wallet Engine.
//!
//! **Architectural Boundary Rule #2:** `lezer-core` NEVER imports upstream `wallet` or
//! `lee_core` crates directly. All protocol interactions route through `lezer_adapter::ChainAdapter`.

use std::sync::Arc;

use anyhow::Result;
use lezer_adapter::{ChainAdapter, LezAdapter, NetworkProfile};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Compile-time secret redaction wrapper (`Redacted<T>`).
///
/// Any secret wrapped in `Redacted<T>` formats as `"[REDACTED]"` in both `Debug` and
/// `Display` implementations and zeroizes its memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Redacted<T: Zeroize>(T);

impl<T: Zeroize> Redacted<T> {
    pub fn new(secret: T) -> Self {
        Self(secret)
    }

    pub fn expose_secret(&self) -> &T {
        &self.0
    }
}

impl<T: Zeroize> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"[REDACTED]\"")
    }
}

impl<T: Zeroize> std::fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Lock state of the wallet vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Uninitialized,
    Locked,
    Unlocked,
    DeferredLockPending,
}

/// Canonical `WalletStateSnapshot` emitted to Basecamp QML UI and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletStateSnapshot {
    pub revision: u64,
    pub lock_state: LockState,
    pub profile_id: String,
    pub sequencer_url: String,
    pub active_account_id: Option<String>,
}

/// Canonical Lezer Wallet Engine wrapping a [`ChainAdapter`].
pub struct WalletEngine {
    adapter: Arc<dyn ChainAdapter>,
    revision: u64,
    lock_state: LockState,
}

impl WalletEngine {
    /// Construct a new [`WalletEngine`] backed by `adapter`.
    pub fn new(adapter: Arc<dyn ChainAdapter>) -> Self {
        Self {
            adapter,
            revision: 1,
            lock_state: LockState::Uninitialized,
        }
    }

    /// Construct a [`WalletEngine`] with the default [`LezAdapter`].
    pub fn with_default_adapter() -> Result<Self> {
        let adapter = Arc::new(LezAdapter::from_env()?);
        Ok(Self::new(adapter))
    }

    pub fn profile(&self) -> &NetworkProfile {
        self.adapter.profile()
    }

    /// Produce an immutable [`WalletStateSnapshot`] for stateless UI rendering.
    pub fn snapshot(&self) -> WalletStateSnapshot {
        WalletStateSnapshot {
            revision: self.revision,
            lock_state: self.lock_state,
            profile_id: self.adapter.profile().id.clone(),
            sequencer_url: self.adapter.profile().sequencer_url.clone(),
            active_account_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_never_leaks_secret_in_logs() {
        let mnemonic = Redacted::new("abandon abandon abandon secret seed phrase".to_string());
        let debug_str = format!("{mnemonic:?}");
        let display_str = format!("{mnemonic}");
        assert_eq!(debug_str, "\"[REDACTED]\"");
        assert_eq!(display_str, "[REDACTED]");
        assert!(!debug_str.contains("abandon"));
    }

    #[test]
    fn snapshot_reflects_adapter_profile() {
        let engine = WalletEngine::with_default_adapter().unwrap();
        let snap = engine.snapshot();
        assert_eq!(snap.lock_state, LockState::Uninitialized);
        assert_eq!(snap.revision, 1);
    }
}
