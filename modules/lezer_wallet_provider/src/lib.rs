//! `lezer_wallet_provider` — Logos Basecamp Core Module (`type: "core"`, `interface: "cdylib"`).
//!
//! Wraps `lezer_core::WalletEngine` behind the C-ABI LIDL contract (`lezer_wallet_provider.lidl`).
//! Never imports internal vault structs; delegates all state and signing to `lezer-core`.

use lezer_core::WalletEngine;

pub struct LezerWalletProvider {
    engine: WalletEngine,
}

impl LezerWalletProvider {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            engine: WalletEngine::with_default_adapter()?,
        })
    }

    pub fn get_status(&self) -> Vec<u8> {
        let snapshot = self.engine.snapshot();
        serde_json::to_vec(&snapshot).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_returns_valid_status_json() {
        let provider = LezerWalletProvider::new().expect("provider init");
        let status_bytes = provider.get_status();
        let parsed: serde_json::Value = serde_json::from_slice(&status_bytes).unwrap();
        assert_eq!(parsed["lock_state"], "uninitialized");
        assert_eq!(parsed["revision"], 1);
    }
}
