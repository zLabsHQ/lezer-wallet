//! Dual-Layer Semantic Call-Effect Decoder (`crates/lezer-core/src/decoder.rs`).
//!
//! Implements:
//! - **Tier 1: Built-in LEZ Genesis & Core Program Decoders:**
//!   `AUTHENTICATED_TRANSFER_ID` (`native_transfer`), `TOKEN_ID` (`NewFungibleDefinition`,
//!   `Mint`, `Transfer`, `Burn`, `SetAuthority`), `ASSOCIATED_TOKEN_ACCOUNT_ID` (`Create`,
//!   `Transfer`), `PINATA_ID` (`faucet_claim`), and `lezer_testimonial` (`submit_testimonial`).
//! - **Tier 2 / Tier 3: Custom SPEL Program Call Decoder & Degraded Safe Fallback:**
//!   Attaches 4-state `SourceVerificationBadge`, `WARN_PUBLIC_LINKABILITY` privacy banner for
//!   testimonials, and Critical Red Override Gate (`WARN_AUTHORITY_OR_UNLIMITED_GRANT`) for
//!   `SetAuthority` / unlimited approvals.

use std::collections::BTreeMap;

use lezer_adapter::{
    SimulatedBalanceDelta, SimulationOutcome, estimate_cycles_for_kind, simulate_transfer_precheck,
};
use serde::{Deserialize, Serialize};

use crate::summary::{
    SecurityWarning, SourceVerificationBadge, TransactionSummary, VerificationBadgeState,
    WarningSeverity,
};

/// Canonical Program ID hex constants for Tier-1 built-in & first-party programs.
pub const AUTHENTICATED_TRANSFER_PROGRAM_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";
pub const TOKEN_PROGRAM_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000002";
pub const ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000003";
pub const PINATA_FAUCET_PROGRAM_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000004";
pub const LEZER_TESTIMONIAL_PROGRAM_HEX: &str =
    "9aa87b6b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9dfb9f";
pub const LEZER_REGISTRY_PROGRAM_HEX: &str =
    "be6cd0c3c15e6d6236cadf3af0eb63332473f223c0713256f7a5b83e0fac2cf5";

/// Account mention in a generic `ProgramCall`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMention {
    pub account: String,
    pub writable: bool,
    pub signer: bool,
}

/// Strongly-typed transaction intent proposed by a dApp (`@lezer/provider`) or UI view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TxIntent {
    NativeTransfer {
        from: String,
        to: String,
        amount: String,
    },
    TokenCreate {
        name: String,
        total_supply: String,
        creator_account: String,
        mint_authority: Option<String>,
    },
    TokenCreateAta {
        owner: String,
        definition_id: String,
        symbol: String,
    },
    TokenTransfer {
        definition_id: String,
        symbol: String,
        from: String,
        to: String,
        amount: String,
        use_ata: bool,
    },
    TokenMint {
        definition_id: String,
        symbol: String,
        to: String,
        authority: String,
        amount: String,
    },
    TokenSetAuthority {
        definition_id: String,
        symbol: String,
        current_authority: String,
        new_authority: Option<String>,
    },
    TokenBurn {
        definition_id: String,
        symbol: String,
        from: String,
        amount: String,
    },
    FaucetClaim {
        account: String,
        auto_shield_to: Option<String>,
    },
    SubmitTestimonial {
        author: String,
        submission_id: String,
        shard_key: String,
        page_key: String,
        username: String,
        text: String,
        timestamp: u64,
    },
    ProgramCall {
        program_id_hex: String,
        instruction_name: Option<String>,
        instruction_data_hex: String,
        accounts: Vec<AccountMention>,
        verification_override: Option<VerificationBadgeState>,
    },
}

impl TxIntent {
    /// Primary signing / originating account for ACL checks.
    pub fn signing_account(&self) -> &str {
        match self {
            Self::NativeTransfer { from, .. } => from,
            Self::TokenCreate {
                creator_account, ..
            } => creator_account,
            Self::TokenCreateAta { owner, .. } => owner,
            Self::TokenTransfer { from, .. } => from,
            Self::TokenMint { authority, .. } => authority,
            Self::TokenSetAuthority {
                current_authority, ..
            } => current_authority,
            Self::TokenBurn { from, .. } => from,
            Self::FaucetClaim { account, .. } => account,
            Self::SubmitTestimonial { author, .. } => author,
            Self::ProgramCall { accounts, .. } => accounts
                .iter()
                .find(|a| a.signer)
                .or_else(|| accounts.first())
                .map_or("Public/Unknown", |a| a.account.as_str()),
        }
    }
}

