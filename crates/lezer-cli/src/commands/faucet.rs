//! Piñata PoW Faucet CLI commands (`lezer faucet info`, `lezer faucet claim`).

use clap::Subcommand;
use lezer_core::{LezerResult, WalletEngine};
use serde_json::Value;

#[derive(Subcommand, Debug, Clone)]
pub enum FaucetSubcommand {
    /// Query live Piñata faucet pool depth, PoW difficulty, and claim eligibility.
    Info,
    /// Solve the live Piñata PoW challenge and claim 150 LEZ (with optional auto-shield into a Private/<id> account).
    Claim {
        /// Recipient public account label or `Public/<base58>` ID.
        #[arg(long, alias = "account")]
        to: String,
        /// Optional private account label or `Private/<base58>` / `lezer:priv1...` URI to auto-shield the claimed 150 LEZ into.
        #[arg(long)]
        auto_shield_to: Option<String>,
    },
}

pub async fn handle_faucet_subcommand(
    engine: &mut WalletEngine,
    password: &str,
    sub: FaucetSubcommand,
) -> LezerResult<Value> {
    match sub {
        FaucetSubcommand::Info => {
            let info = engine.faucet_info().await?;
            Ok(serde_json::to_value(info).expect("FaucetInfoView serializes"))
        }
        FaucetSubcommand::Claim { to, auto_shield_to } => {
            engine.unlock(password)?;
            let receipt = engine.faucet_claim(&to).await?;
            if let Some(shield_dest) = auto_shield_to {
                let amount = receipt.amount_claimed.parse::<u128>().unwrap_or(150);
                let shield_receipt = engine.transfer_public_native(&to, &shield_dest, amount).await?;
                return Ok(serde_json::json!({
                    "claim": receipt,
                    "auto_shield": shield_receipt,
                }));
            }
            Ok(serde_json::to_value(receipt).expect("FaucetClaimReceipt serializes"))
        }
    }
}
