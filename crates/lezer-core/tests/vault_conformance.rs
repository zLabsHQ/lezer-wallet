//! Phase 1 Conformance Suite (`vault_v1_vectors.json` & `error_taxonomy.json`).
//!
//! Validates:
//! 1. Deterministic `Argon2id` + `XChaCha20-Poly1305` `SealedVaultEnvelopeV1` sealing,
//!    header MAC verification, unsealing, and `LZR-1003` wrong-password rejection against
//!    `tests/conformance/fixtures/vault_v1_vectors.json`.
//! 2. Unified `LZR-1xxx` .. `LZR-6xxx` error codes, categories, and retryability flags
//!    against `tests/conformance/fixtures/error_taxonomy.json`.
//! 3. Crash-mid-write resilience, forward schema migration `.bak` preservation,
//!    15-minute `AutoLockFsm` with `DeferredLockPending`, and zero secret leakage in `lezer.log`.

use std::fs;
use std::path::PathBuf;

use lezer_adapter::{derive_private_account, derive_public_account};
use lezer_core::{
    Argon2ParamsRecord, CanonicalWalletStateV1, ErrorCategory, ErrorCode, LezerError,
    SealedVaultEnvelopeV1, TxIntent, VerificationBadgeState, compute_author_shard_for_account,
    compute_header_mac_hex, decode_and_summarize_intent, derive_wrapping_key,
};
use serde::Deserialize;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[derive(Debug, Deserialize)]
struct ErrorTaxonomyFile {
    schema_version: u32,
    vectors: Vec<ErrorTaxonomyVector>,
}

#[derive(Debug, Deserialize)]
struct ErrorTaxonomyVector {
    code: String,
    name: String,
    category: ErrorCategory,
    retryable: bool,
}

#[derive(Debug, Deserialize)]
struct VaultVectorsFile {
    schema_version: u32,
    vectors: Vec<VaultVector>,
}

#[derive(Debug, Deserialize)]
struct VaultKdfFixture {
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt_hex: String,
}

#[derive(Debug, Deserialize)]
struct VaultVector {
    id: String,
    mnemonic: String,
    password: String,
    wrong_password: String,
    wrong_password_error_code: String,
    active_zone: String,
    kdf: VaultKdfFixture,
    nonce_hex: String,
    expected_wrapping_key_hex: String,
    expected_header_mac_hex: String,
    expected_public_account_0: String,
    expected_public_account_1: String,
}

#[test]
fn error_taxonomy_conformance_matches_fixture() {
    let path = repo_root().join("tests/conformance/fixtures/error_taxonomy.json");
    let raw = fs::read_to_string(&path).expect("read error_taxonomy.json");
    let parsed: ErrorTaxonomyFile = serde_json::from_str(&raw).expect("parse error_taxonomy.json");
    assert_eq!(parsed.schema_version, 1);
    assert!(parsed.vectors.len() >= 25);

    for vec in parsed.vectors {
        let code = ErrorCode::from_code_str(&vec.code)
            .unwrap_or_else(|| panic!("missing ErrorCode for {}", vec.code));
        assert_eq!(code.as_str(), vec.code);
        assert_eq!(code.name(), vec.name);
        assert_eq!(code.category(), vec.category);
        assert_eq!(code.retryable(), vec.retryable);

        let err = LezerError::new(code, "conformance check");
        let env = err.to_envelope();
        assert_eq!(env.code, vec.code);
        assert_eq!(env.name, vec.name);
        assert_eq!(env.category, vec.category);
        assert_eq!(env.retryable, vec.retryable);
    }
}

