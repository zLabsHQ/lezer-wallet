//! Cross-Surface Conformance Runner (`tests/conformance/runner.rs`).
//!
//! Asserts that `lezer-core`, `lezer-cli` (`--json`), and `lezer_wallet_provider` (LIDL C-ABI)
//! produce byte-identical `TransactionSummary` digests and `LZR-xxxx` error codes across
//! all 4 golden fixture files in `tests/conformance/fixtures/`.

pub const CONFORMANCE_SUITES: [&str; 4] = [
    "vault_v1_vectors.json",
    "transaction_summaries.json",
    "error_taxonomy.json",
    "shard_routing_vectors.json",
];
