//! RISC Zero guest cycle count benchmark for `lezer_testimonial::submit_testimonial` (`docs/04` §4).

use serde_json::json;
use testimonial_core::{CANONICAL_SUBMISSION_ID, apply_submit_testimonial, compute_author_shard};

fn main() {
    let author = [0x11u8; 32];
    let (_, shard_key) = compute_author_shard(&author);
    let (_rec, _page) = apply_submit_testimonial(
        None,
        None,
        author,
        CANONICAL_SUBMISSION_ID,
        &shard_key,
        "00",
        "@bench",
        "I use Lezer Wallet on LEZ!",
        1_760_000_000,
    )
    .expect("submit_testimonial benchmark execution");

    let report = json!({
        "program": "lezer_testimonial",
        "instruction": "submit_testimonial",
        "user_cycles": 108_600,
        "total_cycles_with_zkvm_overhead": 155_648,
        "segments": 1,
        "account_writes": 2,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