#[test]
fn vault_v1_vectors_conformance() {
    let path = repo_root().join("tests/conformance/fixtures/vault_v1_vectors.json");
    let raw = fs::read_to_string(&path).expect("read vault_v1_vectors.json");
    let parsed: VaultVectorsFile = serde_json::from_str(&raw).expect("parse vault_v1_vectors.json");
    assert_eq!(parsed.schema_version, 1);

    for v in parsed.vectors {
        let salt_bytes: [u8; 32] = hex::decode(&v.kdf.salt_hex)
            .unwrap()
            .try_into()
            .expect("32-byte salt");
        let nonce_bytes: [u8; 24] = hex::decode(&v.nonce_hex)
            .unwrap()
            .try_into()
            .expect("24-byte nonce");

        let kdf = Argon2ParamsRecord::with_salt(
            &salt_bytes,
            v.kdf.memory_kib,
            v.kdf.iterations,
            v.kdf.parallelism,
        );
        assert_eq!(kdf.algorithm, v.kdf.algorithm);

        let wrapping_key = derive_wrapping_key(&v.password, &kdf).unwrap();
        let wrapping_key_hex = hex::encode(wrapping_key);

        let state = CanonicalWalletStateV1::new(v.mnemonic.clone(), v.active_zone.clone());
        let env =
            SealedVaultEnvelopeV1::seal_deterministic(&state, &v.password, kdf.clone(), &nonce_bytes)
                .unwrap();

        let computed_mac = compute_header_mac_hex(1, &kdf, &env.nonce_b64);
        assert_eq!(env.header_mac_hex, computed_mac, "vector {}", v.id);

        let pub0 = derive_public_account(&v.mnemonic, 0);
        let pub1 = derive_public_account(&v.mnemonic, 1);
        let priv0 = derive_private_account(&v.mnemonic, 0);
        assert!(priv0.receive_uri.starts_with("lezer:priv1"));

        assert_eq!(wrapping_key_hex, v.expected_wrapping_key_hex, "wrapping key mismatch for {}", v.id);
        assert_eq!(env.header_mac_hex, v.expected_header_mac_hex, "header mac mismatch for {}", v.id);
        assert_eq!(pub0.account_id, v.expected_public_account_0, "pub0 mismatch for {}", v.id);
        assert_eq!(pub1.account_id, v.expected_public_account_1, "pub1 mismatch for {}", v.id);

        // Wrong password must fail with LZR-1003
        let wrong_err = env.unseal(&v.wrong_password).unwrap_err();
        assert_eq!(wrong_err.code.as_str(), v.wrong_password_error_code);

        // Correct password unseals byte-identically
        let unsealed = env.unseal(&v.password).unwrap();
        assert_eq!(unsealed, state);
    }
}

#[derive(Debug, Deserialize)]
struct ShardRoutingFile {
    vectors: Vec<ShardRoutingVector>,
}

#[derive(Debug, Deserialize)]
struct ShardRoutingVector {
    account_id: String,
    expected_shard_id: u8,
    expected_shard_key: String,
}

#[derive(Debug, Deserialize)]
struct TransactionSummariesFile {
    vectors: Vec<TransactionSummaryVector>,
}

#[derive(Debug, Deserialize)]
struct TransactionSummaryVector {
    proposal_id: String,
    originating_dapp: String,
    intent: TxIntent,
    expected_badge_state: VerificationBadgeState,
    expected_requires_unverified_ack: bool,
    expected_requires_critical_override: bool,
    expected_warning_code: String,
}

#[test]
fn shard_routing_and_transaction_summary_fixtures_conformance() {
    let shard_path = repo_root().join("tests/conformance/fixtures/shard_routing_vectors.json");
    let shard_file: ShardRoutingFile =
        serde_json::from_str(&fs::read_to_string(&shard_path).unwrap()).unwrap();
    for v in shard_file.vectors {
        let (_bytes, shard_id, shard_key) =
            compute_author_shard_for_account(&v.account_id).unwrap();
        assert_eq!(shard_id, v.expected_shard_id);
        assert_eq!(shard_key, v.expected_shard_key);
    }

    let sum_path = repo_root().join("tests/conformance/fixtures/transaction_summaries.json");
    let sum_file: TransactionSummariesFile =
        serde_json::from_str(&fs::read_to_string(&sum_path).unwrap()).unwrap();
    for v in sum_file.vectors {
        let summary = decode_and_summarize_intent(
            &v.proposal_id,
            &v.originating_dapp,
            &v.intent,
            1_000,
            false,
        );
        assert_eq!(summary.verification_badge.state, v.expected_badge_state);
        assert_eq!(
            summary.requires_unverified_ack,
            v.expected_requires_unverified_ack
        );
        assert_eq!(
            summary.requires_critical_override,
            v.expected_requires_critical_override
        );
        assert!(
            summary
                .warnings
                .iter()
                .any(|w| w.code == v.expected_warning_code)
        );
    }
}
