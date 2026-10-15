//! `tooling/gen-conformance-fixtures.rs` — Reference generator and validator for the
//! golden JSON vectors in `tests/conformance/fixtures/`.
//!
//! Can be verified via `cargo test -p lezer-core --test vault_conformance`.

pub const FIXTURE_FILES: [&str; 4] = [
    "tests/conformance/fixtures/vault_v1_vectors.json",
    "tests/conformance/fixtures/transaction_summaries.json",
    "tests/conformance/fixtures/error_taxonomy.json",
    "tests/conformance/fixtures/shard_routing_vectors.json",
];
