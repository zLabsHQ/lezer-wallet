//! `lezer-adapter` — the only crate that talks to the Logos Execution Zone.
//!
//! All key management, transaction building, signing, and proving is delegated to the
//! official `wallet::WalletCore` (logos-execution-zone `v0.3.1`) via [`engine::LezEngine`].
//! [`rpc::LezRpcClient`] provides raw, read-only JSON-RPC access (e.g. for stateless
//! verification tools that run without a wallet).

pub mod codecs;
pub mod engine;
pub mod faucet;
pub mod profile;
pub mod programs;
pub mod registry;
pub mod rpc;
pub mod token;

pub use codecs::{
    bytes_to_risc0_u32_words, program_id_words_from_hex, program_id_words_to_hex,
    risc0_u32_words_to_bytes,
};
pub use engine::{
    AccountKind, ChainStatus, DEFAULT_MAX_FEE, LezEngine, OwnedAccount, PRIVATE_RECEIVE_PREFIX,
    PrivateReceiveKeys, TransferMode, TransferTarget, TxReceipt, TxStatus, decode_receive_address,
    encode_receive_address, native_token_program_id, privacy_circuit_id, parse_public_id, parse_target,
};
pub use faucet::{FaucetStatus, faucet_status_rpc, faucet_vault_id};
pub use faucet_core::DEFAULT_DRIP_AMOUNT;
pub use profile::{
    DEFAULT_LOCALNET_SEQUENCER_URL, DEFAULT_TESTNET_SEQUENCER_URL, LezerDeployments,
    NetworkProfile,
};
pub use programs::{AdoptionReport, DeployedProgram, TestimonialEntry, image_id_hex, verify_adoption};
pub use registry::{
    RegistryEntry, RegistryReport, first_page_with_room_rpc, list_registry,
    lookup_registry_entry, register_self_instruction,
};
pub use token::{TokenInfo, token_balance_rpc, token_info_rpc};
pub use rpc::{AllowedEndpointsGuard, LezRpcClient, RpcAccountState};

/// Standalone sequencer client for wallet-less reads (same builder `WalletCore` uses).
pub fn sequencer_client(profile: &NetworkProfile) -> Result<sequencer_service_rpc::SequencerClient, anyhow::Error> {
    AllowedEndpointsGuard::from_profile(profile).check_url(&profile.sequencer_url)?;
    sequencer_service_rpc::SequencerClientBuilder::default()
        .build(profile.sequencer_url.as_str())
        .map_err(Into::into)
}

/// Peak resident set size of this process in bytes (`getrusage(RUSAGE_SELF).ru_maxrss`).
pub fn peak_rss_bytes() -> u64 {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: `getrusage` fully initializes the struct on success.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
            let raw = unsafe { usage.assume_init() }.ru_maxrss as u64;
            return if cfg!(target_os = "macos") { raw } else { raw * 1024 };
        }
    }
    0
}
