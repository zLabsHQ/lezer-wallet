//! Rebuildable cache (`~/.lezer/cache.json`): last observed public balances, for offline
//! display only. Any read failure discards it; it never holds secrets.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::vault::write_private_atomic;

pub const CURRENT_CACHE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedBalance {
    pub balance: String,
    pub block_id: u64,
    pub updated_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    pub schema_version: u32,
    pub last_seen_block: Option<u64>,
    pub balances: BTreeMap<String, CachedBalance>,
}

impl Default for Cache {
    fn default() -> Self {
        Self { schema_version: CURRENT_CACHE_SCHEMA_VERSION, last_seen_block: None, balances: BTreeMap::new() }
    }
}

impl Cache {
    pub fn load_or_default(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .filter(|c| c.schema_version == CURRENT_CACHE_SCHEMA_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = write_private_atomic(path, &bytes);
        }
    }

    pub fn record_balance(&mut self, account_id: &str, balance: u128, block_id: u64, now_unix: u64) {
        self.last_seen_block = Some(self.last_seen_block.map_or(block_id, |b| b.max(block_id)));
        self.balances.insert(
            account_id.to_string(),
            CachedBalance { balance: balance.to_string(), block_id, updated_unix: now_unix },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_or_old_cache_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(Cache::load_or_default(&path), Cache::default());
        let mut c = Cache::default();
        c.record_balance("Public/x", 7, 3, 1);
        c.save(&path);
        assert_eq!(Cache::load_or_default(&path).balances["Public/x"].balance, "7");
    }
}
