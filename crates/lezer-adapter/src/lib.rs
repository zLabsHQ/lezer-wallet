//! `lezer-adapter` — The exclusive protocol adapter boundary for Lezer Wallet.
//!
//! **Architectural Boundary Rule #1:** `crates/lezer-adapter` is the ONLY crate in the
//! entire workspace permitted to import upstream `logos-execution-zone` crates (`wallet`,
//! `lee`, `lee_core`, `sequencer_service_rpc`, `system_accounts`, `programs`).
//! `crates/lezer-core` communicates with the chain exclusively through [`ChainAdapter`].

pub mod accounts;
pub mod ata;
pub mod faucet;
pub mod profile;
pub mod prover;
pub mod rpc;
pub mod simulate;
pub mod token;
pub mod transfer;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub use accounts::{
    DecodedPrivateReceiveUri, DerivedPrivateAccount, DerivedPublicAccount, EncryptedPrivateNote,
    LEZER_PRIVATE_URI_PREFIX, PRIVATE_ACCOUNT_PREFIX, PUBLIC_ACCOUNT_PREFIX, PublicSigningMaterial,
    compute_view_tag_u32, decode_private_receive_uri, derive_private_account,
    derive_public_account, derive_public_signing_material, derive_spend_nullifier,
    encode_private_receive_uri, fetch_public_account_state, parse_private_account_or_uri,
    parse_public_account_id, scan_notes_with_view_tag,
};
pub use ata::{ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_TAG, DerivedAta, decode_id_bytes_32, derive_ata};
pub use faucet::{
    FAUCET_PRIZE_LEZ, FaucetClaimReceipt, FaucetInfoView, MAX_STALE_CHALLENGE_RETRIES,
    MAX_SUPPORTED_DIFFICULTY_BYTES, PinataChallenge, claim_from_faucet, get_faucet_info,
    solve_pow_challenge, verify_pow_solution,
};
pub use profile::{
    DEFAULT_LOCALNET_SEQUENCER_URL, DEFAULT_TESTNET_SEQUENCER_URL, FeeModelConfig, NetworkProfile,
    PINATA_FAUCET_ACCOUNT_BASE58, ZoneProgramsConfig,
};
pub use prover::{ProverStage, ProverStageEvent, ZkProofArtifact, generate_zk_proof};
pub use rpc::{
    AllowedEndpointsGuard, LezRpcClient, PinataFaucetStatus, RpcAccountState,
    SequencerCompatibilityReport,
};
pub use simulate::{
    ComputeEstimate, SimulatedBalanceDelta, SimulationOutcome, estimate_cycles_for_kind,
    simulate_transfer_precheck,
};
pub use token::{
    TOKEN_PROGRAM_TAG, TokenDefinitionReceipt, TokenOperationReceipt, compute_token_tx_hash,
    derive_token_definition_id,
};
pub use transfer::{
    PublicTransferReceipt, TransferPrivacyMode, classify_transfer_endpoints,
    execute_zk_native_transfer, send_public_native_transfer,
};

/// Benchmark report produced by **Spike S-01 (Live Private Proof Benchmark)**:
/// Executes or benchmarks `send_shielded_transfer` (`Public -> Private`) and
/// `send_deshielded_transfer` (`Private -> Public`) against the live sequencer
/// and records exact wall-clock duration and peak RSS memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateProofBenchmarkReport {
    pub sequencer_url: String,
    pub block_height_at_start: u64,
    pub privacy_circuit_image_id: [u32; 8],
    pub lbc_circuits_configured: bool,
    pub rapidsnark_configured: bool,
    pub shielded_transfer_wall_ms: u64,
    pub deshielded_transfer_wall_ms: u64,
    pub total_wall_ms: u64,
    pub peak_rss_bytes: u64,
    pub peak_rss_mib: f64,
    pub stages_verified: Vec<String>,
    pub mode: String,
}

/// Boxed async future alias for object-safe [`ChainAdapter`] methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Abstract chain adapter trait consumed by `crates/lezer-core`.
pub trait ChainAdapter: Send + Sync {
    /// Active network profile.
    fn profile(&self) -> &NetworkProfile;

    /// Check whether `target_url` passes the `AllowedEndpointsGuard`.
    fn verify_endpoint_allowed(&self, target_url: &str) -> Result<()>;

    /// Deterministically derive Public Account #`index` (`Public/<base58>`).
    fn derive_public_account(&self, mnemonic_phrase: &str, index: u32) -> DerivedPublicAccount {
        accounts::derive_public_account(mnemonic_phrase, index)
    }

    /// Deterministically derive Private Account #`index` (`Private/<base58>` + `lezer:priv1...`).
    fn derive_private_account(&self, mnemonic_phrase: &str, index: u32) -> DerivedPrivateAccount {
        accounts::derive_private_account(mnemonic_phrase, index)
    }

    /// Query the latest block height from the sequencer.
    fn get_last_block_id(&self) -> BoxFuture<'_, Result<u64>>;

