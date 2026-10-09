//! Stateless Evaluator Adoption Verifier (`lezer verify-adoption --submission-id lp0021-lezer`).
//!
//! Runs with zero local wallet vault (`LEZER_NO_VAULT=1`), enumerating all 16 SHA-256
//! shard PDAs (`"00"`..`"0f"`) and their paginated overflow pages (`"00"`, `"01"`, ...)
//! directly from the sequencer / ledger state.

use std::path::Path;

use lezer_core::{LezerResult, verify_adoption_from_ledger};
use serde_json::Value;

pub fn handle_verify_adoption(
    ledger_path: &Path,
    submission_id: &str,
    sequencer_url: &str,
) -> LezerResult<Value> {
    let report = verify_adoption_from_ledger(ledger_path, submission_id, sequencer_url)?;
    Ok(serde_json::to_value(report).expect("AdoptionVerificationReport serializes"))
}
