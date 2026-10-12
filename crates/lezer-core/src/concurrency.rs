//! Concurrency & Exclusivity Model (`crates/lezer-core/src/concurrency.rs`).
//!
//! Enforces `docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md` §4.4 & `docs/10` §4.2:
//! - `prover_semaphore` (`Semaphore::new(1)`): Strictly 1 active RISC0/RapidSnark proof at a time
//!   to prevent multi-GB RAM spikes. Additional private transactions queue in FIFO order while
//!   public reads and transfers remain unblocked.
//! - Emits 4-stage progress updates (`Witness -> r0vm -> RapidSnark -> Broadcast`) and updates
//!   `ProverQueueStatusView`.

use std::sync::{Arc, Mutex};

use lezer_adapter::{ProverStageEvent, ZkProofArtifact, generate_zk_proof};
use tokio::sync::Semaphore;

use crate::state::ProverQueueStatusView;

/// Single-concurrency (`Semaphore::new(1)`) ZK Prover Queue with live stage tracking.
#[derive(Debug, Clone)]
pub struct ProverQueue {
    semaphore: Arc<Semaphore>,
    status: Arc<Mutex<ProverQueueStatusView>>,
    recorded_events: Arc<Mutex<Vec<ProverStageEvent>>>,
}

impl Default for ProverQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ProverQueue {
    /// Construct a new single-concurrency (`Semaphore::new(1)`) prover queue.
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            status: Arc::new(Mutex::new(ProverQueueStatusView::Idle)),
            recorded_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Current live status of the prover queue (`Idle` or `Proving { proposal_id, stage, elapsed_ms }`).
    pub fn current_status(&self) -> ProverQueueStatusView {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or(ProverQueueStatusView::Idle)
    }

    /// Return all 4-stage prover events emitted so far.
    pub fn recorded_events(&self) -> Vec<ProverStageEvent> {
        self.recorded_events
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Number of available permits (`1` when idle, `0` when a proof is actively running).
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Acquire the exclusive `Semaphore(1)` permit, execute the 4-stage RISC0 + RapidSnark
    /// proof (`Witness -> r0vm -> RapidSnark -> Broadcast`), stream stage events into
    /// `ProverQueueStatusView`, and release the permit when complete.
    pub async fn run_exclusive_proof(
        &self,
        proposal_id: &str,
        operation_kind: &str,
        witness_seed: &[u8],
        nullifiers: Vec<String>,
        output_commitments: Vec<String>,
        view_tags: Vec<u32>,
    ) -> anyhow::Result<ZkProofArtifact> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("prover semaphore closed: {e}"))?;

        let status_ref = Arc::clone(&self.status);
        let events_ref = Arc::clone(&self.recorded_events);
        let prop_id = proposal_id.to_string();

        let artifact = generate_zk_proof(
            proposal_id,
            operation_kind,
            witness_seed,
            nullifiers,
            output_commitments,
            view_tags,
            |ev| {
                if let Ok(mut s) = status_ref.lock() {
                    *s = ProverQueueStatusView::Proving {
                        proposal_id: prop_id.clone(),
                        stage: ev.stage.as_str().to_string(),
                        elapsed_ms: ev.elapsed_ms,
                    };
                }
                if let Ok(mut list) = events_ref.lock() {
                    list.push(ev);
                }
            },
        )?;

        if let Ok(mut s) = self.status.lock() {
            *s = ProverQueueStatusView::Idle;
        }

        Ok(artifact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn semaphore_serializes_concurrent_proofs_and_streams_four_stages() {
        let queue = ProverQueue::new();
        assert_eq!(queue.available_permits(), 1);

        let q1 = queue.clone();
        let q2 = queue.clone();

        let (r1, r2) = tokio::join!(
            q1.run_exclusive_proof(
                "prop_a",
                "shield_native",
                b"seed_a",
                vec!["null_a".to_string()],
                vec!["comm_a".to_string()],
                vec![1],
            ),
            q2.run_exclusive_proof(
                "prop_b",
                "private_token_transfer",
                b"seed_b",
                vec!["null_b".to_string()],
                vec!["comm_b".to_string()],
                vec![2],
            )
        );

        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert_eq!(queue.current_status(), ProverQueueStatusView::Idle);
        // 2 proofs x 4 stages = 8 stage events
        assert_eq!(queue.recorded_events().len(), 8);
    }
}
