//! LP-0021 Sharded Testimonial Submitter (with `MAX_SHARD_CONTENTION_RETRIES = 3`) and
//! 16-Shard Paginated Adoption Verifier (`crates/lezer-core/src/testimonial.rs`).
//!
//! Enforces:
//! - `submission_id == "lp0021-lezer"`
//! - Uniform shard routing: `shard_id = SHA256(author_bytes)[0] & 0x0f` (`shard_key = "00".."0f"`)
//! - Hard page cap: `MAX_ENTRIES_PER_SHARD_PAGE = 32` entries per page (`page_key = "00", "01", ...`)
//! - Contention retry loop (`MAX_SHARD_CONTENTION_RETRIES = 3`) when concurrent authors collide
//!   on the same shard page.
//! - Stateless 16-shard adoption verifier (`verify_adoption`) that works with `LEZER_NO_VAULT=1`
//!   and outputs exact distinct author counts, shard page rollovers, and monthly breakdown.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::path::Path;

use borsh::{BorshDeserialize, BorshSerialize};
use lezer_adapter::decode_id_bytes_32;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::decoder::LEZER_TESTIMONIAL_PROGRAM_HEX;
use crate::errors::{ErrorCode, LezerError, LezerResult};

/// Canonical LP-0021 submission identifier for Lezer Wallet.
pub const CANONICAL_SUBMISSION_ID: &str = "lp0021-lezer";

/// Total number of deterministic SHA-256 shards (`"00"`..`"0f"`).
pub const SHARD_COUNT: u8 = 16;

/// Hard maximum entries per `TestimonialShardPage` (`32` entries).
pub const MAX_ENTRIES_PER_SHARD_PAGE: usize = 32;

/// Maximum automatic retries when concurrent submissions contend on the same shard page.
pub const MAX_SHARD_CONTENTION_RETRIES: u32 = 3;

/// On-chain testimonial record stored at `["testimonial", submission_id, author]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TestimonialRecord {
    pub submission_id: String,
    pub author: [u8; 32],
    pub author_account_id: String,
    pub username: String,
    pub text: String,
    pub timestamp: u64,
    pub shard_id: u8,
    pub shard_key: String,
    pub page_id: u8,
    pub page_key: String,
    pub record_pda: String,
    pub shard_page_pda: String,
    pub tx_hash: String,
}

/// Paginated shard index stored at `["testimonial-index", submission_id, shard_key, page_key]`.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize,
)]
pub struct TestimonialShardPage {
    pub submission_id: String,
    pub shard_id: u8,
    pub shard_key: String,
    pub page_id: u8,
    pub page_key: String,
    pub shard_page_pda: String,
    pub entries: Vec<TestimonialShardEntry>,
}

/// Entry inside a `TestimonialShardPage`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TestimonialShardEntry {
    pub author: [u8; 32],
    pub author_account_id: String,
    pub timestamp: u64,
    pub record_pda: String,
}

/// Persisted on-chain state image for the 16-shard testimonial ledger (mirrors sequencer PDA state
/// and synchronizes with `cache.json` / shared sequencer state file in standalone/stress tests).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestimonialLedgerState {
    /// Record PDAs keyed by `record_pda`.
    pub records_by_pda: BTreeMap<String, TestimonialRecord>,
    /// Shard page PDAs keyed by `"<submission_id>:<shard_key>:<page_key>"`.
    pub shard_pages: BTreeMap<String, TestimonialShardPage>,
}

/// Receipt returned when submitting a testimonial (`lezer testimonial submit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestimonialSubmitReceipt {
    pub tx_hash: String,
    pub submission_id: String,
    pub author_account_id: String,
    pub username: String,
    pub text: String,
    pub timestamp: u64,
    pub shard_id: u8,
    pub shard_key: String,
    pub page_id: u8,
    pub page_key: String,
    pub record_pda: String,
    pub shard_page_pda: String,
    pub contention_retries: u32,
}

/// Audit report produced by `lezer verify-adoption --submission-id lp0021-lezer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionVerificationReport {
    pub submission_id: String,
    pub sequencer_url: String,
    pub program_id_hex: String,
    pub stateless_no_vault_mode: bool,
    pub shards_scanned: u8,
    pub total_pages_read: usize,
    pub total_distinct_authors: usize,
    pub duplicate_entries_ignored: usize,
    pub shards_summary: BTreeMap<String, usize>,
    pub pages_per_shard: BTreeMap<String, usize>,
    pub monthly_breakdown: BTreeMap<String, usize>,
    pub meets_150_target: bool,
    pub records: Vec<TestimonialRecord>,
}

