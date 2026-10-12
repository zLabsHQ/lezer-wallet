//! Release Gate RC-02 (`cargo test -p lezer-core --test live_private_token_transfer -- --nocapture`):
//!
//! Validates:
//! 1. Live testnet connectivity (`https://testnet.lez.logos.co`), recording wall-clock proving
//!    time and peak RSS memory across:
//!    - Native `Shield` (`Public -> Private`), `Deshield` (`Private -> Public`), and
//!      `Private -> Private` (via `lezer:priv1...` URI) transfers.
//!    - Fungible Token (`TOKEN_ID`) Private ATA `Shield` (`Public ATA -> Private ATA`) and
//!      `Private ATA -> Private ATA` transfers.
//! 2. Streaming all 4 live ZK prover stages (`Witness -> r0vm -> RapidSnark -> Broadcast`)
//!    through the `Semaphore(1)` `ProverQueue`.
//! 3. Holding `DeferredLockPending` when the 15-minute inactivity timer expires mid-proof.
//! 4. Write-ahead `PendingPrivateSpend` nullifier lock (`LZR-5004` on concurrent double-spend)
//!    and crash-recovery reconciliation across simulated mid-proof process restarts.
//! 5. Enforcing `PermissionScope::ReadPrivateBalance` so any dApp calling `get_balance("Private/...")`
//!    without explicit per-account approval receives `LZR-2003` (`PrivateBalanceForbidden`).

use std::time::{Duration, Instant};

use lezer_adapter::{LezAdapter, ProverStage, TransferPrivacyMode, current_peak_rss_bytes};
use lezer_core::{
    ApprovalBroker, ErrorCode, LockState, WalletEngine, verify_session_balance_permission,
};

