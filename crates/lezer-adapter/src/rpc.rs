//! Sequencer JSON-RPC client and `AllowedEndpointsGuard` zero-telemetry network policy.
//!
//! Enforces LP-0021 Reliability Criterion #3: the wallet only connects to the user's
//! configured `sequencer_url`, local LEZ node, or configured Logos Storage (Codex) node.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::profile::{NetworkProfile, PINATA_FAUCET_ACCOUNT_BASE58};

/// Strict network allowlist guard (`AllowedEndpointsGuard`).
///
/// Rejects any outbound HTTP request to an origin that is not explicitly listed
/// in the active [`NetworkProfile`], returning `LZR-4002: DisallowedEndpointBlocked`.
#[derive(Debug, Clone)]
pub struct AllowedEndpointsGuard {
    allowed_origins: Vec<String>,
}

impl AllowedEndpointsGuard {
    /// Construct an allowlist guard from a [`NetworkProfile`].
    pub fn from_profile(profile: &NetworkProfile) -> Self {
        let mut allowed_origins = Vec::new();
        if let Some(origin) = extract_origin(&profile.sequencer_url) {
            allowed_origins.push(origin);
        }
        if let Some(ref storage_url) = profile.storage_node_url
            && let Some(origin) = extract_origin(storage_url)
        {
            allowed_origins.push(origin);
        }
        // Always permit localhost loopback for standalone sequencer CI.
        allowed_origins.push("http://127.0.0.1".to_string());
        allowed_origins.push("http://localhost".to_string());
        Self { allowed_origins }
    }

    /// Verify that `target_url` is permitted by the allowlist.
    pub fn check_url(&self, target_url: &str) -> Result<()> {
        let Some(origin) = extract_origin(target_url) else {
            bail!("LZR-4002: DisallowedEndpointBlocked — malformed URL '{target_url}'");
        };
        let allowed = self.allowed_origins.iter().any(|allowed_origin| {
            origin == *allowed_origin
                || (allowed_origin == "http://127.0.0.1" && origin.starts_with("http://127.0.0.1:"))
                || (allowed_origin == "http://localhost" && origin.starts_with("http://localhost:"))
        });
        if !allowed {
            bail!(
                "LZR-4002: DisallowedEndpointBlocked — outbound connection to '{origin}' is forbidden by zero-third-party-service policy"
            );
        }
        Ok(())
    }
}

fn extract_origin(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host_port = rest.split('/').next()?;
    if host_port.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host_port}"))
}

/// Account state snapshot returned by the LEZ sequencer JSON-RPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcAccountState {
    pub account_id: String,
    pub balance: u128,
    pub nonce: u128,
    pub program_owner: [u32; 8],
    pub data_raw: Value,
    /// True if `program_owner == [0; 8]` (default-owned account on LEZ v0.3/v0.4).
    pub is_default_owned: bool,
}

/// Piñata faucet status and challenge read from the sequencer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinataFaucetStatus {
    pub faucet_account_id: String,
    pub balance: u128,
    pub nonce: u128,
    pub difficulty_bits: u8,
    pub challenge_summary: String,
    pub protocol_generation: String,
}

/// Report produced by Spike S-02 (Live Sequencer Compatibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencerCompatibilityReport {
    pub sequencer_url: String,
    pub profile_id: String,
    pub reachable: bool,
    pub latency_ms: u64,
    pub latest_block_id: u64,
    pub program_ids: BTreeMap<String, [u32; 8]>,
    pub default_owned_account_check: RpcAccountState,
    pub pinata_status: PinataFaucetStatus,
}

/// JSON-RPC client for the Logos Execution Zone (LEZ) sequencer.
#[derive(Debug, Clone)]
pub struct LezRpcClient {
    profile: NetworkProfile,
    guard: AllowedEndpointsGuard,
    http: reqwest::Client,
}

impl LezRpcClient {
    /// Create a new allowlist-guarded RPC client for `profile`.
    pub fn new(profile: NetworkProfile) -> Result<Self> {
        let guard = AllowedEndpointsGuard::from_profile(&profile);
        guard.check_url(&profile.sequencer_url)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .context("failed to initialize HTTP client")?;
        Ok(Self {
            profile,
            guard,
            http,
        })
    }

    pub fn profile(&self) -> &NetworkProfile {
        &self.profile
    }

    pub fn guard(&self) -> &AllowedEndpointsGuard {
        &self.guard
    }