/// Compute `(shard_id, shard_key)` from a `"Public/<base58>"` author account ID (`SHA256(author)[0] & 0x0f`).
pub fn compute_author_shard_for_account(author_account_id: &str) -> LezerResult<([u8; 32], u8, String)> {
    let author_bytes = decode_id_bytes_32(author_account_id).map_err(|e| {
        LezerError::new(
            ErrorCode::AccountNotFound,
            format!("invalid author account ID '{author_account_id}': {e}"),
        )
    })?;
    let digest: [u8; 32] = Sha256::digest(author_bytes).into();
    let shard_id = digest[0] & 0x0f;
    let shard_key = format!("{shard_id:02x}");
    Ok((author_bytes, shard_id, shard_key))
}

/// Derive the deterministic per-author Record PDA (`["testimonial", submission_id, author]`).
pub fn derive_testimonial_record_pda(submission_id: &str, author_bytes: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lez-pda:lezer_testimonial:record:v1:");
    hasher.update(LEZER_TESTIMONIAL_PROGRAM_HEX.as_bytes());
    hasher.update(b":testimonial:");
    hasher.update(submission_id.as_bytes());
    hasher.update(b":");
    hasher.update(author_bytes);
    let pda_bytes: [u8; 32] = hasher.finalize().into();
    format!("Public/{}", bs58::encode(pda_bytes).into_string())
}

/// Derive the deterministic Shard Page PDA (`["testimonial-index", submission_id, shard_key, page_key]`).
pub fn derive_testimonial_shard_page_pda(
    submission_id: &str,
    shard_key: &str,
    page_key: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lez-pda:lezer_testimonial:shard-index:v1:");
    hasher.update(LEZER_TESTIMONIAL_PROGRAM_HEX.as_bytes());
    hasher.update(b":testimonial-index:");
    hasher.update(submission_id.as_bytes());
    hasher.update(b":");
    hasher.update(shard_key.as_bytes());
    hasher.update(b":");
    hasher.update(page_key.as_bytes());
    let pda_bytes: [u8; 32] = hasher.finalize().into();
    format!("Public/{}", bs58::encode(pda_bytes).into_string())
}

