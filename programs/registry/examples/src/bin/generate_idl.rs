//! Generates and prints the canonical SPEL JSON IDL for `lezer_registry`.

use lezer_registry_methods::LEZER_REGISTRY_PROGRAM_ID_HEX;
use serde_json::json;

fn main() {
    let idl = json!({
        "name": "lezer_registry",
        "version": "1.0.0",
        "program_id_hex": LEZER_REGISTRY_PROGRAM_ID_HEX,
        "instructions": ["register_deployer", "update_deployer", "register_third_party", "register_self"],
    });
    println!("{}", serde_json::to_string_pretty(&idl).unwrap());
}
