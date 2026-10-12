//! Phase 3 Exit Gate E2E Test (`cargo test -p lezer-cli --test token_ata_e2e`):
//! - Claim 150 LEZ from Piñata PoW faucet (`lezer faucet info`, `lezer faucet claim --to alice`)
//! - Create fungible token `LEZERUSD` (`lezer token create --name LEZERUSD --supply 1000000 --creator alice --authority alice`)
//! - Deterministically derive and initialize ATAs (`lezer token ata-address`, `lezer token ata-create`)
//! - Transfer `LEZERUSD` between accounts (`lezer token send --from alice --to bob --mint LEZERUSD --amount 250`)
//! - Mint additional supply under LP-0013 (`lezer token mint --mint LEZERUSD --to bob --amount 500 --authority alice`)
//! - Burn supply (`lezer token burn --from bob --mint LEZERUSD --amount 50`)
//! - Rotate & permanently revoke `mint_authority` (`lezer token set-authority --mint LEZERUSD --authority alice --new-authority none`)
//!   and verify subsequent mints fail closed (`LZR-3003`).

use std::process::Command;

use serde_json::Value;

fn lezer_bin() -> String {
    env!("CARGO_BIN_EXE_lezer").to_string()
}

fn run_lezer(
    vault_path: &std::path::Path,
    ledger_path: &std::path::Path,
    password: &str,
    args: &[&str],
) -> (bool, Value) {
    let output = Command::new(lezer_bin())
        .env("LEZER_KDF_MEMORY_KIB", "1024")
        .env("LEZER_SIMULATE_BROADCAST", "1")
        .env("LEZER_TESTIMONIAL_LEDGER", ledger_path)
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
            "Invalid JSON ({e}):\nSTDOUT:\n{stdout_str}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), parsed)
}

#[test]
fn phase3_faucet_token_ata_and_lp0013_authority_lifecycle_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("vault.enc");
    let ledger_path = dir.path().join("testimonials.json");
    let pw = "phase3-e2e-pw";

    // 1. Initialize vault and create `alice` & `bob`
    let (ok, _) = run_lezer(&vault_path, &ledger_path, pw, &["init"]);
    assert!(ok);
    let (ok, alice_res) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["account", "new", "--label", "alice"],
    );
    assert!(ok);
    let alice_id = alice_res["account"]["account_id"].as_str().unwrap().to_string();

    let (ok, bob_res) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["account", "new", "--label", "bob"],
    );
    assert!(ok);
    let bob_id = bob_res["account"]["account_id"].as_str().unwrap().to_string();

    // 2. Query Faucet Info & Claim 150 LEZ to `alice`
    let (ok_info, info_json) = run_lezer(&vault_path, &ledger_path, pw, &["faucet", "info"]);
    assert!(ok_info);
    assert_eq!(info_json["prize_amount"], "150");
    assert_eq!(info_json["can_claim"], true);

    let (ok_claim, claim_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["faucet", "claim", "--to", "alice"],
    );
    assert!(ok_claim, "faucet claim failed: {claim_json}");
    assert_eq!(claim_json["recipient_account"], alice_id);
    assert_eq!(claim_json["amount_claimed"], "150");
    assert_eq!(claim_json["balance_after"], "150");

    // 3. Submit an LP-0021 testimonial via CLI
    let (ok_test, test_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "testimonial",
            "submit",
            "--author",
            "alice",
            "--username",
            "@alice",
            "--text",
            "I use Lezer Wallet",
        ],
    );
    assert!(ok_test, "testimonial submit failed: {test_json}");
    assert_eq!(test_json["submission_id"], "lp0021-lezer");

    // 4. Create fungible token `LEZERUSD` with `alice` as LP-0013 `mint_authority`
    let (ok_create, create_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "create",
            "--name",
            "LEZERUSD",
            "--supply",
            "1000000",
            "--creator",
            "alice",
            "--authority",
            "alice",
        ],
    );
    assert!(ok_create, "token create failed: {create_json}");
    let def_id = create_json["definition_id"].as_str().unwrap().to_string();
    assert_eq!(create_json["symbol"], "LEZERUSD");
    assert_eq!(create_json["mint_authority"], alice_id);

    // 5. Derive and initialize Bob's ATA
    let (ok_ata_addr, ata_addr_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["token", "ata-address", "--owner", "bob", "--mint", &def_id],
    );
    assert!(ok_ata_addr);
    let bob_ata = ata_addr_json["ata_account_id"].as_str().unwrap().to_string();
    assert!(bob_ata.starts_with("Public/"));

    let (ok_ata_init, ata_init_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["token", "ata-create", "--owner", "bob", "--mint", "LEZERUSD"],
    );
    assert!(ok_ata_init);
    assert_eq!(ata_init_json["ata_account_id"], bob_ata);

    // 6. Transfer 250 LEZERUSD from `alice` to `bob`
    let (ok_send, send_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "send",
            "--from",
            "alice",
            "--to",
            "bob",
            "--mint",
            "LEZERUSD",
            "--amount",
            "250",
        ],
    );
    assert!(ok_send, "token send failed: {send_json}");
    assert_eq!(send_json["to_account"], bob_id);
    assert_eq!(send_json["ata_account_id"], bob_ata);

    // 7. Mint 500 additional LEZERUSD to `bob` signed by `alice` (LP-0013)
    let (ok_mint, mint_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "mint",
            "--mint",
            "LEZERUSD",
            "--to",
            "bob",
            "--amount",
            "500",
            "--authority",
            "alice",
        ],
    );
    assert!(ok_mint, "token mint failed: {mint_json}");

    // 8. Burn 50 LEZERUSD from `bob` (leaving Bob with 250 + 500 - 50 = 700 LEZERUSD)
    let (ok_burn, burn_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "burn",
            "--from",
            "bob",
            "--mint",
            "LEZERUSD",
            "--amount",
            "50",
        ],
    );
    assert!(ok_burn, "token burn failed: {burn_json}");

    // 9. Permanently revoke `mint_authority` (`--new-authority none`) and verify subsequent mints fail
    let (ok_revoke, revoke_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "set-authority",
            "--mint",
            "LEZERUSD",
            "--authority",
            "alice",
            "--new-authority",
            "none",
        ],
    );
    assert!(ok_revoke, "set-authority failed: {revoke_json}");
    assert!(revoke_json["new_authority"].is_null());

    let (ok_mint_after_revoke, mint_err_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &[
            "token",
            "mint",
            "--mint",
            "LEZERUSD",
            "--to",
            "bob",
            "--amount",
            "100",
            "--authority",
            "alice",
        ],
    );
    assert!(!ok_mint_after_revoke);
    assert_eq!(mint_err_json["error"]["code"], "LZR-3003");

    // 10. Check final token balances for `bob`
    let (ok_bals, bals_json) = run_lezer(
        &vault_path,
        &ledger_path,
        pw,
        &["token", "balances", "--owner", "bob"],
    );
    assert!(ok_bals);
    let first_tok = &bals_json["balances"][0];
    assert_eq!(first_tok["symbol"], "LEZERUSD");
    assert_eq!(first_tok["balance"], "700");
    assert_eq!(first_tok["total_supply"], "1000450");
    assert!(first_tok["mint_authority"].is_null());
}