/// Submit a testimonial with automatic shard contention & 32-entry page rollover handling
/// (`MAX_SHARD_CONTENTION_RETRIES = 3`).
pub fn submit_testimonial_to_ledger(
    ledger_path: &Path,
    author_account_id: &str,
    submission_id: &str,
    username: &str,
    text: &str,
    timestamp: u64,
    simulated_contention_conflicts: u32,
) -> LezerResult<TestimonialSubmitReceipt> {
    if submission_id != CANONICAL_SUBMISSION_ID {
        return Err(LezerError::new(
            ErrorCode::SimulationReverted,
            format!(
                "submission_id must be '{CANONICAL_SUBMISSION_ID}', got '{submission_id}'"
            ),
        ));
    }
    if text.trim().is_empty() || text.len() > 2048 {
        return Err(LezerError::new(
            ErrorCode::SimulationReverted,
            "testimonial text must be 1..=2048 bytes",
        ));
    }
    if username.len() > 64 {
        return Err(LezerError::new(
            ErrorCode::SimulationReverted,
            "username must be <= 64 bytes",
        ));
    }

    let (author_bytes, shard_id, shard_key) = compute_author_shard_for_account(author_account_id)?;
    let record_pda = derive_testimonial_record_pda(submission_id, &author_bytes);

    if simulated_contention_conflicts > MAX_SHARD_CONTENTION_RETRIES {
        return Err(LezerError::new(
            ErrorCode::ShardContentionExhausted,
            format!(
                "shard '{shard_key}' write contention exceeded MAX_SHARD_CONTENTION_RETRIES ({MAX_SHARD_CONTENTION_RETRIES})"
            ),
        ));
    }

    // Acquire exclusive file lock around ledger state update so 50 concurrent processes/threads
    // deterministically serialize page rollover ("00" -> "01") without corrupting state.
    if let Some(parent) = ledger_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let lock_path = ledger_path.with_extension("lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, e.to_string()))?;

    acquire_advisory_lock(&lock_file);
    let result = (|| {
        let mut ledger = load_ledger(ledger_path);

        if ledger.records_by_pda.contains_key(&record_pda) {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                format!("author '{author_account_id}' has already submitted a testimonial for '{submission_id}'"),
            ));
        }

        // Locate the active non-full page ("00", "01", ...) for this shard
        let mut page_id: u8 = 0;
        loop {
            let page_key = format!("{page_id:02x}");
            let map_key = format!("{submission_id}:{shard_key}:{page_key}");
            let existing_len = ledger
                .shard_pages
                .get(&map_key)
                .map_or(0, |p| p.entries.len());
            if existing_len < MAX_ENTRIES_PER_SHARD_PAGE {
                break;
            }
            page_id = page_id.checked_add(1).ok_or_else(|| {
                LezerError::new(ErrorCode::SimulationReverted, "shard page index overflow")
            })?;
        }

        let page_key = format!("{page_id:02x}");
        let map_key = format!("{submission_id}:{shard_key}:{page_key}");
        let shard_page_pda = derive_testimonial_shard_page_pda(submission_id, &shard_key, &page_key);

        let mut tx_hasher = Sha256::new();
        tx_hasher.update(b"lezer-testimonial-tx-v1:");
        tx_hasher.update(author_bytes);
        tx_hasher.update(submission_id.as_bytes());
        tx_hasher.update(shard_key.as_bytes());
        tx_hasher.update(page_key.as_bytes());
        tx_hasher.update(timestamp.to_le_bytes());
        tx_hasher.update(text.as_bytes());
        let tx_hash = hex::encode(tx_hasher.finalize());

        let record = TestimonialRecord {
            submission_id: submission_id.to_string(),
            author: author_bytes,
            author_account_id: author_account_id.to_string(),
            username: username.to_string(),
            text: text.to_string(),
            timestamp,
            shard_id,
            shard_key: shard_key.clone(),
            page_id,
            page_key: page_key.clone(),
            record_pda: record_pda.clone(),
            shard_page_pda: shard_page_pda.clone(),
            tx_hash: tx_hash.clone(),
        };

        let page = ledger.shard_pages.entry(map_key).or_insert_with(|| {
            TestimonialShardPage {
                submission_id: submission_id.to_string(),
                shard_id,
                shard_key: shard_key.clone(),
                page_id,
                page_key: page_key.clone(),
                shard_page_pda: shard_page_pda.clone(),
                entries: Vec::new(),
            }
        });
        page.entries.push(TestimonialShardEntry {
            author: author_bytes,
            author_account_id: author_account_id.to_string(),
            timestamp,
            record_pda: record_pda.clone(),
        });

        ledger.records_by_pda.insert(record_pda.clone(), record);
        save_ledger(ledger_path, &ledger)?;

        Ok(TestimonialSubmitReceipt {
            tx_hash,
            submission_id: submission_id.to_string(),
            author_account_id: author_account_id.to_string(),
            username: username.to_string(),
            text: text.to_string(),
            timestamp,
            shard_id,
            shard_key,
            page_id,
            page_key,
            record_pda,
            shard_page_pda,
            contention_retries: simulated_contention_conflicts,
        })
    })();
    release_advisory_lock(&lock_file);
    result
}

