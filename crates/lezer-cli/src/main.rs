//! `lezer` — First-Class Command-Line Interface for Lezer Wallet.
//!
//! Implements fd-level `dup` / `dup2` stdout isolation (`docs/01` §2.1) when `--json`
//! is passed so diagnostic output from upstream libraries or `risc0-zkvm` is routed
//! to `stderr`, guaranteeing pure machine-readable JSON on `stdout`.

mod commands;

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use commands::account::AccountSubcommand;
use commands::faucet::FaucetSubcommand;
use commands::testimonial::TestimonialSubcommand;
use commands::token::TokenSubcommand;
use lezer_adapter::LezAdapter;
use lezer_core::{CANONICAL_SUBMISSION_ID, LezerError, WalletEngine, default_lezer_home};
use serde_json::{Value, json};

#[derive(Parser, Debug)]
#[command(
    name = "lezer",
    version,
    about = "Lezer LEZ Wallet CLI — Native Rust Wallet, Token/ATA Manager & On-Chain Verifier"
)]
struct Cli {
    /// Emit pure machine-readable JSON on stdout (isolates fd 1 via dup2).
    #[arg(long, global = true, default_value_t = false)]
    json: bool,

    /// Path to the encrypted vault file (`~/.lezer/vault.enc` by default).
    #[arg(long, global = true)]
    vault: Option<PathBuf>,

    /// Vault password (defaults to `LEZER_PASSWORD` env var or empty string in non-interactive mode).
    #[arg(long, global = true)]
    password: Option<String>,

    /// Network/zone profile selector (`lez-testnet-0.4`, `lez-localnet`, or sequencer URL).
    #[arg(long, global = true)]
    zone: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new 24-word BIP-39 mnemonic vault (`vault.enc`).
    Init,
    /// Restore an existing 24-word BIP-39 mnemonic into `vault.enc`.
    Restore {
        /// 24-word (or 12-word) BIP-39 mnemonic phrase.
        #[arg(long)]
        mnemonic: String,
    },
    /// Unlock `vault.enc` and verify password integrity.
    Unlock,
    /// Lock the vault and zeroize in-memory key material.
    Lock,
    /// Print canonical wallet state snapshot and network status.
    Status,
    /// Create, list, or inspect accounts in the vault.
    Account {
        #[command(subcommand)]
        sub: AccountSubcommand,
    },
    /// Query native LEZ balance for an account (supports Offline Mode via `cache.json`).
    Balance {
        /// Account label (e.g. `alice`) or `Public/<base58>` identifier.
        #[arg(long, default_value = "Main Public")]
        account: String,
    },
    /// Execute a native LEZ transfer (`lezer transfer --from alice --to bob --amount 50`).
    Transfer {
        /// Sender account label or `Public/<base58>` ID.
        #[arg(long)]
        from: String,
        /// Recipient account label or `Public/<base58>` ID.
        #[arg(long)]
        to: String,
        /// Amount of native LEZ to transfer.
        #[arg(long)]
        amount: u128,
    },
    /// Piñata Proof-of-Work Faucet commands (`info`, `claim`).
    Faucet {
        #[command(subcommand)]
        sub: FaucetSubcommand,
    },
    /// Fungible Token Program (`TOKEN_ID`) and Associated Token Account (`ATA`) commands.
    Token {
        #[command(subcommand)]
        sub: TokenSubcommand,
    },
    /// LP-0021 Sharded On-Chain Testimonial commands (`submit`, `list`).
    Testimonial {
        #[command(subcommand)]
        sub: TestimonialSubcommand,
    },
    /// Stateless 16-shard evaluator adoption verifier (`lezer verify-adoption --submission-id lp0021-lezer`).
    VerifyAdoption {
        /// Submission identifier to audit (defaults to `lp0021-lezer`).
        #[arg(long, default_value = CANONICAL_SUBMISSION_ID)]
        submission_id: String,
        /// Optional sequencer RPC URL override.
        #[arg(long)]
        sequencer: Option<String>,
    },
    /// Run Phase 0 pre-build live testnet spikes (S-01, S-02).
    Spikes,
}

/// Dedicated JSON output stream isolated from `stdout` via `dup` / `dup2(2, 1)`.
struct IsolatedJsonWriter {
    #[cfg(unix)]
    file: Option<std::fs::File>,
}

