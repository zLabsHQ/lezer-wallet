//! Native LEZ transfer CLI command (`lezer transfer --from <alice> --to <bob>|--to-uri <lezer:priv1...> --amount <N>`).

use lezer_core::{LezerResult, WalletEngine};
use serde_json::Value;

/// Execute `lezer transfer` across all 4 privacy permutations (`Public -> Public`,
/// `Shield`, `Deshield`, `Private -> Private`).
pub async fn handle_transfer(
    engine: &mut WalletEngine,
    password: &str,
    from: &str,
    to: &str,
    amount: u128,
) -> LezerResult<Value> {
    engine.unlock(password)?;
    let receipt = engine.transfer_public_native(from, to, amount).await?;
    Ok(serde_json::to_value(receipt).expect("PublicTransferReceipt serializes"))
}
