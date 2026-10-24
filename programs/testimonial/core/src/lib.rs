//! Types and derivations shared by the `lezer_testimonial` guest program and the wallet.
//!
//! On-chain layout (LEZ v0.3 sharded accounts — a program writes only its own shard):
//! - The testimonial itself lives in this program's shard on the **author's** account,
//!   written once; a second submission by the same author fails.
//! - An append-only index of authors lives in this program's shard on deterministic
//!   index accounts `for_public_pda(program, page_seed(bucket, page))`, so anyone can
//!   enumerate every testimonial with exact-account reads only (no indexer, no scans).
//!   Authors are spread over [`BUCKETS`] buckets by `sha256(author)[0] % BUCKETS`; each
//!   index page holds at most [`MAX_ENTRIES_PER_PAGE`] authors, then the next page is used.

#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest as _, Sha256};

pub const BUCKETS: u8 = 16;
pub const MAX_ENTRIES_PER_PAGE: usize = 64;
pub const MAX_USERNAME_BYTES: usize = 64;
pub const MAX_TEXT_BYTES: usize = 1024;

/// Top-level instruction sent to the program.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Accounts: `[author (signer), index_page]`.
    Submit(Submit),
    /// Accounts: `[entry, index_page]` — PDAs OF THE REGISTRY for THIS program.
    /// Emits a chained call to `params.registry`, so the entry is keyed by this program.
    RegisterSelf(registry_core::RegisterSelfParams),
}

/// Testimonial payload. Accounts: `[author (signer), index_page]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Submit {
    pub username: String,
    pub text: String,
    /// Index page the client expects to have room (`0, 1, …`). If full, the tx fails
    /// and the client retries with `page + 1`.
    pub page: u16,
}

/// Stored in the author's account (this program's shard).
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Testimonial {
    pub version: u8,
    pub username: String,
    pub text: String,
}

/// Effects planned for the two accounts.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    WriteRecord(Testimonial),
    AppendAuthor([u8; 32]),
}

/// Index page contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IndexPage {
    pub authors: Vec<[u8; 32]>,
}

pub fn bucket_of(author: &[u8; 32]) -> u8 {
    Sha256::digest(author)[0] % BUCKETS
}

/// PDA seed of index page `page` in `bucket`.
pub fn page_seed(bucket: u8, page: u16) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"lezer-testimonial-index-v1");
    h.update([bucket]);
    h.update(page.to_le_bytes());
    h.finalize().into()
}

/// Validate user input; returns an error message for the client / guest panic.
pub fn validate(submit: &Submit) -> Result<(), &'static str> {
    if submit.text.trim().is_empty() {
        return Err("testimonial text must not be empty");
    }
    if submit.text.len() > MAX_TEXT_BYTES {
        return Err("testimonial text is too long");
    }
    if submit.username.len() > MAX_USERNAME_BYTES {
        return Err("username is too long");
    }
    Ok(())
}

/// Apply step for an index page: append `author` unless already present or full.
pub fn append_author(pre: &[u8], author: [u8; 32]) -> Result<IndexPage, &'static str> {
    let mut page = if pre.is_empty() {
        IndexPage::default()
    } else {
        IndexPage::try_from_slice(pre).map_err(|_| "corrupt index page")?
    };
    if page.authors.contains(&author) {
        return Err("author already indexed");
    }
    if page.authors.len() >= MAX_ENTRIES_PER_PAGE {
        return Err("index page full");
    }
    page.authors.push(author);
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn index_page_appends_rejects_duplicates_and_overflow() {
        let a = [1u8; 32];
        let page = append_author(&[], a).unwrap();
        let bytes = borsh::to_vec(&page).unwrap();
        assert_eq!(append_author(&bytes, a), Err("author already indexed"));

        let mut full = IndexPage::default();
        for i in 0..MAX_ENTRIES_PER_PAGE {
            full.authors.push([i as u8; 32]);
        }
        assert_eq!(append_author(&borsh::to_vec(&full).unwrap(), [255; 32]), Err("index page full"));
    }

    #[test]
    fn buckets_and_seeds_are_deterministic_and_distinct() {
        assert!(bucket_of(&[7; 32]) < BUCKETS);
        assert_eq!(page_seed(3, 1), page_seed(3, 1));
        assert_ne!(page_seed(3, 1), page_seed(3, 2));
        assert_ne!(page_seed(3, 1), page_seed(4, 1));
    }

    #[test]
    fn validation() {
        let ok = Submit { username: "@a".to_string(), text: "great".to_string(), page: 0 };
        assert!(validate(&ok).is_ok());
        assert!(validate(&Submit { text: " ".to_string(), ..ok.clone() }).is_err());
    }
}