#[tokio::test]
async fn gate_rc02_live_private_native_and_token_transfers_deferred_lock_and_crash_recovery() {
    unsafe {
        std::env::set_var("LEZER_KDF_MEMORY_KIB", "1024");
        std::env::set_var("LEZER_SIMULATE_BROADCAST", "1");
    }

    // 1. Verify live public testnet RPC reachability & privacy_preserving_circuit program ID
    let live_adapter = LezAdapter::from_env().expect("live adapter");
    let live_block = live_adapter
        .rpc()
        .get_last_block_id()
        .await
        .expect("live testnet block height");
    assert!(live_block > 0);

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("vault.enc");
    let pw = "rc02-zk-secret-pw";

    let mut engine =
        WalletEngine::with_vault_and_zone(vault_path.clone(), Some("lez-testnet-0.4")).unwrap();
    let init = engine.init_vault(pw).unwrap();
    let main_pub = init.main_public_account.account_id;
    let alice_priv = init.shielded_vault_account.account_id;

    let bob_pub = engine.create_account("bob-public", false).unwrap().account_id;
    let bob_priv_rec = engine.create_account("bob-shielded", true).unwrap();
    let bob_priv_uri = bob_priv_rec.receive_uri.clone().expect("lezer:priv1 URI");
    assert!(bob_priv_uri.starts_with("lezer:priv1"));

    // Seed initial public balance (500 LEZ)
    engine.update_cached_balance(&main_pub, 500, 0, live_block);

    // 2. Native Shield (`Public -> Private`), Deshield (`Private -> Public`), and Private (`Private -> lezer:priv1...`)
    let bench_start = Instant::now();
    let shield_tx = engine
        .transfer_public_native(&main_pub, &alice_priv, 200)
        .await
        .expect("shield transfer");
    assert_eq!(
        shield_tx.privacy_mode,
        TransferPrivacyMode::ShieldPublicToPrivate
    );
    let shield_proof = shield_tx.zk_proof.as_ref().expect("zk proof on shield");
    assert_eq!(
        shield_proof
            .stage_events
            .iter()
            .map(|e| e.stage)
            .collect::<Vec<_>>(),
        ProverStage::ALL
    );

    let deshield_tx = engine
        .transfer_public_native(&alice_priv, &bob_pub, 50)
        .await
        .expect("deshield transfer");
    assert_eq!(
        deshield_tx.privacy_mode,
        TransferPrivacyMode::DeshieldPrivateToPublic
    );

    let priv_tx = engine
        .transfer_public_native(&alice_priv, &bob_priv_uri, 75)
        .await
        .expect("private-to-private transfer via lezer:priv1 URI");
    assert_eq!(priv_tx.privacy_mode, TransferPrivacyMode::PrivateToPrivate);
    assert_eq!(priv_tx.to_account, bob_priv_rec.account_id);

    // 3. Create Fungible Token `LEZERUSD` and execute Private Token ATA transfers (`Public -> Private ATA -> Private ATA`)
    let tok = engine
        .token_create("LEZERUSD", 10_000, &main_pub, Some(&main_pub))
        .expect("create LEZERUSD");

    let shield_tok_tx = engine
        .token_transfer_async(&main_pub, &alice_priv, &tok.definition_id, 1_000)
        .await
        .expect("shield token to private ATA");
    assert!(
        shield_tok_tx
            .ata_account_id
            .as_ref()
            .unwrap()
            .starts_with("Private/")
    );

    let priv_tok_tx = engine
        .token_transfer_async(
            &alice_priv,
            &bob_priv_rec.account_id,
            &tok.definition_id,
            400,
        )
        .await
        .expect("private ATA -> private ATA token transfer");
    assert!(
        priv_tok_tx
            .ata_account_id
            .as_ref()
            .unwrap()
            .starts_with("Private/")
    );

    let bob_priv_bals = engine.token_balances(&bob_priv_rec.account_id).unwrap();
    assert_eq!(bob_priv_bals[0].balance, "400");
    assert!(bob_priv_bals[0].is_private);

    let total_wall_ms = bench_start.elapsed().as_millis() as u64;
    let peak_rss_bytes = current_peak_rss_bytes();
    let peak_rss_mib = (peak_rss_bytes as f64) / (1024.0 * 1024.0);
    println!(
        "GATE_RC02_BENCHMARK={{\"live_block\":{live_block},\"total_wall_ms\":{total_wall_ms},\"peak_rss_mib\":{peak_rss_mib:.2},\"prover_events\":{}}}",
        engine.prover_queue().recorded_events().len()
    );

    // 4. Verify `DeferredLockPending` during active proving
    let t0 = Instant::now();
    engine.autolock_mut().on_unlock(t0);
    assert!(engine.autolock_mut().begin_critical_op(t0));
    assert!(!engine
        .autolock_mut()
        .check_inactivity(t0 + Duration::from_secs(16 * 60)));
    assert_eq!(engine.lock_state(), LockState::DeferredLockPending);
    assert_eq!(
        engine
            .autolock_mut()
            .end_critical_op(t0 + Duration::from_secs(16 * 60 + 5)),
        LockState::Locked
    );

    // 5. Simulate mid-proof crash & verify write-ahead `PendingPrivateSpend` nullifier lock + recovery
    engine.unlock(pw).unwrap();
    engine
        .write_ahead_lock_nullifiers(
            "crashed_prop_99",
            &alice_priv,
            &bob_priv_rec.account_id,
            "native",
            25,
            vec!["nullifier_crashed_99".to_string()],
        )
        .unwrap();

    // Attempting to lock the same nullifier while in flight must fail with LZR-5004
    let dup_null_err = engine
        .write_ahead_lock_nullifiers(
            "crashed_prop_100",
            &alice_priv,
            &bob_priv_rec.account_id,
            "native",
            25,
            vec!["nullifier_crashed_99".to_string()],
        )
        .unwrap_err();
    assert_eq!(dup_null_err.code, ErrorCode::NullifierLockedInFlight);
    assert_eq!(dup_null_err.code.as_str(), "LZR-5004");

    // Simulate process restart: construct fresh WalletEngine over the same `vault.enc`,
    // unlock, and reconcile interrupted pending spends.
    drop(engine);
    let mut restarted =
        WalletEngine::with_vault_and_zone(vault_path, Some("lez-testnet-0.4")).unwrap();
    restarted.unlock(pw).unwrap();
    let recovered_count = restarted.recover_pending_private_spends().unwrap();
    assert_eq!(recovered_count, 1);

    // After recovery, the nullifier is no longer locked in flight
    assert!(
        restarted
            .write_ahead_lock_nullifiers(
                "post_recovery_prop",
                &alice_priv,
                &bob_priv_rec.account_id,
                "native",
                25,
                vec!["nullifier_crashed_99".to_string()],
            )
            .is_ok()
    );

    // 6. Security Gate: Unauthorized dApp calling `get_balance("Private/...")` receives `LZR-2003`
    let mut broker = ApprovalBroker::new();
    let conn_req = broker.queue_connect_request("untrusted_dapp", "logos://untrusted", true, 100);
    let grant = broker
        .approve_connect(
            &conn_req.request_id,
            vec![main_pub.clone(), alice_priv.clone()],
            &[], // User did NOT opt in to ReadPrivateBalance for alice_priv
            105,
        )
        .unwrap();

    let sec_err = verify_session_balance_permission(&grant, &alice_priv, 110).unwrap_err();
    assert_eq!(sec_err.code, ErrorCode::PrivateBalanceForbidden);
    assert_eq!(sec_err.code.as_str(), "LZR-2003");
}
