//! Standalone Sequencer Full Lifecycle E2E Suite (`tests/e2e/standalone_sequencer_e2e.rs`).
//!
//! Covers: `init -> account new -> faucet claim -> public native transfer -> token create ->
//! ATA create/transfer -> shield/deshield/private transfer -> LP-0023 registry verify ->
//! Logos Storage backup push/pull -> stateless 16-shard verify-adoption`.

pub const STANDALONE_SEQUENCER_DEFAULT_URL: &str = "http://127.0.0.1:3040";
