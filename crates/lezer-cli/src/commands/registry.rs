//! LP-0023 Program Registry CLI commands (`lezer registry lookup`, `list`, `search`, `register`, `verify`).

use std::path::Path;

use clap::Subcommand;
use lezer_core::{
    LezerResult, list_registered_programs, lookup_program_entry, register_program_entry,
    search_registered_programs, verify_program_bytecode,
};
use serde_json::{Value, json};

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum RegistrySubcommand {
    /// Lookup an on-chain LP-0023 registry entry by its 64-hex RISC0 Image ID (`ProgramId`).
    Lookup {
        #[arg(long)]
        program: String,
    },
    /// List all registered programs across the 16 on-chain shards.
    List,
    /// Search registered programs by name, description, or tag.
    Search {
        #[arg(long)]
        query: String,
    },
    /// Register a program entry (enforcing `caller_program_id == program` for `deployer` entries).
    Register {
        /// 64-hex RISC0 Image ID (`ProgramId`).
        #[arg(long)]
        program: String,
        /// Entry kind (`deployer` or `third_party`).
        #[arg(long, default_value = "deployer")]
        kind: String,
        /// Optional caller_program_id (set equal to `program` when invoked via `register_self`).
        #[arg(long)]
        caller_program: Option<String>,
        /// Program name.
        #[arg(long)]
        name: String,
        /// Semantic version string.
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Human-readable description.
        #[arg(long, default_value = "")]
        description: String,
        /// Comma-separated tags.
        #[arg(long, default_value = "")]
        tags: String,
        /// Logos Storage CID for the SPEL JSON IDL.
        #[arg(long, default_value = "zDvZRwzmIdlDefault")]
        idl_cid: String,
        /// Logos Storage CID for the source tarball.
        #[arg(long, default_value = "zDvZRwzmSrcDefault")]
        source_cid: String,
        /// Source repository URL.
        #[arg(long, default_value = "https://github.com/logos-co/lezer-wallet")]
        repo_url: String,
        /// Git commit SHA or tag.
        #[arg(long, default_value = "v1.0.0")]
        commit: String,
    },
    /// Verify a program's on-chain `program_id` against rebuilt source and return its 4-state badge.
    Verify {
        /// 64-hex RISC0 Image ID (`ProgramId`).
        #[arg(long)]
        program: String,
        /// Optional rebuilt Image ID hex from `tooling/verify-program.sh`.
        #[arg(long)]
        rebuilt_image_id: Option<String>,
    },
}

pub fn handle_registry_subcommand(
    registry_ledger_path: &Path,
    sub: RegistrySubcommand,
) -> LezerResult<Value> {
    match sub {
        RegistrySubcommand::Lookup { program } => {
            let entry = lookup_program_entry(registry_ledger_path, &program)?;
            Ok(serde_json::to_value(entry).unwrap())
        }
        RegistrySubcommand::List => {
            let entries = list_registered_programs(registry_ledger_path);
            Ok(json!({ "programs": entries }))
        }
        RegistrySubcommand::Search { query } => {
            let matches = search_registered_programs(registry_ledger_path, &query);
            Ok(json!({ "query": query, "results": matches }))
        }
        RegistrySubcommand::Register {
            program,
            kind,
            caller_program,
            name,
            version,
            description,
            tags,
            idl_cid,
            source_cid,
            repo_url,
            commit,
        } => {
            let tag_list: Vec<String> = tags
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let entry = register_program_entry(
                registry_ledger_path,
                &program,
                caller_program.as_deref(),
                &program,
                &kind,
                &name,
                &version,
                &description,
                tag_list,
                &idl_cid,
                &source_cid,
                &repo_url,
                &commit,
                "cargo build --release",
                1_760_000_500,
            )?;
            Ok(serde_json::to_value(entry).unwrap())
        }
        RegistrySubcommand::Verify {
            program,
            rebuilt_image_id,
        } => {
            let report = verify_program_bytecode(
                registry_ledger_path,
                &program,
                rebuilt_image_id.as_deref(),
            )?;
            Ok(serde_json::to_value(report).unwrap())
        }
    }
}