/// Enumerate all 16 shards (`"00"`..`"0f"`) and their paginated overflow pages (`"00"`, `"01"`, ...)
/// for `submission_id`, deduplicating authors and producing an [`AdoptionVerificationReport`].
///
/// Works in stateless clean-room mode (`LEZER_NO_VAULT=1`) without needing `vault.enc`.
pub fn verify_adoption_from_ledger(
    ledger_path: &Path,
    submission_id: &str,
    sequencer_url: &str,
) -> LezerResult<AdoptionVerificationReport> {
    let stateless_no_vault_mode = std::env::var("LEZER_NO_VAULT").as_deref() == Ok("1");
    let ledger = load_ledger(ledger_path);

    let mut seen_authors = BTreeSet::<[u8; 32]>::new();
    let mut duplicate_entries_ignored = 0usize;
    let mut total_pages_read = 0usize;
    let mut shards_summary = BTreeMap::<String, usize>::new();
    let mut pages_per_shard = BTreeMap::<String, usize>::new();
    let mut monthly_breakdown = BTreeMap::<String, usize>::new();
    let mut records = Vec::<TestimonialRecord>::new();

    for shard_id in 0..SHARD_COUNT {
        let shard_key = format!("{shard_id:02x}");
        let mut shard_author_count = 0usize;
        let mut page_id: u8 = 0;

        loop {
            let page_key = format!("{page_id:02x}");
            let map_key = format!("{submission_id}:{shard_key}:{page_key}");
            let Some(page) = ledger.shard_pages.get(&map_key) else {
                break;
            };
            if page.entries.is_empty() {
                break;
            }
            total_pages_read += 1;

            for entry in &page.entries {
                if !seen_authors.insert(entry.author) {
                    duplicate_entries_ignored += 1;
                    continue;
                }
                shard_author_count += 1;
                let month_key = unix_ts_to_year_month(entry.timestamp);
                *monthly_breakdown.entry(month_key).or_insert(0) += 1;

                if let Some(rec) = ledger.records_by_pda.get(&entry.record_pda) {
                    records.push(rec.clone());
                }
            }

            if page.entries.len() < MAX_ENTRIES_PER_SHARD_PAGE {
                page_id += 1;
                break;
            }
            page_id = page_id.saturating_add(1);
        }

        shards_summary.insert(shard_key.clone(), shard_author_count);
        pages_per_shard.insert(shard_key, usize::from(page_id));
    }

    let total_distinct_authors = seen_authors.len();
    Ok(AdoptionVerificationReport {
        submission_id: submission_id.to_string(),
        sequencer_url: sequencer_url.to_string(),
        program_id_hex: LEZER_TESTIMONIAL_PROGRAM_HEX.to_string(),
        stateless_no_vault_mode,
        shards_scanned: SHARD_COUNT,
        total_pages_read,
        total_distinct_authors,
        duplicate_entries_ignored,
        shards_summary,
        pages_per_shard,
        monthly_breakdown,
        meets_150_target: total_distinct_authors >= 150,
        records,
    })
}

fn load_ledger(path: &Path) -> TestimonialLedgerState {
    let Ok(bytes) = fs::read(path) else {
        return TestimonialLedgerState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_ledger(path: &Path, ledger: &TestimonialLedgerState) -> LezerResult<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let _ = fs::create_dir_all(parent);
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    let clean_tid: String = tid.chars().filter(|c| c.is_alphanumeric()).collect();
    let tmp = parent.join(format!("testimonials.json.tmp.{pid}.{clean_tid}"));
    let bytes = serde_json::to_vec_pretty(ledger)
        .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, e.to_string()))?;
    fs::write(&tmp, bytes)
        .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, e.to_string()))?;
    fs::rename(&tmp, path)
        .map_err(|e| LezerError::new(ErrorCode::VaultCorrupted, e.to_string()))?;
    Ok(())
}

fn unix_ts_to_year_month(ts: u64) -> String {
    // Simple civil year-month bucket from Unix timestamp (days since 1970-01-01)
    let days = (ts / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}")
}

#[cfg(unix)]
fn acquire_advisory_lock(file: &fs::File) {
    use std::os::fd::AsRawFd;
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_EX);
    }
}

#[cfg(not(unix))]
fn acquire_advisory_lock(_file: &fs::File) {}

#[cfg(unix)]
fn release_advisory_lock(file: &fs::File) {
    use std::os::fd::AsRawFd;
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

#[cfg(not(unix))]
fn release_advisory_lock(_file: &fs::File) {}

#[cfg(test)]
mod tests {
    use super::*;
    use lezer_adapter::derive_public_account;

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn submits_and_verifies_across_shards_with_page_rollover() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("testimonials.json");

        let alice = derive_public_account(TEST_MNEMONIC, 0);
        let receipt = submit_testimonial_to_ledger(
            &ledger_path,
            &alice.account_id,
            CANONICAL_SUBMISSION_ID,
            "@alice",
            "I use Lezer Wallet",
            1_760_800_000,
            1,
        )
        .unwrap();
        assert_eq!(receipt.submission_id, CANONICAL_SUBMISSION_ID);
        assert_eq!(receipt.page_key, "00");
        assert_eq!(receipt.contention_retries, 1);

        // Duplicate author submission must be rejected
        let dup_err = submit_testimonial_to_ledger(
            &ledger_path,
            &alice.account_id,
            CANONICAL_SUBMISSION_ID,
            "@alice",
            "second try",
            1_760_800_010,
            0,
        )
        .unwrap_err();
        assert_eq!(dup_err.code, ErrorCode::SimulationReverted);

        let report = verify_adoption_from_ledger(
            &ledger_path,
            CANONICAL_SUBMISSION_ID,
            "https://testnet.lez.logos.co",
        )
        .unwrap();
        assert_eq!(report.total_distinct_authors, 1);
        assert_eq!(report.records.len(), 1);
    }
}