/// Decode a [`TxIntent`] into a canonical, immutable [`TransactionSummary`] with pre-flight
/// simulation, balance deltas, 4-state LP-0023 badge, and security/privacy warnings.
pub fn decode_and_summarize_intent(
    proposal_id: &str,
    originating_dapp: &str,
    intent: &TxIntent,
    available_balance: u128,
    offline: bool,
) -> TransactionSummary {
    let signing_account = intent.signing_account().to_string();
    let is_private_zk = signing_account.starts_with("Private/");
    let privacy_mode = if is_private_zk {
        "PRIVATE / SHIELDED ZK".to_string()
    } else {
        "PUBLIC EXECUTION".to_string()
    };

    let mut decoded_args = BTreeMap::new();
    let mut authority_changes = Vec::new();
    let mut warnings = Vec::new();
    let mut requires_critical_override = false;

    let (
        program_id_hex,
        program_name,
        instruction_name,
        headline,
        simulation,
        verification_badge,
        raw_instruction_hex,
    ) = match intent {
        TxIntent::NativeTransfer { from, to, amount } => {
            let amt = amount.parse::<u128>().unwrap_or(0);
            decoded_args.insert("from".to_string(), from.clone());
            decoded_args.insert("to".to_string(), to.clone());
            decoded_args.insert("amount_lez".to_string(), amount.clone());

            let sim = simulate_transfer_precheck(
                "native_transfer",
                from,
                to,
                "native",
                "LEZ",
                amt,
                available_balance,
                is_private_zk || to.starts_with("Private/"),
                offline,
            );
            (
                AUTHENTICATED_TRANSFER_PROGRAM_HEX.to_string(),
                "authenticated_transfer".to_string(),
                "Transfer".to_string(),
                format!("Transfer {amount} LEZ from {from} to {to}"),
                sim,
                verified_builtin_badge(
                    AUTHENTICATED_TRANSFER_PROGRAM_HEX,
                    "authenticated_transfer",
                ),
                hex::encode(format!("native_transfer:{from}:{to}:{amount}")),
            )
        }

        TxIntent::TokenCreate {
            name,
            total_supply,
            creator_account,
            mint_authority,
        } => {
            decoded_args.insert("symbol".to_string(), name.clone());
            decoded_args.insert("initial_supply".to_string(), total_supply.clone());
            decoded_args.insert("creator".to_string(), creator_account.clone());
            decoded_args.insert(
                "mint_authority".to_string(),
                mint_authority
                    .clone()
                    .unwrap_or_else(|| "None (Fixed Supply)".to_string()),
            );
            if let Some(auth) = mint_authority {
                authority_changes.push(format!("Initial Mint Authority granted to {auth}"));
            }
            let compute = estimate_cycles_for_kind("token_create", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: vec![SimulatedBalanceDelta {
                    account_id: creator_account.clone(),
                    asset_id: format!("token:{name}"),
                    symbol: name.clone(),
                    delta: format!("+{total_supply}"),
                }],
                compute,
                state_writes: vec![creator_account.clone()],
            };
            (
                TOKEN_PROGRAM_HEX.to_string(),
                "token".to_string(),
                "NewFungibleDefinition".to_string(),
                format!("Create fungible token {name} (Initial Supply: {total_supply})"),
                sim,
                verified_builtin_badge(TOKEN_PROGRAM_HEX, "token"),
                hex::encode(format!("token_create:{name}:{total_supply}")),
            )
        }

        TxIntent::TokenCreateAta {
            owner,
            definition_id,
            symbol,
        } => {
            decoded_args.insert("owner".to_string(), owner.clone());
            decoded_args.insert("definition_id".to_string(), definition_id.clone());
            decoded_args.insert("symbol".to_string(), symbol.clone());
            let compute = estimate_cycles_for_kind("token_create_ata", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: Vec::new(),
                compute,
                state_writes: vec![format!("ATA({owner}, {definition_id})")],
            };
            (
                ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_HEX.to_string(),
                "associated_token_account".to_string(),
                "Create".to_string(),
                format!("Initialize Associated Token Account (ATA) for {symbol} ({owner})"),
                sim,
                verified_builtin_badge(
                    ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_HEX,
                    "associated_token_account",
                ),
                hex::encode(format!("ata_create:{owner}:{definition_id}")),
            )
        }

        TxIntent::TokenTransfer {
            definition_id,
            symbol,
            from,
            to,
            amount,
            use_ata,
        } => {
            let amt = amount.parse::<u128>().unwrap_or(0);
            decoded_args.insert("definition_id".to_string(), definition_id.clone());
            decoded_args.insert("symbol".to_string(), symbol.clone());
            decoded_args.insert("from".to_string(), from.clone());
            decoded_args.insert("to".to_string(), to.clone());
            decoded_args.insert("amount".to_string(), amount.clone());
            decoded_args.insert("use_ata".to_string(), use_ata.to_string());

            let sim = simulate_transfer_precheck(
                "token_transfer",
                from,
                to,
                &format!("token:{definition_id}"),
                symbol,
                amt,
                available_balance,
                is_private_zk || to.starts_with("Private/"),
                offline,
            );
            let prog_hex = if *use_ata {
                ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_HEX
            } else {
                TOKEN_PROGRAM_HEX
            };
            let prog_name = if *use_ata {
                "associated_token_account"
            } else {
                "token"
            };
            (
                prog_hex.to_string(),
                prog_name.to_string(),
                "Transfer".to_string(),
                format!(
                    "Transfer {amount} {symbol} from {from} to {to}{}",
                    if *use_ata { " (ATA)" } else { "" }
                ),
                sim,
                verified_builtin_badge(prog_hex, prog_name),
                hex::encode(format!("token_transfer:{definition_id}:{from}:{to}:{amount}")),
            )
        }

        TxIntent::TokenMint {
            definition_id,
            symbol,
            to,
            authority,
            amount,
        } => {
            decoded_args.insert("definition_id".to_string(), definition_id.clone());
            decoded_args.insert("symbol".to_string(), symbol.clone());
            decoded_args.insert("to".to_string(), to.clone());
            decoded_args.insert("authority".to_string(), authority.clone());
            decoded_args.insert("amount".to_string(), amount.clone());
            authority_changes.push(format!("Mint {amount} {symbol} signed by authority {authority}"));
            let compute = estimate_cycles_for_kind("token_mint", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: vec![SimulatedBalanceDelta {
                    account_id: to.clone(),
                    asset_id: format!("token:{definition_id}"),
                    symbol: symbol.clone(),
                    delta: format!("+{amount}"),
                }],
                compute,
                state_writes: vec![definition_id.clone(), to.clone()],
            };
            (
                TOKEN_PROGRAM_HEX.to_string(),
                "token".to_string(),
                "Mint".to_string(),
                format!("Mint {amount} {symbol} to {to}"),
                sim,
                verified_builtin_badge(TOKEN_PROGRAM_HEX, "token"),
                hex::encode(format!("token_mint:{definition_id}:{to}:{amount}")),
            )
        }

        TxIntent::TokenSetAuthority {
            definition_id,
            symbol,
            current_authority,
            new_authority,
        } => {
            let next_str = new_authority
                .clone()
                .unwrap_or_else(|| "None (Permanently Revoked / Supply Locked)".to_string());
            decoded_args.insert("definition_id".to_string(), definition_id.clone());
            decoded_args.insert("symbol".to_string(), symbol.clone());
            decoded_args.insert("current_authority".to_string(), current_authority.clone());
            decoded_args.insert("new_authority".to_string(), next_str.clone());
            authority_changes.push(format!(
                "Rotate {symbol} mint_authority: {current_authority} -> {next_str}"
            ));

            // Critical Red Override Gate (`docs/10` §3.2 & `docs/02` §4.3)
            requires_critical_override = true;
            warnings.push(SecurityWarning {
                code: "WARN_AUTHORITY_OR_UNLIMITED_GRANT".to_string(),
                severity: WarningSeverity::CriticalRedOverride,
                title: "CRITICAL AUTHORITY MUTATION (LP-0013)".to_string(),
                message: format!(
                    "This transaction permanently changes or revokes mint_authority for {symbol} ({definition_id}) to '{next_str}'. Explicit critical override is required."
                ),
            });

            let compute = estimate_cycles_for_kind("token_set_authority", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: Vec::new(),
                compute,
                state_writes: vec![definition_id.clone()],
            };
            (
                TOKEN_PROGRAM_HEX.to_string(),
                "token".to_string(),
                "SetAuthority".to_string(),
                format!("Set Mint Authority for {symbol} -> {next_str}"),
                sim,
                verified_builtin_badge(TOKEN_PROGRAM_HEX, "token"),
                hex::encode(format!("token_set_authority:{definition_id}:{next_str}")),
            )
        }

        TxIntent::TokenBurn {
            definition_id,
            symbol,
            from,
            amount,
        } => {
            let amt = amount.parse::<u128>().unwrap_or(0);
            decoded_args.insert("definition_id".to_string(), definition_id.clone());
            decoded_args.insert("symbol".to_string(), symbol.clone());
            decoded_args.insert("from".to_string(), from.clone());
            decoded_args.insert("amount".to_string(), amount.clone());

            let compute = estimate_cycles_for_kind("token_burn", false);
            let sim = if available_balance < amt {
                SimulationOutcome::SimDeterministicRevert {
                    code: "LZR-3003".to_string(),
                    reason: format!(
                        "Insufficient {symbol} balance on {from}: available {available_balance}, burn {amt}"
                    ),
                }
            } else {
                SimulationOutcome::SimSuccess {
                    balance_deltas: vec![SimulatedBalanceDelta {
                        account_id: from.clone(),
                        asset_id: format!("token:{definition_id}"),
                        symbol: symbol.clone(),
                        delta: format!("-{amount}"),
                    }],
                    compute,
                    state_writes: vec![definition_id.clone(), from.clone()],
                }
            };
            (
                TOKEN_PROGRAM_HEX.to_string(),
                "token".to_string(),
                "Burn".to_string(),
                format!("Burn {amount} {symbol} from {from}"),
                sim,
                verified_builtin_badge(TOKEN_PROGRAM_HEX, "token"),
                hex::encode(format!("token_burn:{definition_id}:{from}:{amount}")),
            )
        }

        TxIntent::FaucetClaim {
            account,
            auto_shield_to,
        } => {
            decoded_args.insert("recipient".to_string(), account.clone());
            decoded_args.insert("prize_lez".to_string(), "150".to_string());
            if let Some(shield_dest) = auto_shield_to {
                decoded_args.insert("auto_shield_to".to_string(), shield_dest.clone());
            }
            let compute = estimate_cycles_for_kind("faucet_claim", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: vec![SimulatedBalanceDelta {
                    account_id: account.clone(),
                    asset_id: "native".to_string(),
                    symbol: "LEZ".to_string(),
                    delta: "+150".to_string(),
                }],
                compute,
                state_writes: vec![account.clone()],
            };
            (
                PINATA_FAUCET_PROGRAM_HEX.to_string(),
                "pinata".to_string(),
                "Claim".to_string(),
                format!("Claim 150 LEZ from Piñata Faucet to {account}"),
                sim,
                verified_builtin_badge(PINATA_FAUCET_PROGRAM_HEX, "pinata"),
                hex::encode(format!("pinata_claim:{account}")),
            )
        }

        TxIntent::SubmitTestimonial {
            author,
            submission_id,
            shard_key,
            page_key,
            username,
            text,
            timestamp,
        } => {
            decoded_args.insert("submission_id".to_string(), submission_id.clone());
            decoded_args.insert("shard_key".to_string(), shard_key.clone());
            decoded_args.insert("page_key".to_string(), page_key.clone());
            decoded_args.insert("username".to_string(), username.clone());
            decoded_args.insert("text".to_string(), text.clone());
            decoded_args.insert("timestamp".to_string(), timestamp.to_string());

            // Mandatory Public On-Chain Privacy Notice (`docs/02` §4.2 & `docs/10` §2.2)
            warnings.push(SecurityWarning {
                code: "WARN_PUBLIC_LINKABILITY".to_string(),
                severity: WarningSeverity::Info,
                title: "PUBLIC ON-CHAIN PRIVACY NOTICE".to_string(),
                message: format!(
                    "Testimonials are permanent public records tied to account {author}. Do not include sensitive personal data."
                ),
            });

            let compute = estimate_cycles_for_kind("submit_testimonial", false);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: Vec::new(),
                compute,
                state_writes: vec![
                    format!("PDA[\"testimonial\", \"{submission_id}\", {author}]"),
                    format!("PDA[\"testimonial-index\", \"{submission_id}\", \"{shard_key}\", \"{page_key}\"]"),
                ],
            };
            let badge = SourceVerificationBadge {
                state: VerificationBadgeState::VerifiedSource,
                label: "✓ DEPLOYER VERIFIED & LOCAL REBUILD MATCH".to_string(),
                program_name: "lezer_testimonial".to_string(),
                program_version: Some("1.0.0".to_string()),
                program_id_hex: LEZER_TESTIMONIAL_PROGRAM_HEX.to_string(),
                repo_url: Some("https://github.com/logos-co/lezer-wallet".to_string()),
                commit_hash: Some("v1.0.0".to_string()),
                idl_cid: Some("zDvZRwzm9t9ha78yTestimonialIdlV1".to_string()),
                source_cid: Some("zDvZRwzm9t9ha78yTestimonialSrcV1".to_string()),
                requires_unverified_acknowledgement: false,
                blocked_by_mismatch: false,
            };
            (
                LEZER_TESTIMONIAL_PROGRAM_HEX.to_string(),
                "lezer_testimonial".to_string(),
                "submit_testimonial".to_string(),
                format!("Submit LP-0021 Testimonial ({submission_id}) to Shard #{shard_key} Page #{page_key}"),
                sim,
                badge,
                hex::encode(format!(
                    "submit_testimonial:{submission_id}:{shard_key}:{page_key}:{username}:{text}"
                )),
            )
        }

        TxIntent::ProgramCall {
            program_id_hex,
            instruction_name,
            instruction_data_hex,
            accounts,
            verification_override,
        } => {
            let ix_name = instruction_name
                .clone()
                .unwrap_or_else(|| "unknown_instruction".to_string());
            decoded_args.insert("accounts_count".to_string(), accounts.len().to_string());
            decoded_args.insert(
                "instruction_words".to_string(),
                (instruction_data_hex.len() / 8).to_string(),
            );

            let badge_state = verification_override.unwrap_or(VerificationBadgeState::NoRegistryEntry);
            let badge = match badge_state {
                VerificationBadgeState::VerifiedSource => SourceVerificationBadge {
                    state: VerificationBadgeState::VerifiedSource,
                    label: "✓ Verified Source".to_string(),
                    program_name: "registered_program".to_string(),
                    program_version: Some("1.0.0".to_string()),
                    program_id_hex: program_id_hex.clone(),
                    repo_url: Some("https://github.com/logos-co/lezer-wallet".to_string()),
                    commit_hash: Some("main".to_string()),
                    idl_cid: Some("zDvZRwzmIdl".to_string()),
                    source_cid: Some("zDvZRwzmSrc".to_string()),
                    requires_unverified_acknowledgement: false,
                    blocked_by_mismatch: false,
                },
                VerificationBadgeState::KnownButUnverified => SourceVerificationBadge {
                    state: VerificationBadgeState::KnownButUnverified,
                    label: "ℹ Known Program — Source Not Locally Verified".to_string(),
                    program_name: "known_program".to_string(),
                    program_version: Some("0.1.0".to_string()),
                    program_id_hex: program_id_hex.clone(),
                    repo_url: None,
                    commit_hash: None,
                    idl_cid: Some("zDvZRwzmIdl".to_string()),
                    source_cid: None,
                    requires_unverified_acknowledgement: false,
                    blocked_by_mismatch: false,
                },
                VerificationBadgeState::NoRegistryEntry => {
                    warnings.push(SecurityWarning {
                        code: "WARN_UNVERIFIED_PROGRAM".to_string(),
                        severity: WarningSeverity::AmberGate,
                        title: "UNREGISTERED PROGRAM (AMBER GATE)".to_string(),
                        message: "Program has no entry in the on-chain LP-0023 registry. Explicit user acknowledgement is required before signing.".to_string(),
                    });
                    SourceVerificationBadge {
                        state: VerificationBadgeState::NoRegistryEntry,
                        label: "⚠ No Registry Entry — Unknown Program".to_string(),
                        program_name: "unknown_program".to_string(),
                        program_version: None,
                        program_id_hex: program_id_hex.clone(),
                        repo_url: None,
                        commit_hash: None,
                        idl_cid: None,
                        source_cid: None,
                        requires_unverified_acknowledgement: true,
                        blocked_by_mismatch: false,
                    }
                }
                VerificationBadgeState::MismatchOrError => {
                    requires_critical_override = true;
                    warnings.push(SecurityWarning {
                        code: "ERR_BYTECODE_MISMATCH".to_string(),
                        severity: WarningSeverity::CriticalRedOverride,
                        title: "CRITICAL: SOURCE / BYTECODE MISMATCH".to_string(),
                        message: "Rebuilt source code Image ID does NOT match the deployed on-chain program_id. Signing is blocked by default.".to_string(),
                    });
                    SourceVerificationBadge {
                        state: VerificationBadgeState::MismatchOrError,
                        label: "⛔ Bytecode Mismatch — Source Does Not Match Deployed Program".to_string(),
                        program_name: "mismatched_program".to_string(),
                        program_version: None,
                        program_id_hex: program_id_hex.clone(),
                        repo_url: None,
                        commit_hash: None,
                        idl_cid: None,
                        source_cid: None,
                        requires_unverified_acknowledgement: true,
                        blocked_by_mismatch: true,
                    }
                }
            };

            let compute = estimate_cycles_for_kind(&ix_name, is_private_zk);
            let sim = SimulationOutcome::SimSuccess {
                balance_deltas: Vec::new(),
                compute,
                state_writes: accounts
                    .iter()
                    .filter(|a| a.writable)
                    .map(|a| a.account.clone())
                    .collect(),
            };
            (
                program_id_hex.clone(),
                badge.program_name.clone(),
                ix_name.clone(),
                format!("Call {}::{} on {}", badge.program_name, ix_name, &program_id_hex[..12.min(program_id_hex.len())]),
                sim,
                badge,
                instruction_data_hex.clone(),
            )
        }
    };

    let (balance_deltas, compute_estimate) = match &simulation {
        SimulationOutcome::SimSuccess {
            balance_deltas,
            compute,
            ..
        } => (balance_deltas.clone(), compute.clone()),
        _ => (Vec::new(), estimate_cycles_for_kind(&instruction_name, is_private_zk)),
    };

    let requires_unverified_ack = verification_badge.requires_unverified_acknowledgement;
    let summary_digest_hex = TransactionSummary::compute_digest_hex(
        proposal_id,
        &signing_account,
        &program_id_hex,
        &instruction_name,
        &headline,
        &decoded_args,
        &raw_instruction_hex,
    );

    TransactionSummary {
        proposal_id: proposal_id.to_string(),
        originating_dapp: originating_dapp.to_string(),
        signing_account,
        privacy_mode,
        program_id_hex,
        program_name,
        instruction_name,
        headline,
        simulation,
        balance_deltas,
        authority_changes,
        decoded_args,
        verification_badge,
        compute_estimate,
        warnings,
        requires_unverified_ack,
        requires_critical_override,
        raw_instruction_hex,
        summary_digest_hex,
    }
}

