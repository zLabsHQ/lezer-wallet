//! The error taxonomy is a cross-surface contract (CLI, Basecamp module, TS SDK).
//! This test pins `ErrorCode` to `tests/conformance/fixtures/error_taxonomy.json`.

use lezer_core::{ErrorCode, LezerError};

#[test]
fn error_codes_match_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/fixtures/error_taxonomy.json");
    let fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    for v in vectors {
        let code = ErrorCode::from_code_str(v["code"].as_str().unwrap())
            .unwrap_or_else(|| panic!("fixture code {} missing in ErrorCode", v["code"]));
        let env = serde_json::to_value(LezerError::new(code, "x").to_envelope()).unwrap();
        assert_eq!(env["name"], v["name"]);
        assert_eq!(env["category"], v["category"]);
        assert_eq!(env["retryable"], v["retryable"]);
        assert_eq!(serde_json::to_value(code).unwrap(), v["code"], "serde code mismatch");
    }
    assert_eq!(vectors.len(), 32, "update the fixture when adding error codes");
}