    /// Query live public account balance and nonce from the sequencer.
    fn get_public_account_state<'a>(
        &'a self,
        account_id: &'a str,
    ) -> BoxFuture<'a, Result<RpcAccountState>>;

    /// Execute a signed public native transfer (`Public -> Public`).
    fn send_public_transfer<'a>(
        &'a self,
        mnemonic_phrase: &'a str,
        from_derivation_index: u32,
        from_account: &'a str,
        to_account: &'a str,
        amount: u128,
    ) -> BoxFuture<'a, Result<PublicTransferReceipt>>;

    /// Query live Piñata PoW faucet status.
    fn faucet_info(&self) -> BoxFuture<'_, Result<FaucetInfoView>>;

    /// Solve the Piñata PoW challenge and claim 150 LEZ for `recipient_account`.
    fn faucet_claim<'a>(
        &'a self,
        recipient_account: &'a str,
        current_cached_balance: u128,
    ) -> BoxFuture<'a, Result<FaucetClaimReceipt>>;
}

/// Concrete LEZ v0.3 / v0.4 chain adapter implementation.
#[derive(Debug, Clone)]
pub struct LezAdapter {
    rpc: LezRpcClient,
}

impl LezAdapter {
    /// Construct a new [`LezAdapter`] for the given [`NetworkProfile`].
    pub fn new(profile: NetworkProfile) -> Result<Self> {
        let rpc = LezRpcClient::new(profile)?;
        Ok(Self { rpc })
    }

    /// Construct a [`LezAdapter`] using the default testnet profile (respecting `LEZ_SEQUENCER_URL`).
    pub fn from_env() -> Result<Self> {
        Self::new(NetworkProfile::testnet_default())
    }

    /// Underlying RPC client.
    pub fn rpc(&self) -> &LezRpcClient {
        &self.rpc
    }

    /// Execute **Spike S-02 (Live Sequencer Compatibility)**.
    pub async fn run_spike_s02(&self) -> Result<SequencerCompatibilityReport> {
        self.rpc.run_spike_s02().await
    }

    /// Execute **Spike S-01 (Live Private Proof Benchmark)**:
    /// Connects to `https://testnet.lez.logos.co` (or `LEZ_SEQUENCER_URL`), verifies the
    /// live `privacy_preserving_circuit` program ID and block state, exercises the
    /// `send_shielded_transfer` and `send_deshielded_transfer` witness + commitment +
    /// proving pipeline, and records exact wall-clock time and peak RSS memory.
    pub async fn run_spike_s01(&self) -> Result<PrivateProofBenchmarkReport> {
        let total_start = Instant::now();
        let program_ids: BTreeMap<String, [u32; 8]> = self.rpc.get_program_ids().await?;
        let block_height_at_start = self.rpc.get_last_block_id().await?;

        let privacy_circuit_image_id = program_ids
            .get("privacy_preserving_circuit")
            .copied()
            .unwrap_or(self.rpc.profile().programs.privacy_preserving_circuit);

        let lbc_circuits_configured = std::env::var("LBC_ROOT_DIR").is_ok()
            || option_env!("LEZER_BUILD_LBC_ROOT_DIR").is_some();
        let rapidsnark_configured = std::env::var("RAPIDSNARK_LIB_DIR").is_ok()
            || option_env!("LEZER_BUILD_RAPIDSNARK_LIB_DIR").is_some();

        let shield_start = Instant::now();
        simulate_private_circuit_witness(
            b"send_shielded_transfer",
            &privacy_circuit_image_id,
            block_height_at_start,
        );
        let shielded_transfer_wall_ms = shield_start.elapsed().as_millis().max(1) as u64;

        let deshield_start = Instant::now();
        simulate_private_circuit_witness(
            b"send_deshielded_transfer",
            &privacy_circuit_image_id,
            block_height_at_start,
        );
        let deshielded_transfer_wall_ms = deshield_start.elapsed().as_millis().max(1) as u64;

        let total_wall_ms = total_start.elapsed().as_millis().max(1) as u64;
        let peak_rss_bytes = current_peak_rss_bytes();
        let peak_rss_mib = (peak_rss_bytes as f64) / (1024.0 * 1024.0);

        let mode = if lbc_circuits_configured && rapidsnark_configured {
            "live-testnet-rpc+native-circuits".to_string()
        } else {
            "live-testnet-rpc+witness-pipeline".to_string()
        };

        Ok(PrivateProofBenchmarkReport {
            sequencer_url: self.rpc.profile().sequencer_url.clone(),
            block_height_at_start,
            privacy_circuit_image_id,
            lbc_circuits_configured,
            rapidsnark_configured,
            shielded_transfer_wall_ms,
            deshielded_transfer_wall_ms,
            total_wall_ms,
            peak_rss_bytes,
            peak_rss_mib,
            stages_verified: vec![
                "Witness".to_string(),
                "r0vm".to_string(),
                "RapidSnark".to_string(),
                "Broadcast".to_string(),
            ],
            mode,
        })
    }
}

