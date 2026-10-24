//! Types shared by the `lezer_registry` guest and readers.
//!
//! The registry is a flat directory of registered programs. Entries are keyed by the
//! *registering program's own account id* — it reaches the guest as `caller_account_id` of a
//! chained call, so only a program can register itself; no forgeable "deployer" field.
//! Enumeration is by exact-account reads over 16 index buckets with overflow pages.

#![no_std]
extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest as _, Sha256};

/// Index fan-out. `account_id[0] % 16` chooses the bucket page chain.
pub const BUCKETS: u8 = 16;
pub const MAX_ENTRIES_PER_PAGE: usize = 32;
pub const MAX_PAGES_PER_BUCKET: u8 = 64;
pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_URL_BYTES: usize = 256;
pub const MAX_COMMIT_BYTES: usize = 40;
pub const MAX_VERSION_BYTES: usize = 24;

/// Self-registration payload carried inside each registerable program's own instruction.
/// `page` is the index page the client believes still has room in this program's bucket.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RegisterSelfParams {
    /// The registry program's account id.
    pub registry: [u8; 32],
    /// Index page within the program's bucket (`0`, `1`, …).
    pub page: u8,
    pub name: String,
    pub source_url: String,
    pub commit: String,
    pub version: String,
}

/// `Instruction` sent to the registry.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Accounts: `[entry, index_page]` — both derived from `caller_account_id`.
    /// Only meaningful as a chained call: `caller_account_id` IS the registered program.
    Register { name: String, source_url: String, commit: String, version: String },
}

/// Program shard on the entry account: one registration per program.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Entry {
    /// The registered program's account id (= chained caller).
    pub program_id: [u8; 32],
    pub name: String,
    pub source_url: String,
    pub commit: String,
    pub version: String,
}

/// Program shard on an index-page account: a page of registered program ids.
/// Pages are enumerated by deterministic seeds (bucket, page) — a full page just means the
/// next write goes to `page+1`; readers walk page numbers until a page is uninitialized.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IndexPage {
    pub entries: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    /// `previous` is Some(old entry) on re-registration; the index is left untouched.
    WriteEntry(Entry),
    AppendToIndex { program_id: [u8; 32] },
}

pub fn entry_seed(program_id: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lezer-registry-entry-v1");
    h.update(program_id);
    h.finalize().into()
}

/// Index-page seed for bucket `bucket`, page `page` ("00", "01", …).
pub fn index_page_seed(bucket: u8, page: u8) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lezer-registry-index-v1");
    h.update([bucket]);
    h.update(format!("{page:02}").as_bytes());
    h.finalize().into()
}

/// Bucket for a registered program id.
pub fn bucket_of(program_id: &[u8; 32]) -> u8 {
    program_id[0] % BUCKETS
}

pub fn validate_metadata(name: &str, url: &str, commit: &str, version: &str) -> Result<(), &'static str> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err("invalid program name");
    }
    if url.len() > MAX_URL_BYTES || commit.len() > MAX_COMMIT_BYTES || version.len() > MAX_VERSION_BYTES {
        return Err("metadata field too long");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_stable() {
        assert_eq!(index_page_seed(3, 0), index_page_seed(3, 0));
        assert_ne!(index_page_seed(3, 0), index_page_seed(3, 1));
        assert_ne!(index_page_seed(3, 0), index_page_seed(4, 0));
        assert_ne!(entry_seed(&[1; 32]), entry_seed(&[2; 32]));
    }

    #[test]
    fn buckets_cover_the_range() {
        assert!((0..255u8).all(|b| bucket_of(&[b; 32]) < BUCKETS));
    }
}
