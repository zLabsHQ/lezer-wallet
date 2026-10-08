//! Ephemeral, 100% rebuildable cache (`~/.lezer/cache.json` -> `CacheEnvelopeV1`).
//!
//! Strictly separated from `~/.lezer/vault.enc` so cache corruption or schema upgrades
//! can never endanger user key material. Enables full **Offline Mode** balance and
//! metadata inspection when the sequencer is unreachable.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Current schema version for `cache.json`.
pub const CURRENT_CACHE_SCHEMA_VERSION: u32 = 1;

/// Cached balance entry for an account (supports Offline Mode display).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedBalanceEntry {
    pub account_id: String,
    pub native_balance: String,
    pub nonce: u128,
    pub token_balances: BTreeMap<String, String>,
    pub updated_at_block: u64,
    pub updated_at_unix: u64,
}

/// Ephemeral rebuildable cache envelope (`~/.lezer/cache.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEnvelopeV1 {
    pub schema_version: u32,
    pub last_seen_block: u64,
    pub last_sync_unix: u64,
    /// Cached public & token/ATA balances keyed by `account_id`.
    pub balances: BTreeMap<String, CachedBalanceEntry>,
    /// Cached LP-0023 `RegistryEntry` JSON payloads keyed by `program_id`.
    pub registry_entries: BTreeMap<String, serde_json::Value>,
    /// Cached SPEL JSON IDLs keyed by `idl_cid`.
    pub idls_by_cid: BTreeMap<String, serde_json::Value>,
    /// Cached `TestimonialShardPage` snapshots keyed by `"<submission_id>:<shard_hex>:<page_hex>"`.
    pub testimonial_shards: BTreeMap<String, serde_json::Value>,
}

impl Default for CacheEnvelopeV1 {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_CACHE_SCHEMA_VERSION,
            last_seen_block: 0,
            last_sync_unix: 0,
            balances: BTreeMap::new(),
            registry_entries: BTreeMap::new(),
            idls_by_cid: BTreeMap::new(),
            testimonial_shards: BTreeMap::new(),
        }
    }
}

impl CacheEnvelopeV1 {
    /// Load `cache.json` from disk.
    ///
    /// Per architectural rule (`docs/08` §3.1): if `cache.json` is missing, corrupted,
    /// or has a mismatched `schema_version`, silently discard it and return a fresh
    /// empty `CacheEnvelopeV1` with zero risk to `vault.enc`.
    pub fn load_or_rebuild(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(cache) if cache.schema_version == CURRENT_CACHE_SCHEMA_VERSION => cache,
            _ => {
                let _ = fs::remove_file(path);
                Self::default()
            }
        }
    }

    /// Persist `cache.json` atomically.
    pub fn save(&self, path: &Path) {
        let Some(parent) = path.parent() else {
            return;
        };
        let _ = fs::create_dir_all(parent);
        let tmp_path = parent.join("cache.json.tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(self)
            && fs::write(&tmp_path, bytes).is_ok()
        {
            let _ = fs::rename(&tmp_path, path);
        }
    }

    /// Record an updated native balance for `account_id`.
    pub fn update_native_balance(
        &mut self,
        account_id: &str,
        balance: u128,
        nonce: u128,
        block_height: u64,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if block_height > self.last_seen_block {
            self.last_seen_block = block_height;
        }
        self.last_sync_unix = now;

        let entry = self
            .balances
            .entry(account_id.to_string())
            .or_insert_with(|| CachedBalanceEntry {
                account_id: account_id.to_string(),
                native_balance: "0".to_string(),
                nonce: 0,
                token_balances: BTreeMap::new(),
                updated_at_block: block_height,
                updated_at_unix: now,
            });
        entry.native_balance = balance.to_string();
        entry.nonce = nonce;
        entry.updated_at_block = block_height;
        entry.updated_at_unix = now;
    }

    /// Lookup the cached native balance for `account_id` (used in Offline Mode).
    pub fn get_cached_native_balance(&self, account_id: &str) -> Option<(u128, u64)> {
        let entry = self.balances.get(account_id)?;
        let bal = entry.native_balance.parse::<u128>().ok()?;
        Some((bal, entry.updated_at_block))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_or_outdated_cache_rebuilds_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.json");

        fs::write(&cache_path, b"not-valid-json{{{").unwrap();
        let rebuilt = CacheEnvelopeV1::load_or_rebuild(&cache_path);
        assert_eq!(rebuilt.schema_version, CURRENT_CACHE_SCHEMA_VERSION);
        assert!(!cache_path.exists());

        let mut cache = CacheEnvelopeV1::default();
        cache.update_native_balance("Public/Alice123", 150, 1, 36316);
        cache.save(&cache_path);

        let loaded = CacheEnvelopeV1::load_or_rebuild(&cache_path);
        assert_eq!(
            loaded.get_cached_native_balance("Public/Alice123"),
            Some((150, 36316))
        );
    }
}