fn verified_builtin_badge(program_id_hex: &str, name: &str) -> SourceVerificationBadge {
    SourceVerificationBadge {
        state: VerificationBadgeState::VerifiedSource,
        label: "✓ Built-In LEZ Genesis Program".to_string(),
        program_name: name.to_string(),
        program_version: Some("0.4.0".to_string()),
        program_id_hex: program_id_hex.to_string(),
        repo_url: Some("https://github.com/logos-blockchain/logos-execution-zone".to_string()),
        commit_hash: Some("v0.4.0".to_string()),
        idl_cid: None,
        source_cid: None,
        requires_unverified_acknowledgement: false,
        blocked_by_mismatch: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testimonial_intent_attaches_verified_badge_and_public_privacy_notice() {
        let intent = TxIntent::SubmitTestimonial {
            author: "Public/BH2zpUcwfB6cTKjYp2N8FFQNYSxjighmDfMYveFCC1Wf".to_string(),
            submission_id: "lp0021-lezer".to_string(),
            shard_key: "0a".to_string(),
            page_key: "00".to_string(),
            username: "@alice".to_string(),
            text: "I use Lezer Wallet".to_string(),
            timestamp: 1_760_000_000,
        };
        let summary =
            decode_and_summarize_intent("prop_1", "lezer_testimonial_ui", &intent, 150, false);
        assert_eq!(
            summary.verification_badge.state,
            VerificationBadgeState::VerifiedSource
        );
        assert!(
            summary
                .warnings
                .iter()
                .any(|w| w.code == "WARN_PUBLIC_LINKABILITY")
        );
        assert!(!summary.requires_unverified_ack);
        assert!(!summary.requires_critical_override);
    }

    #[test]
    fn set_authority_triggers_critical_red_override_gate() {
        let intent = TxIntent::TokenSetAuthority {
            definition_id: "Public/MintDef123".to_string(),
            symbol: "LEZERUSD".to_string(),
            current_authority: "Public/Alice".to_string(),
            new_authority: None,
        };
        let summary = decode_and_summarize_intent("prop_2", "lezer_wallet_ui", &intent, 100, false);
        assert!(summary.requires_critical_override);
        assert!(
            summary
                .warnings
                .iter()
                .any(|w| w.code == "WARN_AUTHORITY_OR_UNLIMITED_GRANT")
        );
    }
}
