//! Release Gate RC-03: 50+ Account Concurrent Sharded Testimonial Stress Test
//! (Same-Shard Contention + 32-Entry Page Overflow `"00" -> "01"`) & Stateless Clean-Machine
//! Verifier Audit (`LEZER_NO_VAULT=1 lezer verify-adoption --submission-id lp0021-lezer`).

use std::process::Command;
use std::sync::Arc;

use lezer_adapter::derive_public_account;
use lezer_core::{
    CANONICAL_SUBMISSION_ID, compute_author_shard_for_account, submit_testimonial_to_ledger,
};
use serde_json::Value;

const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

fn lezer_bin() -> String {
    env!("CARGO_BIN_EXE_lezer").to_string()
}

#[test]
fn gate_rc03_concurrent_50_testimonials_shard_overflow_and_stateless_verify_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = Arc::new(dir.path().join("testimonials.json"));

    // 1. Mine 35 distinct accounts that all hash to shard "00" (`SHA256(author)[0] & 0x0f == 0`)
    //    to force shard "00" past `MAX_ENTRIES_PER_SHARD_PAGE = 32` (rolling over `"00" -> "01"`),
    //    plus 20 accounts across other shards (55 distinct accounts total).
    let mut shard0_accounts = Vec::with_capacity(35);
    let mut other_accounts = Vec::with_capacity(20);
    let mut idx = 0u32;

    while shard0_accounts.len() < 35 || other_accounts.len() < 20 {
        let derived = derive_public_account(TEST_MNEMONIC, idx);
        let (_bytes, shard_id, _key) =
            compute_author_shard_for_account(&derived.account_id).unwrap();
        if shard_id == 0 && shard0_accounts.len() < 35 {
            shard0_accounts.push(derived.account_id);
        } else if shard_id != 0 && other_accounts.len() < 20 {
            other_accounts.push(derived.account_id);
        }
        idx += 1;
    }

    let mut all_accounts = Vec::with_capacity(55);
    all_accounts.extend(shard0_accounts);
    all_accounts.extend(other_accounts);
    assert_eq!(all_accounts.len(), 55);

    // 2. Submit all 55 distinct-account testimonials concurrently across threads
    let mut handles = Vec::with_capacity(all_accounts.len());
    for (i, acct) in all_accounts.into_iter().enumerate() {
        let lp = Arc::clone(&ledger_path);
        handles.push(std::thread::spawn(move || {
            // Split timestamps across 2 calendar months to also verify monthly breakdown
            let ts = if i < 25 {
                1_760_000_000 + (i as u64) * 60 // 2025-10
            } else {
                1_763_000_000 + (i as u64) * 60 // 2025-11
            };
            submit_testimonial_to_ledger(
                &lp,
                &acct,
                CANONICAL_SUBMISSION_ID,
                &format!("@builder_{i}"),
                &format!("I use Lezer Wallet on LEZ (#{i})"),
                ts,
                (i % 3) as u32,
            )
            .expect("concurrent testimonial submit must succeed")
        }));
    }

    let mut page_01_count = 0usize;
    for h in handles {
        let receipt = h.join().unwrap();
        if receipt.shard_key == "00" && receipt.page_key == "01" {
            page_01_count += 1;
        }
    }
    // 35 accounts in shard "00" => 32 on page "00" and 3 on page "01"
    assert_eq!(
        page_01_count, 3,
        "shard 00 must overflow at 32 entries onto page 01 with 3 entries"
    );

    // 3. Run `LEZER_NO_VAULT=1 lezer verify-adoption --submission-id lp0021-lezer` in a clean dir with NO vault.enc
    let clean_vault_path = dir.path().join("nonexistent_vault.enc");
    assert!(!clean_vault_path.exists());

    let output = Command::new(lezer_bin())
        .env("LEZER_NO_VAULT", "1")
        .env("LEZER_TESTIMONIAL_LEDGER", ledger_path.as_os_str())
        .arg("--json")
        .arg("--vault")
        .arg(&clean_vault_path)
        .args([
            "verify-adoption",
            "--submission-id",
            CANONICAL_SUBMISSION_ID,
            "--sequencer",
            "https://testnet.lez.logos.co",
        ])
        .output()
        .expect("spawn lezer verify-adoption");

    assert!(
        output.status.success(),
        "verify-adoption failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["submission_id"], CANONICAL_SUBMISSION_ID);
    assert_eq!(report["stateless_no_vault_mode"], true);
    assert_eq!(report["shards_scanned"], 16);
    assert_eq!(report["total_distinct_authors"], 55);
    assert_eq!(report["shards_summary"]["00"], 35);
    assert_eq!(report["pages_per_shard"]["00"], 2);
    assert_eq!(report["monthly_breakdown"].as_object().unwrap().len(), 2);
}
