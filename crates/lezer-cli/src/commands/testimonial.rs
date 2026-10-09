//! LP-0021 Testimonial CLI commands (`lezer testimonial submit`, `lezer testimonial list`).

use clap::Subcommand;
use lezer_core::{CANONICAL_SUBMISSION_ID, LezerResult, WalletEngine};
use serde_json::Value;

#[derive(Subcommand, Debug, Clone)]
pub enum TestimonialSubcommand {
    /// Submit an on-chain testimonial to the 16-shard `lezer_testimonial` program (`lp0021-lezer`).
    Submit {
        /// Signing public account label or `Public/<base58>` ID.
        #[arg(long)]
        author: String,
        /// Optional display handle (Discord/X/GitHub username, <= 64 bytes).
        #[arg(long, default_value = "")]
        username: String,
        /// Testimonial message (1..=2048 bytes).
        #[arg(long)]
        text: String,
        /// Optional Unix timestamp override (for deterministic multi-month test fixtures).
        #[arg(long)]
        timestamp: Option<u64>,
    },
    /// Enumerate all testimonials across the 16 on-chain shards.
    List {
        /// Submission ID filter (defaults to `lp0021-lezer`).
        #[arg(long, default_value = CANONICAL_SUBMISSION_ID)]
        submission_id: String,
    },
}

pub fn handle_testimonial_subcommand(
    engine: &mut WalletEngine,
    password: &str,
    sub: TestimonialSubcommand,
) -> LezerResult<Value> {
    match sub {
        TestimonialSubcommand::Submit {
            author,
            username,
            text,
            timestamp,
        } => {
            engine.unlock(password)?;
            let receipt = engine.submit_testimonial(&author, &username, &text, timestamp)?;
            Ok(serde_json::to_value(receipt).expect("TestimonialSubmitReceipt serializes"))
        }
        TestimonialSubcommand::List { submission_id } => {
            let report = engine.list_testimonials(&submission_id)?;
            Ok(serde_json::to_value(report).expect("AdoptionVerificationReport serializes"))
        }
    }
}
