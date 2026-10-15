//! Generates and prints the canonical SPEL JSON IDL for `lezer_testimonial`.

use lezer_testimonial_methods::{CANONICAL_SUBMISSION_ID, LEZER_TESTIMONIAL_PROGRAM_ID_HEX};
use serde_json::json;

fn main() {
    let idl = json!({
        "name": "lezer_testimonial",
        "version": "1.0.0",
        "program_id_hex": LEZER_TESTIMONIAL_PROGRAM_ID_HEX,
        "submission_id": CANONICAL_SUBMISSION_ID,
        "instructions": ["submit_testimonial", "register_self"],
    });
    println!("{}", serde_json::to_string_pretty(&idl).unwrap());
}
