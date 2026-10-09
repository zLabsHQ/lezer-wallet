//! Fungible Token Program (`TOKEN_ID`) and Associated Token Account (`ASSOCIATED_TOKEN_ACCOUNT_ID`)
//! CLI commands (`lezer token create`, `ata-address`, `ata-create`, `send`, `mint`,
//! `set-authority`, `burn`, `balances`).

use clap::Subcommand;
use lezer_core::{LezerResult, WalletEngine};
use serde_json::{Value, json};

#[derive(Subcommand, Debug, Clone)]
pub enum TokenSubcommand {
    /// Create a new fungible token definition (`TOKEN_ID::NewFungibleDefinition`).
    Create {
        /// Token ticker symbol (e.g. `LEZERUSD`).
        #[arg(long)]
        name: String,
        /// Initial token supply in base units.
        #[arg(long)]
        supply: u128,
        /// Creator account label or `Public/<base58>` ID (defaults to `"Main Public"`).
        #[arg(long, default_value = "Main Public")]
        creator: String,
        /// Optional LP-0013 Mint Authority account label or `Public/<base58>` ID (`none` for fixed supply).
        #[arg(long)]
        authority: Option<String>,
    },
    /// Deterministically compute the Associated Token Account (ATA) address (`SHA256(owner || definition)`).
    AtaAddress {
        /// Owner account label or `Public/<base58>` / `Private/<base58>` ID.
        #[arg(long)]
        owner: String,
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
    },
    /// Idempotently initialize an Associated Token Account (`ASSOCIATED_TOKEN_ACCOUNT_ID::Create`).
    AtaCreate {
        /// Owner account label or ID.
        #[arg(long)]
        owner: String,
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
    },
    /// Transfer fungible tokens between accounts (auto-deriving ATAs).
    Send {
        /// Sender account label or ID.
        #[arg(long)]
        from: String,
        /// Recipient account label or ID.
        #[arg(long)]
        to: String,
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
        /// Amount to transfer.
        #[arg(long)]
        amount: u128,
    },
    /// Mint additional fungible supply (LP-0013), signed by the current `mint_authority`.
    Mint {
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
        /// Recipient account label or ID.
        #[arg(long)]
        to: String,
        /// Amount to mint.
        #[arg(long)]
        amount: u128,
        /// Signing `mint_authority` account label or ID.
        #[arg(long)]
        authority: String,
    },
    /// Rotate or permanently revoke (`none`) the LP-0013 `mint_authority`.
    SetAuthority {
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
        /// Current `mint_authority` account label or ID.
        #[arg(long)]
        authority: String,
        /// New authority account label/ID, or `none` to permanently lock supply.
        #[arg(long)]
        new_authority: String,
    },
    /// Burn fungible tokens from an account's ATA and decrement `total_supply`.
    Burn {
        /// Holder account label or ID.
        #[arg(long)]
        from: String,
        /// Token definition ID or symbol.
        #[arg(long)]
        mint: String,
        /// Amount to burn.
        #[arg(long)]
        amount: u128,
    },
    /// List all tracked fungible token balances and ATAs for an owner account.
    Balances {
        /// Owner account label or ID.
        #[arg(long, default_value = "Main Public")]
        owner: String,
    },
}

pub fn handle_token_subcommand(
    engine: &mut WalletEngine,
    password: &str,
    sub: TokenSubcommand,
) -> LezerResult<Value> {
    engine.unlock(password)?;
    match sub {
        TokenSubcommand::Create {
            name,
            supply,
            creator,
            authority,
        } => {
            let receipt =
                engine.token_create(&name, supply, &creator, authority.as_deref())?;
            Ok(serde_json::to_value(receipt).expect("TokenDefinitionReceipt serializes"))
        }
        TokenSubcommand::AtaAddress { owner, mint } => {
            let ata = engine.token_derive_ata(&owner, &mint)?;
            Ok(serde_json::to_value(ata).expect("DerivedAta serializes"))
        }
        TokenSubcommand::AtaCreate { owner, mint } => {
            let receipt = engine.token_create_ata(&owner, &mint)?;
            Ok(serde_json::to_value(receipt).expect("TokenOperationReceipt serializes"))
        }
        TokenSubcommand::Send {
            from,
            to,
            mint,
            amount,
        } => {
            let receipt = engine.token_transfer(&from, &to, &mint, amount)?;
            Ok(serde_json::to_value(receipt).expect("TokenOperationReceipt serializes"))
        }
        TokenSubcommand::Mint {
            mint,
            to,
            amount,
            authority,
        } => {
            let receipt = engine.token_mint(&mint, &to, &authority, amount)?;
            Ok(serde_json::to_value(receipt).expect("TokenOperationReceipt serializes"))
        }
        TokenSubcommand::SetAuthority {
            mint,
            authority,
            new_authority,
        } => {
            let receipt =
                engine.token_set_authority(&mint, &authority, Some(&new_authority))?;
            Ok(serde_json::to_value(receipt).expect("TokenOperationReceipt serializes"))
        }
        TokenSubcommand::Burn { from, mint, amount } => {
            let receipt = engine.token_burn(&from, &mint, amount)?;
            Ok(serde_json::to_value(receipt).expect("TokenOperationReceipt serializes"))
        }
        TokenSubcommand::Balances { owner } => {
            let items = engine.token_balances(&owner)?;
            Ok(json!({
                "owner": owner,
                "balances": items,
            }))
        }
    }
}
