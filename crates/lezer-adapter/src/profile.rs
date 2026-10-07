//! `NetworkProfile` — dynamic zone and endpoint configuration for LEZ v0.3 / v0.4,
//! local standalone sequencer CI, and LP-0022 multi-zone forward compatibility.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Canonical public LEZ testnet sequencer endpoint.
pub const DEFAULT_TESTNET_SEQUENCER_URL: &str = "https://testnet.lez.logos.co";

/// Canonical local standalone sequencer endpoint for hermetic CI.
pub const DEFAULT_LOCALNET_SEQUENCER_URL: &str = "http://127.0.0.1:3040";

/// Genesis Piñata faucet account ID on LEZ testnet (Base58).
pub const PINATA_FAUCET_ACCOUNT_BASE58: &str = "EfQhKQAkX2FJiwNii2WFQsGndjvF1Mzd7RuVe7QdPLw7";

/// Fee and RISC0 cycle telemetry model for a zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeModelConfig {
    /// Whether the zone charges gas/fees per RISC0 cycle.
    pub fee_enabled: bool,
    /// Base fee in native units (0 on testnet v0.3/v0.4).
    pub base_fee_lez: u128,
    /// Default RISC0 cycle budget warning threshold.
    pub max_guest_cycles: u64,
}

impl Default for FeeModelConfig {
    fn default() -> Self {
        Self {
            fee_enabled: false,
            base_fee_lez: 0,
            max_guest_cycles: 16_777_216,
        }
    }
}

/// Canonical deployed Program IDs (`[u32; 8]` RISC0 Image IDs) on a zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneProgramsConfig {
    pub authenticated_transfer: [u32; 8],
    pub token: [u32; 8],
    pub associated_token_account: [u32; 8],
    pub pinata_faucet: [u32; 8],
    pub clock: [u32; 8],
    pub privacy_preserving_circuit: [u32; 8],
    pub lezer_registry: [u32; 8],
    pub lezer_testimonial: [u32; 8],
}

impl Default for ZoneProgramsConfig {
    fn default() -> Self {
        Self {
            authenticated_transfer: [0; 8],
            token: [0; 8],
            associated_token_account: [0; 8],
            pinata_faucet: [0; 8],
            clock: [0; 8],
            // Live testnet privacy_preserving_circuit image ID words
            privacy_preserving_circuit: [
                3943929112, 3682468618, 2822605667, 1764660516,
                290278025, 2512717227, 411457898, 556249114,
            ],
            lezer_registry: [0; 8],
            lezer_testimonial: [0; 8],
        }
    }
}

/// Encapsulates all chain/zone parameters for `lezer-adapter` and `lezer-core`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProfile {
    /// Canonical identifier: `"lez-testnet-0.3"`, `"lez-testnet-0.4"`, or `"lez-localnet"`.
    pub id: String,
    /// Zone identifier for LP-0022 multi-zone routing (default: `"lez"`).
    pub zone_id: String,
    /// Sequencer JSON-RPC endpoint (overridable via `LEZ_SEQUENCER_URL`).
    pub sequencer_url: String,
    /// Block explorer base URL (display-only, never required for wallet operation).
    pub explorer_url: String,
    /// Optional Logos Storage (Codex) REST endpoint for IDL resolution & encrypted backup.
    pub storage_node_url: Option<String>,
    /// Whether default-owned accounts (`program_owner == [0; 8]`) can receive native credits
    /// directly without pre-registration (`true` on v0.3/v0.4; `false` on v0.2.x).
    pub supports_default_owned_credits: bool,
    /// Whether private/shielded transfers and ML-KEM-768 note scanning are supported.
    pub supports_private_accounts: bool,
    /// Whether the Piñata PoW faucet is active on this network.
    pub supports_faucet: bool,
    /// Fee & cycle telemetry capability flags.
    pub fee_model: FeeModelConfig,
    /// Canonical deployed Program IDs on this zone.
    pub programs: ZoneProgramsConfig,
}

impl NetworkProfile {
    /// Default profile for live Logos Execution Zone Testnet (`https://testnet.lez.logos.co`),
    /// respecting the `LEZ_SEQUENCER_URL` environment variable override when set.
    pub fn testnet_default() -> Self {
        let sequencer_url = std::env::var("LEZ_SEQUENCER_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_TESTNET_SEQUENCER_URL.to_string());
        let storage_node_url = std::env::var("LEZER_STORAGE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty());

        Self {
            id: "lez-testnet-0.4".to_string(),
            zone_id: "lez".to_string(),
            sequencer_url,
            explorer_url: "https://explorer.lez.logos.co".to_string(),
            storage_node_url,
            supports_default_owned_credits: true,
            supports_private_accounts: true,
            supports_faucet: true,
            fee_model: FeeModelConfig::default(),
            programs: ZoneProgramsConfig::default(),
        }
    }

    /// Profile for local standalone sequencer testing (`http://127.0.0.1:3040`).
    pub fn localnet() -> Self {
        let sequencer_url = std::env::var("LEZ_SEQUENCER_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_LOCALNET_SEQUENCER_URL.to_string());

        Self {
            id: "lez-localnet".to_string(),
            zone_id: "lez".to_string(),
            sequencer_url,
            explorer_url: "http://127.0.0.1:3040".to_string(),
            storage_node_url: None,
            supports_default_owned_credits: true,
            supports_private_accounts: true,
            supports_faucet: true,
            fee_model: FeeModelConfig::default(),
            programs: ZoneProgramsConfig::default(),
        }
    }

    /// Resolve a profile by selector (`"lez"`, `"lez-testnet-0.3"`, `"lez-testnet-0.4"`,
    /// `"lez-localnet"`, or an explicit `http(s)://` sequencer URL).
    pub fn resolve(selector: Option<&str>) -> Result<Self> {
        let sel = selector.unwrap_or("lez-testnet-0.4").trim();
        match sel {
            "" | "lez" | "lez-testnet" | "lez-testnet-0.3" | "lez-testnet-0.4" => {
                let mut profile = Self::testnet_default();
                if sel == "lez-testnet-0.3" {
                    profile.id = "lez-testnet-0.3".to_string();
                }
                Ok(profile)
            }
            "local" | "localnet" | "lez-localnet" => Ok(Self::localnet()),
            url if url.starts_with("http://") || url.starts_with("https://") => {
                let mut profile = Self::testnet_default();
                profile.id = "lez-custom".to_string();
                profile.sequencer_url = url.to_string();
                Ok(profile)
            }
            other => bail!(
                "LZR-4002: unknown network profile '{other}' (expected 'lez-testnet-0.3', 'lez-testnet-0.4', 'lez-localnet', or a valid http(s):// sequencer URL)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_builtin_profiles_and_urls() {
        let p = NetworkProfile::resolve(Some("lez-testnet-0.4")).unwrap();
        assert_eq!(p.zone_id, "lez");
        assert!(p.supports_default_owned_credits);
        assert!(p.supports_private_accounts);

        let local = NetworkProfile::resolve(Some("lez-localnet")).unwrap();
        assert_eq!(local.id, "lez-localnet");

        let custom = NetworkProfile::resolve(Some("http://127.0.0.1:9999")).unwrap();
        assert_eq!(custom.sequencer_url, "http://127.0.0.1:9999");

        assert!(NetworkProfile::resolve(Some("solana-mainnet")).is_err());
    }
}
