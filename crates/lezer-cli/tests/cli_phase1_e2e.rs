//! End-to-end integration test for `lezer-cli` Phase 1 commands:
//! - `lezer init`, `lezer restore`, `lezer unlock`, `lezer lock` (including `LZR-1003` wrong password)
//! - `lezer account new --label alice`, `lezer account new --label bob`, `lezer account list`
//! - `lezer balance --account alice` (Offline Mode via `cache.json`)
//! - `lezer transfer --from alice --to bob --amount 50`
//! - Pure JSON output on `stdout` under `--json` (`dup2` stdout isolation)

use std::process::Command;

use lezer_core::CacheEnvelopeV1;
use serde_json::Value;

fn lezer_bin() -> String {
    env!("CARGO_BIN_EXE_lezer").to_string()
}

fn run_lezer(vault_path: &std::path::Path, password: &str, args: &[&str]) -> (bool, Value) {
    let output = Command::new(lezer_bin())
        .env("LEZER_KDF_MEMORY_KIB", "1024")
        .env("LEZER_SIMULATE_BROADCAST", "1")
        .env("LEZER_PASSWORD", password)
        .arg("--json")
        .arg("--vault")
        .arg(vault_path)
        .args(args)
        .output()
        .expect("spawn lezer CLI");

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout_str).unwrap_or_else(|e| {
        panic!(
            "CLI --json stdout was not pure valid JSON ({e}):\nSTDOUT:\n{stdout_str}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), parsed)
}

#[test]
fn cli_phase1_e2e_init_restore_unlock_accounts_offline_balance_and_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("vault.enc");
    let cache_path = dir.path().join("cache.json");

    // 1. `lezer init`
    let (ok, init_json) = run_lezer(&vault_path, "cli-pass-123", &["init"]);
    assert!(ok, "init failed: {init_json}");
    assert_eq!(init_json["status"], "initialized");
    let mnemonic = init_json["mnemonic_phrase"].as_str().unwrap().to_string();
    assert_eq!(mnemonic.split_whitespace().count(), 24);
    let main_pub = init_json["main_public_account"]["account_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Wrong password on `lezer unlock` returns LZR-1003
    let (ok_bad, bad_json) = run_lezer(&vault_path, "wrong-password", &["unlock"]);
    assert!(!ok_bad);
    assert_eq!(bad_json["error"]["code"], "LZR-1003");

    // 3. `lezer restore` into a second vault verifies identical deterministic account derivation
    let restore_vault = dir.path().join("restored_vault.enc");
    let (ok_rest, rest_json) = run_lezer(
        &restore_vault,
        "cli-pass-123",
        &["restore", "--mnemonic", &mnemonic],
    );
    assert!(ok_rest, "restore failed: {rest_json}");
    assert_eq!(
        rest_json["main_public_account"]["account_id"].as_str().unwrap(),
        main_pub
    );

    // 4. Create labeled accounts `alice` and `bob`
    let (ok_alice, alice_json) = run_lezer(
        &vault_path,
        "cli-pass-123",
        &["account", "new", "--label", "alice"],
    );
    assert!(ok_alice);
    let alice_id = alice_json["account"]["account_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (ok_bob, bob_json) = run_lezer(
        &vault_path,
        "cli-pass-123",
        &["account", "new", "--label", "bob"],
    );
    assert!(ok_bob);
    let bob_id = bob_json["account"]["account_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(alice_id, bob_id);

    // 5. Seed `cache.json` with 250 LEZ for `alice` and verify Offline Mode `lezer balance`
    let mut cache = CacheEnvelopeV1::load_or_rebuild(&cache_path);
    cache.update_native_balance(&alice_id, 250, 0, 36316);
    cache.save(&cache_path);

    let output_offline = Command::new(lezer_bin())
        .env("LEZER_KDF_MEMORY_KIB", "1024")
        .env("LEZER_OFFLINE", "1")
        .env("LEZER_PASSWORD", "cli-pass-123")
        .arg("--json")
        .arg("--vault")
        .arg(&vault_path)
        .args(["balance", "--account", "alice"])
        .output()
        .unwrap();
    assert!(output_offline.status.success());
    let bal_json: Value = serde_json::from_slice(&output_offline.stdout).unwrap();
    assert_eq!(bal_json["balance"], "250");
    assert_eq!(bal_json["is_cached_offline"], true);

    // 6. Execute `lezer transfer --from alice --to bob --amount 50`
    let (ok_tx, tx_json) = run_lezer(
        &vault_path,
        "cli-pass-123",
        &["transfer", "--from", "alice", "--to", "bob", "--amount", "50"],
    );
    assert!(ok_tx, "transfer failed: {tx_json}");
    assert_eq!(tx_json["from_account"], alice_id);
    assert_eq!(tx_json["to_account"], bob_id);
    assert_eq!(tx_json["amount"], 50);
    assert_eq!(tx_json["tx_hash"].as_str().unwrap().len(), 64);

    // 7. Verify `lezer lock` succeeds
    let (ok_lock, lock_json) = run_lezer(&vault_path, "cli-pass-123", &["lock"]);
    assert!(ok_lock);
    assert_eq!(lock_json["lock_state"], "locked");
}
