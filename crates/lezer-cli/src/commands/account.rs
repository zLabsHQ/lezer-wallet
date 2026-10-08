//! Account management and balance CLI commands (`lezer account new`, `list`, `receive`, `lezer balance`).

use clap::Subcommand;
use lezer_core::{LezerResult, WalletEngine};
use serde_json::{Value, json};

#[derive(Subcommand, Debug, Clone)]
pub enum AccountSubcommand {
    /// Create a new labeled public or private account in the vault.
    New {
        /// Human-readable label for the account (e.g. `alice`, `trading`).
        #[arg(long)]
        label: String,
        /// Derive a private/shielded (`Private/<base58>`) account instead of public (`Public/<base58>`).
        #[arg(long, default_value_t = false)]
        private: bool,
    },
    /// List all accounts stored in the vault.
    List,
    /// Display the receive address or `lezer:priv1...` URI for an account.
    Receive {
        /// Account label or ID (defaults to `"Main Public"`).
        #[arg(long, default_value = "Main Public")]
        account: String,
    },
}

/// Dispatch `lezer account <subcommand>`.
pub fn handle_account_subcommand(
    engine: &mut WalletEngine,
    password: &str,
    sub: AccountSubcommand,
) -> LezerResult<Value> {
    engine.unlock(password)?;
    match sub {
        AccountSubcommand::New { label, private } => {
            let record = engine.create_account(&label, private)?;
            Ok(json!({
                "status": "account_created",
                "account": record,
            }))
        }
        AccountSubcommand::List => {
            let accounts = engine.list_accounts()?;
            Ok(json!({
                "accounts": accounts,
            }))
        }
        AccountSubcommand::Receive { account } => {
            let record = engine.resolve_account_or_address(&account)?;
            Ok(json!({
                "account_id": record.account_id,
                "label": record.label,
                "privacy": record.privacy,
                "receive_uri": record.receive_uri.unwrap_or_else(|| record.account_id.clone()),
            }))
        }
    }
}

/// Execute `lezer balance` (supports both Online sequencer query and Offline `cache.json` fallback).
pub async fn handle_balance(
    engine: &mut WalletEngine,
    password: &str,
    account: &str,
) -> LezerResult<Value> {
    engine.unlock(password)?;
    let res = engine.query_balance(account).await?;
    Ok(serde_json::to_value(res).expect("BalanceQueryResult serializes"))
}
