#![no_main]

//! `lezer_testimonial` SPEL (RISC Zero) guest program (`submission_id == "lp0021-lezer"`).
//!
//! Enforces:
//! - `submission_id == "lp0021-lezer"`
//! - `shard_id == SHA256(author)[0] & 0x0f` (`shard_key` `"00"`..`"0f"`)
//! - `MAX_ENTRIES_PER_SHARD_PAGE = 32` with `page_key` (`"00"`, `"01"`, ...) rollover
//! - `register_self` chained call to `lezer_registry` for LP-0023 deployer verification.

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest as _, Sha256};
pub use testimonial_core::{
    CANONICAL_SUBMISSION_ID, MAX_ENTRIES_PER_SHARD_PAGE, SHARD_COUNT, TestimonialRecord,
    TestimonialShardPage, apply_submit_testimonial, compute_author_shard, format_page_key,
};

/// Execute `submit_testimonial` inside the RISC Zero guest environment.
pub fn submit_testimonial_guest(
    existing_entry_bytes: &[u8],
    shard_page_bytes: &[u8],
    author: [u8; 32],
    submission_id: &str,
    shard_key: &str,
    page_key: &str,
    username: &str,
    text: &str,
    timestamp: u64,
) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
    let existing = if existing_entry_bytes.is_empty() {
        None
    } else {
        Some(
            TestimonialRecord::try_from_slice(existing_entry_bytes)
                .map_err(|_| "corrupt_existing_record")?,
        )
    };

    let current_page = if shard_page_bytes.is_empty() {
        None
    } else {
        Some(
            TestimonialShardPage::try_from_slice(shard_page_bytes)
                .map_err(|_| "corrupt_shard_page")?,
        )
    };

    let ( digest_shard, _) = {
        let d: [u8; 32] = Sha256::digest(author).into();
        (d[0] & 0x0f, ())
    };
    let _ = digest_shard;

    let (record, updated_page) = apply_submit_testimonial(
        existing.as_ref(),
        current_page,
        author,
        submission_id,
        shard_key,
        page_key,
        username,
        text,
        timestamp,
    )?;

    let record_bytes = borsh::to_vec(&record).map_err(|_| "serialize_record_failed")?;
    let page_bytes = borsh::to_vec(&updated_page).map_err(|_| "serialize_page_failed")?;
    Ok((record_bytes, page_bytes))
}