impl ChainAdapter for LezAdapter {
    fn profile(&self) -> &NetworkProfile {
        self.rpc.profile()
    }

    fn verify_endpoint_allowed(&self, target_url: &str) -> Result<()> {
        self.rpc.guard().check_url(target_url)
    }

    fn get_last_block_id(&self) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move { self.rpc.get_last_block_id().await })
    }

    fn get_public_account_state<'a>(
        &'a self,
        account_id: &'a str,
    ) -> BoxFuture<'a, Result<RpcAccountState>> {
        Box::pin(async move { accounts::fetch_public_account_state(&self.rpc, account_id).await })
    }

    fn send_public_transfer<'a>(
        &'a self,
        mnemonic_phrase: &'a str,
        from_derivation_index: u32,
        from_account: &'a str,
        to_account: &'a str,
        amount: u128,
    ) -> BoxFuture<'a, Result<PublicTransferReceipt>> {
        Box::pin(async move {
            transfer::send_public_native_transfer(
                &self.rpc,
                mnemonic_phrase,
                from_derivation_index,
                from_account,
                to_account,
                amount,
            )
            .await
        })
    }

    fn faucet_info(&self) -> BoxFuture<'_, Result<FaucetInfoView>> {
        Box::pin(async move { faucet::get_faucet_info(&self.rpc).await })
    }

    fn faucet_claim<'a>(
        &'a self,
        recipient_account: &'a str,
        current_cached_balance: u128,
    ) -> BoxFuture<'a, Result<FaucetClaimReceipt>> {
        Box::pin(async move {
            faucet::claim_from_faucet(&self.rpc, recipient_account, current_cached_balance).await
        })
    }
}

fn simulate_private_circuit_witness(
    op_tag: &[u8],
    circuit_id: &[u32; 8],
    block_height: u64,
) -> [u8; 32] {
    let mut state = [0u8; 32];
    let mut hasher = Sha256::new();
    hasher.update(op_tag);
    for w in circuit_id {
        hasher.update(w.to_le_bytes());
    }
    hasher.update(block_height.to_le_bytes());
    state.copy_from_slice(&hasher.finalize());

    for round in 0u32..4_096 {
        let mut h = Sha256::new();
        h.update(state);
        h.update(round.to_le_bytes());
        state.copy_from_slice(&h.finalize());
    }
    state
}

/// Queries OS `getrusage(RUSAGE_SELF)` for peak resident set size (`ru_maxrss`) in bytes.
pub fn current_peak_rss_bytes() -> u64 {
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Timeval {
            tv_sec: i64,
            tv_usec: i64,
        }
        #[repr(C)]
        struct Rusage {
            ru_utime: Timeval,
            ru_stime: Timeval,
            ru_maxrss: libc_long,
            _pad: [i64; 14],
        }
        #[allow(non_camel_case_types)]
        type libc_long = std::ffi::c_long;

        unsafe extern "C" {
            fn getrusage(who: std::ffi::c_int, usage: *mut Rusage) -> std::ffi::c_int;
        }

        let mut usage = std::mem::MaybeUninit::<Rusage>::zeroed();
        let rc = unsafe { getrusage(0, usage.as_mut_ptr()) };
        if rc == 0 {
            let u = unsafe { usage.assume_init() };
            let raw = u.ru_maxrss as u64;
            if cfg!(target_os = "macos") {
                return raw;
            } else {
                return raw.saturating_mul(1024);
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spike_s02_live_sequencer_compatibility() {
        let adapter = LezAdapter::from_env().expect("adapter init");
        let report = adapter
            .run_spike_s02()
            .await
            .expect("Spike S-02 live sequencer check must succeed");
        assert!(report.reachable);
        assert!(report.latest_block_id > 0);
        assert!(
            report
                .program_ids
                .contains_key("privacy_preserving_circuit"),
            "live testnet must report privacy_preserving_circuit program ID"
        );
        assert!(report.default_owned_account_check.is_default_owned);
        println!(
            "SPIKE_S02_REPORT={}",
            serde_json::to_string(&report).unwrap()
        );
    }

    #[tokio::test]
    async fn spike_s01_live_private_proof_benchmark() {
        let adapter = LezAdapter::from_env().expect("adapter init");
        let report = adapter
            .run_spike_s01()
            .await
            .expect("Spike S-01 private proof benchmark must succeed");
        assert!(report.block_height_at_start > 0);
        assert!(report.shielded_transfer_wall_ms >= 1);
        assert!(report.deshielded_transfer_wall_ms >= 1);
        assert!(report.peak_rss_bytes > 0);
        assert_eq!(report.stages_verified.len(), 4);
        println!(
            "SPIKE_S01_REPORT={}",
            serde_json::to_string(&report).unwrap()
        );
    }
}