    /// Perform a raw JSON-RPC 2.0 call against the configured sequencer.
    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        self.guard.check_url(&self.profile.sequencer_url)?;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let response = self
            .http
            .post(&self.profile.sequencer_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LZR-4001: SequencerUnreachable ({}): {e}", self.profile.sequencer_url))?;

        let status = response.status();
        let body: Value = response
            .json()
            .await
            .with_context(|| format!("LZR-4001: invalid JSON-RPC response (HTTP {status})"))?;

        if let Some(err) = body.get("error") {
            bail!("LZR-4001: sequencer RPC error on '{method}': {err}");
        }

        Ok(body.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Query `getProgramIds` on the sequencer (`[u32; 8]` RISC0 Image IDs).
    pub async fn get_program_ids(&self) -> Result<BTreeMap<String, [u32; 8]>> {
        let result = self.rpc_call("getProgramIds", json!({})).await?;
        let map: BTreeMap<String, [u32; 8]> = serde_json::from_value(result)
            .context("failed to deserialize getProgramIds map")?;
        Ok(map)
    }

    /// Query `getLastBlockId` on the sequencer.
    pub async fn get_last_block_id(&self) -> Result<u64> {
        let result = self.rpc_call("getLastBlockId", json!({})).await?;
        result
            .as_u64()
            .context("getLastBlockId did not return a u64 block height")
    }

    /// Query `getAccountBalance` for a public account ID.
    pub async fn get_account_balance(&self, account_id: &str) -> Result<u128> {
        let result = self
            .rpc_call("getAccountBalance", json!({ "account_id": account_id }))
            .await?;
        parse_u128_value(&result).context("invalid balance in getAccountBalance response")
    }

    /// Query `getAccount` and `getAccountBalance` to construct a normalized [`RpcAccountState`].
    /// Handles both LEZ v0.2.4 and LEZ v0.3/v0.4 response shapes seamlessly.
    pub async fn get_account_state(&self, account_id: &str) -> Result<RpcAccountState> {
        let raw = self
            .rpc_call("getAccount", json!({ "account_id": account_id }))
            .await?;

        let balance = if let Some(b) = raw.get("balance").and_then(parse_u128_value) {
            b
        } else {
            self.get_account_balance(account_id).await.unwrap_or(0)
        };

        let nonce = raw
            .get("nonce")
            .and_then(parse_u128_value)
            .unwrap_or(0);

        let program_owner = raw
            .get("program_owner")
            .and_then(|v| serde_json::from_value::<[u32; 8]>(v.clone()).ok())
            .unwrap_or([0u32; 8]);

        let data_raw = raw.get("data").cloned().unwrap_or(Value::Null);
        let is_default_owned = program_owner == [0u32; 8];

        Ok(RpcAccountState {
            account_id: account_id.to_string(),
            balance,
            nonce,
            program_owner,
            data_raw,
            is_default_owned,
        })
    }

    /// Inspect the Piñata faucet account (`EfQhKQAkX2FJiwNii2WFQsGndjvF1Mzd7RuVe7QdPLw7`)
    /// on the connected sequencer.
    pub async fn get_pinata_faucet_status(&self) -> Result<PinataFaucetStatus> {
        let state = self.get_account_state(PINATA_FAUCET_ACCOUNT_BASE58).await?;

        // In v0.2.4 `data` is a 33-byte array `[difficulty, seed..32]`.
        // In v0.3/v0.4 `data` is `{ "shards": { ... } }` or byte array.
        let (difficulty_bits, challenge_summary, protocol_generation) =
            if let Some(arr) = state.data_raw.as_array() {
                let diff = arr.first().and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                (
                    diff,
                    format!("33-byte PoW seed (difficulty={diff} leading zero bytes)"),
                    "lez-v0.2.4".to_string(),
                )
            } else if state.data_raw.get("shards").is_some() {
                (
                    2,
                    format!("sharded account state: {}", state.data_raw),
                    "lez-v0.4-sharded".to_string(),
                )
            } else {
                (
                    0,
                    format!("raw account data: {}", state.data_raw),
                    "lez-v0.3+".to_string(),
                )
            };

        Ok(PinataFaucetStatus {
            faucet_account_id: PINATA_FAUCET_ACCOUNT_BASE58.to_string(),
            balance: state.balance,
            nonce: state.nonce,
            difficulty_bits,
            challenge_summary,
            protocol_generation,
        })
    }

    /// Execute **Spike S-02 (Live Sequencer Compatibility)**:
    /// 1. Verify `getProgramIds()` on live testnet.
    /// 2. Verify default-owned account credit compatibility (`program_owner == [0; 8]`).
    /// 3. Verify Piñata challenge / account reads on live testnet.
    pub async fn run_spike_s02(&self) -> Result<SequencerCompatibilityReport> {
        let start = Instant::now();
        let program_ids = self.get_program_ids().await?;
        let latest_block_id = self.get_last_block_id().await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        // Probe a fresh uninitialized default-owned public account address.
        let probe_account = "11111111111111111111111111111111";
        let default_owned_account_check = self.get_account_state(probe_account).await?;
        if !default_owned_account_check.is_default_owned {
            bail!("expected fresh account to be default-owned ([0; 8])");
        }

        let pinata_status = self.get_pinata_faucet_status().await?;

        Ok(SequencerCompatibilityReport {
            sequencer_url: self.profile.sequencer_url.clone(),
            profile_id: self.profile.id.clone(),
            reachable: true,
            latency_ms,
            latest_block_id,
            program_ids,
            default_owned_account_check,
            pinata_status,
        })
    }
}

fn parse_u128_value(v: &Value) -> Option<u128> {
    if let Some(n) = v.as_u64() {
        Some(u128::from(n))
    } else if let Some(s) = v.as_str() {
        s.parse::<u128>().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_guard_blocks_unauthorized_endpoints() {
        let profile = NetworkProfile::testnet_default();
        let guard = AllowedEndpointsGuard::from_profile(&profile);

        assert!(guard.check_url("https://testnet.lez.logos.co").is_ok());
        assert!(guard.check_url("https://testnet.lez.logos.co/rpc").is_ok());
        assert!(guard.check_url("http://127.0.0.1:3040").is_ok());

        let err = guard
            .check_url("https://api.coingecko.com/api/v3/simple/price")
            .unwrap_err()
            .to_string();
        assert!(err.contains("LZR-4002"));
        assert!(err.contains("DisallowedEndpointBlocked"));
    }
}
