//! `testimonial_core` — Shared Borsh state structs, SHA-256 16-shard routing,
//! and 32-entry page rollover rules for the `lezer_testimonial` SPEL program.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Canonical LP-0021 submission identifier for Lezer Wallet.
pub const CANONICAL_SUBMISSION_ID: &str = "lp0021-lezer";

/// Total number of deterministic SHA-256 shards (`"00"`..`"0f"`).
pub const SHARD_COUNT: u8 = 16;

/// Hard maximum entries per `TestimonialShardPage` (`32 * 40B = 1,280B`).
pub const MAX_ENTRIES_PER_SHARD_PAGE: usize = 32;

/// On-chain record stored at `["testimonial", submission_id, author]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TestimonialRecord {
    /// Fixed LP-0021 submission identifier (`"lp0021-lezer"`).
    pub submission_id: String,
    /// The 32-byte AccountId of the signer who authored this testimonial.
    pub author: [u8; 32],
    /// Optional display handle (Discord/X/GitHub username, <= 64 bytes).
    pub username: String,
    /// Testimonial message (1..=2048 bytes).
    pub text: String,
    /// On-chain Unix timestamp (seconds).
    pub timestamp: u64,
    /// Uniform shard bucket (`0..16`) derived from `SHA256(author)[0] & 0x0F`.
    pub shard_id: u8,
    /// Page index within this shard (`0, 1, ...`; max 32 entries per page).
    pub page_id: u8,
}

/// Paginated shard index stored at `["testimonial-index", submission_id, shard_key, page_key]`.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize,
)]
pub struct TestimonialShardPage {
    pub submission_id: String,
    pub shard_id: u8,
    pub page_id: u8,
    /// Bounded list of `(author_pubkey, timestamp)` in this shard page (max 32 entries).
    pub entries: Vec<([u8; 32], u64)>,
}

/// Deterministically compute the uniform shard bucket (`0..16`) and 2-hex-digit `shard_key`
/// (`"00"`..`"0f"`) from a 32-byte author account ID (`SHA256(author)[0] & 0x0f`).
pub fn compute_author_shard(author_bytes: &[u8; 32]) -> (u8, String) {
    let digest: [u8; 32] = Sha256::digest(author_bytes).into();
    let shard_id = digest[0] & 0x0f;
    (shard_id, format!("{shard_id:02x}"))
}

/// Format a page index (`0..=255`) as a canonical 2-hex-digit `page_key` (`"00"`..`"ff"`).
pub fn format_page_key(page_id: u8) -> String {
    format!("{page_id:02x}")
}

/// Validate and apply a `submit_testimonial` transition in-memory (shared between
/// the RISC Zero guest and host-side simulation / conformance tests).
pub fn apply_submit_testimonial(
    existing_record: Option<&TestimonialRecord>,
    current_page: Option<TestimonialShardPage>,
    author: [u8; 32],
    submission_id: &str,
    shard_key: &str,
    page_key: &str,
    username: &str,
    text: &str,
    timestamp: u64,
) -> Result<(TestimonialRecord, TestimonialShardPage), &'static str> {
    if existing_record.is_some() {
        return Err("duplicate_author_record");
    }
    if submission_id != CANONICAL_SUBMISSION_ID {
        return Err("invalid_submission_id");
    }
    if text.is_empty() || text.len() > 2048 {
        return Err("invalid_text_length");
    }
    if username.len() > 64 {
        return Err("invalid_username_length");
    }
    if timestamp == 0 {
        return Err("invalid_timestamp");
    }

    let (expected_shard_id, expected_shard_key) = compute_author_shard(&author);
    if shard_key != expected_shard_key {
        return Err("shard_key_mismatch");
    }

    let page_id = u8::from_str_radix(page_key, 16).map_err(|_| "invalid_page_key")?;
    if page_key != format_page_key(page_id) {
        return Err("non_canonical_page_key");
    }

    let mut page = current_page.unwrap_or_else(|| TestimonialShardPage {
        submission_id: submission_id.to_string(),
        shard_id: expected_shard_id,
        page_id,
        entries: Vec::new(),
    });

    if page.entries.len() >= MAX_ENTRIES_PER_SHARD_PAGE {
        return Err("shard_page_full");
    }

    page.entries.push((author, timestamp));

    let record = TestimonialRecord {
        submission_id: submission_id.to_string(),
        author,
        username: username.to_string(),
        text: text.to_string(),
        timestamp,
        shard_id: expected_shard_id,
        page_id,
    };

    Ok((record, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_sha256_shard_and_32_entry_page_rollover() {
        let mut page: Option<TestimonialShardPage> = None;
        let author0 = [7u8; 32];
        let (_, shard_key) = compute_author_shard(&author0);

        // Wrong shard key is rejected
        assert_eq!(
            apply_submit_testimonial(
                None,
                None,
                author0,
                CANONICAL_SUBMISSION_ID,
                "ff",
                "00",
                "@alice",
                "I use Lezer Wallet",
                1_760_000_000,
            )
            .unwrap_err(),
            "shard_key_mismatch"
        );

        // Fill 32 entries on page "00"
        for i in 0..MAX_ENTRIES_PER_SHARD_PAGE {
            let (_rec, next_page) = apply_submit_testimonial(
                None,
                page,
                author0,
                CANONICAL_SUBMISSION_ID,
                &shard_key,
                "00",
                "@user",
                "I use Lezer Wallet",
                1_760_000_000 + i as u64,
            )
            .unwrap();
            page = Some(next_page);
        }

        assert_eq!(page.as_ref().unwrap().entries.len(), 32);

        // 33rd entry on page "00" must fail with shard_page_full
        assert_eq!(
            apply_submit_testimonial(
                None,
                page,
                author0,
                CANONICAL_SUBMISSION_ID,
                &shard_key,
                "00",
                "@overflow",
                "I use Lezer Wallet",
                1_760_000_999,
            )
            .unwrap_err(),
            "shard_page_full"
        );

        // Rolling over to page "01" succeeds cleanly
        let (rec_p1, page_01) = apply_submit_testimonial(
            None,
            None,
            author0,
            CANONICAL_SUBMISSION_ID,
            &shard_key,
            "01",
            "@page1",
            "I use Lezer Wallet on page 01",
            1_760_001_000,
        )
        .unwrap();
        assert_eq!(rec_p1.page_id, 1);
        assert_eq!(page_01.entries.len(), 1);
    }
}
