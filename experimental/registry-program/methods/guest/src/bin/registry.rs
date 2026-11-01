#![no_main]

//! `lezer_registry` SPEL (RISC Zero) guest program — LP-0023 Program Registry.
//!
//! Instructions:
//! - `register_deployer`: Enforces `ctx.caller_program_id == program` (chained call from `register_self`).
//! - `update_deployer`: Enforces `ctx.caller_program_id == program` (chained call from `update_self`).
//! - `register_third_party`: Signer-authenticated third-party attestation at `["third_party_entry", program, author]`.
//! - `register_self`: Self-registers `lezer_registry` in itself per LP-0023 Supportability Criterion #4.

use borsh::{BorshDeserialize, BorshSerialize};
pub use registry_core_experimental::{
    MAX_ENTRIES_PER_REGISTRY_PAGE, REGISTRY_SHARD_COUNT, RegistryEntry, RegistryEntryKind,
    RegistryShardPage, apply_register_deployer, compute_program_shard,
};

/// Execute `register_deployer` inside the RISC Zero guest environment.
#[allow(clippy::too_many_arguments)]
pub fn register_deployer_guest(
    caller_program_id: &[u8; 32],
    target_program: [u8; 32],
    existing_entry_bytes: &[u8],
    shard_page_bytes: &[u8],
    shard_key: &str,
    page_key: &str,
    name: String,
    version: String,
    description: String,
    tags: String,
    idl_cid: String,
    source_cid: String,
    repo_url: String,
    commit: String,
    build_config: String,
    registered_at: u64,
) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
    let existing = if existing_entry_bytes.is_empty() {
        None
    } else {
        Some(
            RegistryEntry::try_from_slice(existing_entry_bytes)
                .map_err(|_| "corrupt_existing_entry")?,
        )
    };
    let current_page = if shard_page_bytes.is_empty() {
        None
    } else {
        Some(
            RegistryShardPage::try_from_slice(shard_page_bytes)
                .map_err(|_| "corrupt_shard_page")?,
        )
    };

    let (entry, page) = apply_register_deployer(
        caller_program_id,
        target_program,
        existing.as_ref(),
        current_page,
        shard_key,
        page_key,
        name,
        version,
        description,
        tags,
        idl_cid,
        source_cid,
        repo_url,
        commit,
        build_config,
        registered_at,
    )?;

    let entry_bytes = borsh::to_vec(&entry).map_err(|_| "serialize_entry_failed")?;
    let page_bytes = borsh::to_vec(&page).map_err(|_| "serialize_page_failed")?;
    Ok((entry_bytes, page_bytes))
}
