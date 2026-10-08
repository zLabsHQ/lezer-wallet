//! `lezer` — First-Class Command-Line Interface for Lezer Wallet.

use anyhow::Result;
use clap::{Parser, Subcommand};
use lezer_adapter::LezAdapter;
use lezer_core::WalletEngine;

#[derive(Parser, Debug)]
#[command(
    name = "lezer",
    version,
    about = "Lezer LEZ Wallet CLI — Native Rust Wallet, Token/ATA Manager & On-Chain Verifier"
)]
struct Cli {
    /// Output machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Print wallet engine and network profile status.
    Status,
    /// Run Phase 0 pre-build live testnet spikes (S-01, S-02).
    Spikes,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => {
            let engine = WalletEngine::with_default_adapter()?;
            let snapshot = engine.snapshot();
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Commands::Spikes => {
            let adapter = LezAdapter::from_env()?;
            let s02 = adapter.run_spike_s02().await?;
            let s01 = adapter.run_spike_s01().await?;
            let combined = serde_json::json!({
                "spike_s01_private_proof_benchmark": s01,
                "spike_s02_sequencer_compatibility": s02,
            });
            println!("{}", serde_json::to_string_pretty(&combined)?);
        }
    }

    Ok(())
}
