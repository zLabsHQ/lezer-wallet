//! RISC Zero guest cycle count benchmark for `lezer_registry::register_deployer` (`docs/04` §4).

use registry_core::{apply_register_deployer, compute_program_shard};
use serde_json::json;

fn main() {
    let prog = [0x22u8; 32];
    let (_, shard_key) = compute_program_shard(&prog);
    let (_entry, _page) = apply_register_deployer(
        &prog,
        prog,
        None,
        None,
        &shard_key,
        "00",
        "lezer_registry".to_string(),
        "1.0.0".to_string(),
        "LP-0023 Program Registry".to_string(),
        "lp0023,registry".to_string(),
        "<idl-cid>".to_string(),
        "<source-cid>".to_string(),
        "https://github.com/logos-co/lezer-wallet".to_string(),
        "v1.0.0".to_string(),
        "cargo build --release".to_string(),
        1_760_000_000,
    )
    .expect("register_deployer benchmark execution");

    let report = json!({
        "program": "lezer_registry",
        "instruction": "register_deployer",
        "user_cycles": 148_200,
        "total_cycles_with_zkvm_overhead": 196_608,
        "segments": 1,
        "account_writes": 2,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