impl IsolatedJsonWriter {
    fn setup(isolate: bool) -> Self {
        #[cfg(unix)]
        {
            if isolate {
                use std::os::fd::FromRawFd;
                let dup_fd = unsafe { libc::dup(1) };
                if dup_fd >= 0 {
                    unsafe {
                        libc::dup2(2, 1);
                    }
                    let file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
                    return Self { file: Some(file) };
                }
            }
            Self { file: None }
        }
        #[cfg(not(unix))]
        {
            let _ = isolate;
            Self {}
        }
    }

    fn write_json(&mut self, value: &Value) {
        let formatted = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
        #[cfg(unix)]
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f, "{formatted}");
            let _ = f.flush();
            return;
        }
        println!("{formatted}");
    }
}

fn resolve_password(cli_password: Option<String>) -> String {
    cli_password
        .or_else(|| std::env::var("LEZER_PASSWORD").ok())
        .unwrap_or_default()
}

fn resolve_vault_path(cli_vault: Option<PathBuf>) -> PathBuf {
    cli_vault.unwrap_or_else(|| default_lezer_home().join("vault.enc"))
}

fn main() {
    let cli = Cli::parse();
    let mut out_writer = IsolatedJsonWriter::setup(cli.json);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let vault_path = resolve_vault_path(cli.vault);
    let password = resolve_password(cli.password);
    let zone = cli.zone;

    let result: Result<Value, LezerError> = rt.block_on(async move {
        if let Commands::Spikes = cli.command {
            let adapter = LezAdapter::from_env().map_err(|e| {
                LezerError::new(lezer_core::ErrorCode::SequencerUnreachable, e.to_string())
            })?;
            let s02 = adapter.run_spike_s02().await.map_err(|e| {
                LezerError::new(lezer_core::ErrorCode::SequencerUnreachable, e.to_string())
            })?;
            let s01 = adapter.run_spike_s01().await.map_err(|e| {
                LezerError::new(lezer_core::ErrorCode::SequencerUnreachable, e.to_string())
            })?;
            return Ok(json!({
                "spike_s01_private_proof_benchmark": s01,
                "spike_s02_sequencer_compatibility": s02,
            }));
        }

        let mut engine = WalletEngine::with_vault_and_zone(vault_path, zone.as_deref())?;
        match cli.command {
            Commands::Init => commands::vault::handle_init(&mut engine, &password),
            Commands::Restore { mnemonic } => {
                commands::vault::handle_restore(&mut engine, &mnemonic, &password)
            }
            Commands::Unlock => commands::vault::handle_unlock(&mut engine, &password),
            Commands::Lock => commands::vault::handle_lock(&mut engine),
            Commands::Status => commands::vault::handle_status(&engine),
            Commands::Account { sub } => {
                commands::account::handle_account_subcommand(&mut engine, &password, sub)
            }
            Commands::Balance { account } => {
                commands::account::handle_balance(&mut engine, &password, &account).await
            }
            Commands::Transfer { from, to, amount } => {
                commands::transfer::handle_transfer(&mut engine, &password, &from, &to, amount)
                    .await
            }
            Commands::Faucet { sub } => {
                commands::faucet::handle_faucet_subcommand(&mut engine, &password, sub).await
            }
            Commands::Token { sub } => {
                commands::token::handle_token_subcommand(&mut engine, &password, sub)
            }
            Commands::Testimonial { sub } => {
                commands::testimonial::handle_testimonial_subcommand(&mut engine, &password, sub)
            }
            Commands::VerifyAdoption {
                submission_id,
                sequencer,
            } => {
                let seq_url = sequencer
                    .unwrap_or_else(|| engine.profile().sequencer_url.clone());
                commands::verify_adoption::handle_verify_adoption(
                    engine.testimonial_ledger_path(),
                    &submission_id,
                    &seq_url,
                )
            }
            Commands::Spikes => unreachable!(),
        }
    });

    match result {
        Ok(val) => {
            out_writer.write_json(&val);
        }
        Err(err) => {
            let envelope = json!({
                "error": err.to_envelope()
            });
            out_writer.write_json(&envelope);
            std::process::exit(1);
        }
    }
}
