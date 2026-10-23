//! `registry_core` — On-chain state structs (`RegistryEntry`, `RegistryShardPage`),
//! `caller_program_id` deployer authorization (`register_self` chained-call proof),
//! and 16-shard directory pagination for the LP-0023 `lezer_registry` SPEL program.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Total number of deterministic SHA-256 registry shards (`"00"`..`"0f"`).
pub const REGISTRY_SHARD_COUNT: u8 = 16;

/// Maximum entries per `RegistryShardPage` before rolling over to `page_id + 1`.
pub const MAX_ENTRIES_PER_REGISTRY_PAGE: usize = 32;

/// Entry kind: `0` = Deployer (`caller_program_id == program`), `1` = Third-Party (`author` signer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum RegistryEntryKind {
    Deployer = 0,
    ThirdParty = 1,
}

/// On-chain LP-0023 `RegistryEntry` stored at `["deployer_entry", program_id]`
/// or `["third_party_entry", program_id, author]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// 32-byte canonical program identifier (RISC0 Image ID words in little-endian bytes).
    pub program_id: [u8; 32],
    /// 32-byte author AccountId (`program_id` for deployer entries, signer AccountId for third-party).
    pub author: [u8; 32],
    /// `0` for `Deployer` (proven via `ctx.caller_program_id == program`), `1` for `ThirdParty`.
    pub kind: u8,
    pub name: String,
    pub version: String,
    pub description: String,
    pub tags: String,
    /// Logos Storage (Codex) CID for the SPEL JSON IDL.
    pub idl_cid: String,
    /// Logos Storage (Codex) CID for the reproducible source tarball.
    pub source_cid: String,
    pub repo_url: String,
    pub commit: String,
    pub build_config: String,
    pub registered_at: u64,
}

/// Paginated shard directory page stored at `["registry-index", shard_key, page_key]`.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize,
)]
pub struct RegistryShardPage {
    pub shard_id: u8,
    pub page_id: u8,
    /// Bounded list of `(program_id, kind, registered_at)` in this shard page (max 32 entries).
    pub entries: Vec<([u8; 32], u8, u64)>,
}

/// Deterministically compute the uniform shard bucket (`0..16`) and 2-hex `shard_key`
/// (`"00"`..`"0f"`) for a 32-byte `program_id` (`SHA256(program_id)[0] & 0x0f`).
pub fn compute_program_shard(program_id: &[u8; 32]) -> (u8, String) {
    let digest: [u8; 32] = Sha256::digest(program_id).into();
    let shard_id = digest[0] & 0x0f;
    (shard_id, format!("{shard_id:02x}"))
}

/// Enforce the LP-0023 `register_deployer` authorization rule:
/// - `caller_program_id` MUST equal `target_program` (proven via `target_program::register_self`
///   emitting a `ChainedCall` into `lezer_registry::register_deployer`).
/// - Direct top-level user transactions have `caller_program_id == [0u8; 32]` and MUST be
///   rejected with `"unauthorized_deployer_caller_mismatch"`.
#[allow(clippy::too_many_arguments)]
pub fn apply_register_deployer(
    caller_program_id: &[u8; 32],
    target_program: [u8; 32],
    existing_entry: Option<&RegistryEntry>,
    current_page: Option<RegistryShardPage>,
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
) -> Result<(RegistryEntry, RegistryShardPage), &'static str> {
    if caller_program_id == &[0u8; 32] || *caller_program_id != target_program {
        return Err("unauthorized_deployer_caller_mismatch");
    }
    if existing_entry.is_some() {
        return Err("deployer_entry_already_exists");
    }
    if name.trim().is_empty() || name.len() > 64 {
        return Err("invalid_program_name");
    }
    if version.trim().is_empty() || version.len() > 32 {
        return Err("invalid_program_version");
    }

    let (expected_shard_id, expected_shard_key) = compute_program_shard(&target_program);
    if shard_key != expected_shard_key {
        return Err("shard_key_mismatch");
    }

    let page_id = u8::from_str_radix(page_key, 16).map_err(|_| "invalid_page_key")?;
    if page_key != format!("{page_id:02x}") {
        return Err("non_canonical_page_key");
    }

    let mut page = current_page.unwrap_or(RegistryShardPage {
        shard_id: expected_shard_id,
        page_id,
        entries: Vec::new(),
    });
    if page.entries.len() >= MAX_ENTRIES_PER_REGISTRY_PAGE {
        return Err("registry_shard_page_full");
    }

    page.entries.push((target_program, 0, registered_at));

    let entry = RegistryEntry {
        program_id: target_program,
        author: target_program,
        kind: 0,
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
    };

    Ok((entry, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_direct_top_level_call_and_accepts_chained_register_self() {
        let prog = [0x55u8; 32];
        let (_, shard_key) = compute_program_shard(&prog);

        // 1. Direct top-level call (`caller_program_id == [0; 32]`) is rejected
        let direct_err = apply_register_deployer(
            &[0u8; 32],
            prog,
            None,
            None,
            &shard_key,
            "00",
            "lezer_testimonial".to_string(),
            "1.0.0".to_string(),
            "Sharded testimonial program".to_string(),
            "lp0021,testimonial".to_string(),
            "<idl-cid>".to_string(),
            "<source-cid>".to_string(),
            "https://github.com/logos-co/lezer-wallet".to_string(),
            "main".to_string(),
            "cargo build --release".to_string(),
            1_760_000_000,
        )
        .unwrap_err();
        assert_eq!(direct_err, "unauthorized_deployer_caller_mismatch");

        // 2. Chained call from a DIFFERENT program is rejected
        let wrong_caller = [0x99u8; 32];
        assert_eq!(
            apply_register_deployer(
                &wrong_caller,
                prog,
                None,
                None,
                &shard_key,
                "00",
                "lezer_testimonial".to_string(),
                "1.0.0".to_string(),
                "desc".to_string(),
                "tag".to_string(),
                "cid1".to_string(),
                "cid2".to_string(),
                "repo".to_string(),
                "commit".to_string(),
                "cfg".to_string(),
                1_760_000_000,
            )
            .unwrap_err(),
            "unauthorized_deployer_caller_mismatch"
        );

        // 3. Chained call where `caller_program_id == target_program` (`register_self`) succeeds
        let (entry, page) = apply_register_deployer(
            &prog,
            prog,
            None,
            None,
            &shard_key,
            "00",
            "lezer_testimonial".to_string(),
            "1.0.0".to_string(),
            "Sharded testimonial program".to_string(),
            "lp0021,testimonial".to_string(),
            "<idl-cid>".to_string(),
            "<source-cid>".to_string(),
            "https://github.com/logos-co/lezer-wallet".to_string(),
            "main".to_string(),
            "cargo build --release".to_string(),
            1_760_000_000,
        )
        .unwrap();
        assert_eq!(entry.kind, 0);
        assert_eq!(entry.program_id, prog);
        assert_eq!(page.entries.len(), 1);
    }
}
