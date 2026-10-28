//! `NetworkProfile` — which LEZ sequencer the wallet talks to, plus the program IDs of
//! Lezer's own on-chain programs on that network (only when actually deployed).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Public LEZ testnet sequencer (runs official `logos-execution-zone` `v0.3.1`).
pub const DEFAULT_TESTNET_SEQUENCER_URL: &str = "https://testnet.lez.logos.co";

/// Local standalone sequencer (`sequencer_service --features standalone`).
pub const DEFAULT_LOCALNET_SEQUENCER_URL: &str = "http://127.0.0.1:3040";

/// Program account ids (`Public/<base58>`, as returned by deployment) of Lezer's own
/// programs on a network. `None` means "not deployed here" — callers refuse, never fake.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LezerDeployments {
    pub testimonial: Option<String>,
    pub registry: Option<String>,
    pub token: Option<String>,
    pub faucet: Option<String>,
}

impl LezerDeployments {
    fn from_env() -> Self {
        Self {
            testimonial: env_nonempty("LEZER_TESTIMONIAL_PROGRAM"),
            registry: env_nonempty("LEZER_REGISTRY_PROGRAM"),
            token: env_nonempty("LEZER_TOKEN_PROGRAM"),
            faucet: env_nonempty("LEZER_FAUCET_PROGRAM"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProfile {
    /// `"lez-testnet"`, `"lez-localnet"`, or `"lez-custom"`.
    pub id: String,
    pub sequencer_url: String,
    /// Display-only; never required for wallet operation.
    pub explorer_url: Option<String>,
    /// Optional Logos Storage (Codex) node for encrypted backups and IDLs.
    pub storage_node_url: Option<String>,
    pub deployments: LezerDeployments,
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

impl NetworkProfile {
    pub fn testnet() -> Self {
        Self {
            id: "lez-testnet".to_string(),
            sequencer_url: DEFAULT_TESTNET_SEQUENCER_URL.to_string(),
            explorer_url: Some("https://explorer.testnet.lez.logos.co".to_string()),
            storage_node_url: env_nonempty("LEZER_STORAGE_URL"),
            deployments: LezerDeployments::from_env(),
        }
    }

    pub fn localnet() -> Self {
        Self {
            id: "lez-localnet".to_string(),
            sequencer_url: DEFAULT_LOCALNET_SEQUENCER_URL.to_string(),
            explorer_url: None,
            storage_node_url: env_nonempty("LEZER_STORAGE_URL"),
            deployments: LezerDeployments::from_env(),
        }
    }

    /// Default profile: `LEZ_SEQUENCER_URL` if set, otherwise the public testnet.
    pub fn from_env() -> Self {
        match env_nonempty("LEZ_SEQUENCER_URL") {
            Some(url) => Self::custom(&url),
            None => Self::testnet(),
        }
    }

    fn custom(url: &str) -> Self {
        Self {
            id: "lez-custom".to_string(),
            sequencer_url: url.trim().to_string(),
            explorer_url: None,
            storage_node_url: env_nonempty("LEZER_STORAGE_URL"),
            deployments: LezerDeployments::from_env(),
        }
    }

    /// Resolve `"testnet"`, `"localnet"`, or an explicit `http(s)://` sequencer URL.
    pub fn resolve(selector: Option<&str>) -> Result<Self> {
        let Some(sel) = selector.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::from_env());
        };
        match sel {
            "testnet" | "lez" | "lez-testnet" => Ok(Self::testnet()),
            "local" | "localnet" | "lez-localnet" => Ok(Self::localnet()),
            url if url.starts_with("http://") || url.starts_with("https://") => Ok(Self::custom(url)),
            other => bail!(
                "unknown network '{other}' (expected 'testnet', 'localnet', or an http(s):// sequencer URL)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_networks() {
        assert_eq!(NetworkProfile::resolve(Some("testnet")).unwrap().sequencer_url, DEFAULT_TESTNET_SEQUENCER_URL);
        assert_eq!(NetworkProfile::resolve(Some("localnet")).unwrap().id, "lez-localnet");
        let custom = NetworkProfile::resolve(Some("http://127.0.0.1:9999")).unwrap();
        assert_eq!(custom.sequencer_url, "http://127.0.0.1:9999");
        assert!(NetworkProfile::resolve(Some("solana-mainnet")).is_err());
        assert!(custom.deployments.testimonial.is_none());
    }
}
