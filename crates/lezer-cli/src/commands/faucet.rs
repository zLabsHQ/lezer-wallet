//! Piñata PoW Faucet CLI commands (`lezer faucet info`, `lezer faucet claim`).

use clap::Subcommand;
use lezer_core::{LezerResult, WalletEngine};
use serde_json::Value;

#[derive(Subcommand, Debug, Clone)]
pub enum FaucetSubcommand {
    /// Query live Piñata faucet pool depth, PoW difficulty, and claim eligibility.
    Info,
    /// Solve the live Piñata PoW challenge and claim 150 LEZ.
    Claim {
        /// Recipient public account label or `Public/<base58>` ID.
        #[arg(long, alias = "account")]
        to: String,
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
        FaucetSubcommand::Claim { to } => {
            engine.unlock(password)?;
            let receipt = engine.faucet_claim(&to).await?;
            Ok(serde_json::to_value(receipt).expect("FaucetClaimReceipt serializes"))
        }
    }
}
