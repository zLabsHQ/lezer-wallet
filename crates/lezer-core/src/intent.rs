//! Transaction intents proposed by dApps / UI, and their translation into a canonical
//! [`TransactionSummary`].

use lezer_adapter::{DEFAULT_DRIP_AMOUNT, PRIVATE_RECEIVE_PREFIX, RegistryEntry, TokenInfo};
use serde::{Deserialize, Serialize};

use crate::summary::{
    Precheck, SecurityWarning, SourceVerificationBadge, TransactionSummary, VerificationBadgeState,
    WarningSeverity,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TxIntent {
    /// Native token transfer. `to` may be `Public/..`, an owned `Private/..`, or a
    /// `lezer:priv1…` receive address. `amount` is a decimal string (u128).
    NativeTransfer { from: String, to: String, amount: String },
    /// LP-0021 testimonial, written publicly on chain and linked to `author`.
    SubmitTestimonial { author: String, username: String, text: String },
    /// LP-0021 faucet claim: dispenses `DEFAULT_DRIP_AMOUNT` native LEZ from the on-chain
    /// `lezer_faucet` treasury PDA to `recipient` (enforces per-account rate-limit PDA).
    FaucetClaim { recipient: String },
    /// `lezer_token` transfer from `from` to `to`.
    TokenTransfer { token_id: String, from: String, to: String, amount: String },
    /// `lezer_token` mint by `authority` to `to`.
    TokenMint { token_id: String, authority: String, to: String, amount: String },
    /// `lezer_token` burn by holder `from`.
    TokenBurn { token_id: String, from: String, amount: String },
    /// `lezer_token` authority rotation (`new_authority: None` renounces permanently).
    TokenSetAuthority { token_id: String, authority: String, new_authority: Option<String> },
}

impl TxIntent {
    pub fn signing_account(&self) -> &str {
        match self {
            Self::NativeTransfer { from, .. } => from,
            Self::SubmitTestimonial { author, .. } => author,
            Self::FaucetClaim { recipient } => recipient,
            Self::TokenTransfer { from, .. } => from,
            Self::TokenMint { authority, .. } => authority,
            Self::TokenBurn { from, .. } => from,
            Self::TokenSetAuthority { authority, .. } => authority,
        }
    }

    /// Amount parsed from the intent (`Some(0)` for zero-value / authority / claim calls).
    pub fn amount(&self) -> Option<u128> {
        match self {
            Self::NativeTransfer { amount, .. }
            | Self::TokenTransfer { amount, .. }
            | Self::TokenMint { amount, .. }
            | Self::TokenBurn { amount, .. } => amount.trim().parse().ok(),
            Self::SubmitTestimonial { .. }
            | Self::FaucetClaim { .. }
            | Self::TokenSetAuthority { .. } => Some(0),
        }
    }

    /// Token definition ID if this is a `lezer_token` intent.
    pub fn token_id(&self) -> Option<&str> {
        match self {
            Self::TokenTransfer { token_id, .. }
            | Self::TokenMint { token_id, .. }
            | Self::TokenBurn { token_id, .. }
            | Self::TokenSetAuthority { token_id, .. } => Some(token_id),
            _ => None,
        }
    }

    /// Binary name of the target SPEL program (`None` for built-in native transfers).
    pub fn program_binary_name(&self) -> Option<&'static str> {
        match self {
            Self::NativeTransfer { .. } => None,
            Self::SubmitTestimonial { .. } => Some("lezer_testimonial"),
            Self::FaucetClaim { .. } => Some("lezer_faucet"),
            Self::TokenTransfer { .. }
            | Self::TokenMint { .. }
            | Self::TokenBurn { .. }
            | Self::TokenSetAuthority { .. } => Some("lezer_token"),
        }
    }
}

/// On-chain metadata gathered before summarizing an intent.
#[derive(Debug, Clone, Default)]
pub struct SummarizeContext {
    pub native_token_program_id: String,
    pub max_fee: Option<u128>,
    pub target_program_id: Option<String>,
    pub registry_entry: Option<RegistryEntry>,
    pub token_info: Option<TokenInfo>,
}

pub fn privacy_mode(from: &str, to: &str) -> &'static str {
    let from_private = from.starts_with("Private/");
    let to_private = to.starts_with("Private/") || to.starts_with(PRIVATE_RECEIVE_PREFIX);
    match (from_private, to_private) {
        (false, false) => "public",
        (false, true) => "shielded",
        (true, false) => "deshielded",
        (true, true) => "private",
    }
}

fn short(id: &str) -> String {
    if id.starts_with(PRIVATE_RECEIVE_PREFIX) {
        return "a private receive address".to_string();
    }
    match id.split_once('/') {
        Some((p, rest)) if rest.len() > 12 => format!("{p}/{}…{}", &rest[..6], &rest[rest.len() - 4..]),
        _ => id.to_string(),
    }
}

/// Badge for a SPEL program call. A registry entry proves the *program itself* registered
/// (chained-call auth) and declared its source; it does not prove a reproducible rebuild
/// matches the deployed image, so it maps to `KnownButUnverified`, never `VerifiedSource`.
fn build_program_badge(
    program_name: &str,
    program_id: Option<&str>,
    registry_entry: Option<&RegistryEntry>,
) -> (SourceVerificationBadge, bool) {
    match (program_id, registry_entry) {
        (Some(pid), Some(entry)) => {
            let commit = &entry.commit[..entry.commit.len().min(12)];
            let source = if entry.source_url.is_empty() {
                "no source declared".to_string()
            } else {
                format!("source {} @ {}", entry.source_url, if commit.is_empty() { "?" } else { commit })
            };
            (
                SourceVerificationBadge {
                    state: VerificationBadgeState::KnownButUnverified,
                    label: format!(
                        "Registered in LP-0023 registry as “{} {}” — {source} (self-declared; reproducible build not verified)",
                        entry.name, entry.version
                    ),
                    program_name: program_name.into(),
                    program_id: pid.to_string(),
                },
                true,
            )
        }
        (Some(pid), None) => (
            SourceVerificationBadge {
                state: VerificationBadgeState::NoRegistryEntry,
                label: format!("{program_name}: no LP-0023 registry entry on this network — source unknown"),
                program_name: program_name.into(),
                program_id: pid.to_string(),
            },
            true,
        ),
        (None, _) => (
            SourceVerificationBadge {
                state: VerificationBadgeState::NoRegistryEntry,
                label: format!("{program_name} (not deployed on this network)"),
                program_name: program_name.into(),
                program_id: String::new(),
            },
            true,
        ),
    }
}

fn token_label(token_id: &str, info: Option<&TokenInfo>) -> String {
    match info {
        Some(t) => format!("{} ({})", t.symbol, short(token_id)),
        None => format!("token {}", short(token_id)),
    }
}

/// Build the canonical summary the user approves.
pub fn summarize(
    proposal_id: &str,
    dapp: &str,
    intent: &TxIntent,
    precheck: Precheck,
    ctx: &SummarizeContext,
) -> TransactionSummary {
    let max_fee_str = ctx.max_fee.map(|f| f.to_string());
    let program_id = ctx.target_program_id.as_deref();
    let registry_entry = ctx.registry_entry.as_ref();

    match intent {
        TxIntent::NativeTransfer { from, to, amount } => {
            let mode = privacy_mode(from, to);
            let mut warnings = Vec::new();
            match mode {
                "public" => warnings.push(SecurityWarning {
                    code: "WARN_PUBLIC_LINKABILITY".into(),
                    severity: WarningSeverity::Info,
                    title: "Public transaction".into(),
                    message: "Sender, recipient and amount are visible on chain.".into(),
                }),
                "shielded" => warnings.push(SecurityWarning {
                    code: "WARN_SHIELD_SENDER_PUBLIC".into(),
                    severity: WarningSeverity::Info,
                    title: "Shielding".into(),
                    message: "Your public sender account and the amount are visible; the recipient stays private.".into(),
                }),
                "deshielded" => warnings.push(SecurityWarning {
                    code: "WARN_DESHIELD_REVEALS".into(),
                    severity: WarningSeverity::AmberGate,
                    title: "Leaving private balance".into(),
                    message: "The amount and the public recipient become visible on chain.".into(),
                }),
                _ => {}
            }
            if mode != "public" {
                warnings.push(SecurityWarning {
                    code: "INFO_LOCAL_PROOF".into(),
                    severity: WarningSeverity::Info,
                    title: "Zero-knowledge proof".into(),
                    message: "A proof is generated on this device before submission; this can take a while.".into(),
                });
            }

            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: from.clone(),
                privacy_mode: mode.to_string(),
                requires_zk_proof: mode != "public",
                instruction_name: "native_token::Transfer".into(),
                headline: format!("Send {amount} from {} to {}", short(from), short(to)),
                from: from.clone(),
                to: to.clone(),
                amount: amount.clone(),
                max_fee: (mode == "public").then(|| max_fee_str.clone()).flatten(),
                precheck,
                verification_badge: SourceVerificationBadge {
                    state: VerificationBadgeState::ProtocolBuiltin,
                    label: "Built-in LEZ native token program".into(),
                    program_name: "native_token".into(),
                    program_id: ctx.native_token_program_id.clone(),
                },
                warnings,
                requires_unverified_ack: false,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::SubmitTestimonial { author, username, text } => {
            let preview: String = text.chars().take(120).collect();
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_testimonial", program_id, registry_entry);
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: author.to_string(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_testimonial::Submit".into(),
                headline: format!("Publish a testimonial as {} : “{preview}”", short(author)),
                from: author.to_string(),
                to: program_id.unwrap_or("not deployed").to_string(),
                amount: "0".into(),
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![SecurityWarning {
                    code: "WARN_PUBLIC_ONCHAIN_DATA".into(),
                    severity: WarningSeverity::AmberGate,
                    title: "Permanent public record".into(),
                    message: format!(
                        "The text{} will be stored publicly and permanently on chain, linked to {}. Use a fresh public account if you do not want it linked to your other activity.",
                        if username.is_empty() { String::new() } else { format!(" and username “{username}”") },
                        short(author)
                    ),
                }],
                requires_unverified_ack,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::FaucetClaim { recipient } => {
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_faucet", program_id, registry_entry);
            let drip = DEFAULT_DRIP_AMOUNT.to_string();
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: recipient.clone(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_faucet::Claim".into(),
                headline: format!("Claim 50 LEZ ({drip} atomic) from faucet into {}", short(recipient)),
                from: program_id.unwrap_or("not deployed").to_string(),
                to: recipient.clone(),
                amount: drip,
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![SecurityWarning {
                    code: "INFO_FAUCET_CLAIM".into(),
                    severity: WarningSeverity::Info,
                    title: "On-chain faucet claim".into(),
                    message: format!(
                        "Dispenses native LEZ from the faucet treasury PDA to {} and records a 1-per-account claim ticket on chain.",
                        short(recipient)
                    ),
                }],
                requires_unverified_ack,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::TokenTransfer { token_id, from, to, amount } => {
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_token", program_id, registry_entry);
            let tlabel = token_label(token_id, ctx.token_info.as_ref());
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: from.clone(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_token::Transfer".into(),
                headline: format!("Transfer {amount} {tlabel} from {} to {}", short(from), short(to)),
                from: from.clone(),
                to: to.clone(),
                amount: amount.clone(),
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![SecurityWarning {
                    code: "WARN_PUBLIC_TOKEN_TRANSFER".into(),
                    severity: WarningSeverity::Info,
                    title: "Public token transfer".into(),
                    message: format!(
                        "Moves {amount} {tlabel} from {}'s holding PDA to {}'s holding PDA on program {}.",
                        short(from),
                        short(to),
                        short(program_id.unwrap_or("lezer_token"))
                    ),
                }],
                requires_unverified_ack,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::TokenMint { token_id, authority, to, amount } => {
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_token", program_id, registry_entry);
            let tlabel = token_label(token_id, ctx.token_info.as_ref());
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: authority.clone(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_token::Mint".into(),
                headline: format!("Mint {amount} {tlabel} to {} (authority {})", short(to), short(authority)),
                from: authority.clone(),
                to: to.clone(),
                amount: amount.clone(),
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![SecurityWarning {
                    code: "WARN_TOKEN_SUPPLY_INCREASE".into(),
                    severity: WarningSeverity::AmberGate,
                    title: "Increases token total supply".into(),
                    message: format!(
                        "Mints {amount} new units of {tlabel} to {} using mint authority {}.",
                        short(to),
                        short(authority)
                    ),
                }],
                requires_unverified_ack,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::TokenBurn { token_id, from, amount } => {
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_token", program_id, registry_entry);
            let tlabel = token_label(token_id, ctx.token_info.as_ref());
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: from.clone(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_token::Burn".into(),
                headline: format!("Burn {amount} {tlabel} from {}", short(from)),
                from: from.clone(),
                to: token_id.clone(),
                amount: amount.clone(),
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![SecurityWarning {
                    code: "WARN_TOKEN_BURN".into(),
                    severity: WarningSeverity::AmberGate,
                    title: "Permanent token burn".into(),
                    message: format!(
                        "Permanently destroys {amount} {tlabel} held by {}.",
                        short(from)
                    ),
                }],
                requires_unverified_ack,
                requires_critical_override: false,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
        TxIntent::TokenSetAuthority { token_id, authority, new_authority } => {
            let (verification_badge, requires_unverified_ack) =
                build_program_badge("lezer_token", program_id, registry_entry);
            let tlabel = token_label(token_id, ctx.token_info.as_ref());
            let (headline, target_str, warning) = match new_authority {
                Some(next) => (
                    format!("Transfer mint authority of {tlabel} from {} to {}", short(authority), short(next)),
                    next.clone(),
                    SecurityWarning {
                        code: "WARN_TOKEN_AUTHORITY_TRANSFER".into(),
                        severity: WarningSeverity::CriticalRedOverride,
                        title: "Critical: mint authority transfer".into(),
                        message: format!(
                            "Transfers full minting and authority control of {tlabel} to {next}. You will no longer control this token's supply."
                        ),
                    },
                ),
                None => (
                    format!("Permanently renounce mint authority of {tlabel}"),
                    "renounced (None)".to_string(),
                    SecurityWarning {
                        code: "WARN_TOKEN_AUTHORITY_RENOUNCE".into(),
                        severity: WarningSeverity::CriticalRedOverride,
                        title: "Critical: permanent authority renouncement".into(),
                        message: format!(
                            "Permanently locks the supply of {tlabel}. No account will ever be able to mint additional tokens."
                        ),
                    },
                ),
            };
            TransactionSummary {
                proposal_id: proposal_id.to_string(),
                originating_dapp: dapp.to_string(),
                signing_account: authority.clone(),
                privacy_mode: "public".into(),
                requires_zk_proof: false,
                instruction_name: "lezer_token::SetAuthority".into(),
                headline,
                from: authority.clone(),
                to: target_str,
                amount: "0".into(),
                max_fee: max_fee_str,
                precheck,
                verification_badge,
                warnings: vec![warning],
                requires_unverified_ack,
                requires_critical_override: true,
                summary_digest_hex: String::new(),
            }
            .seal_digest()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_modes_and_digest_detects_tampering() {
        assert_eq!(privacy_mode("Public/a", "Public/b"), "public");
        assert_eq!(privacy_mode("Public/a", "lezer:priv1xyz"), "shielded");
        assert_eq!(privacy_mode("Private/a", "Public/b"), "deshielded");
        assert_eq!(privacy_mode("Private/a", "Private/b"), "private");

        let intent = TxIntent::NativeTransfer { from: "Public/a".into(), to: "Public/b".into(), amount: "5".into() };
        let ctx = SummarizeContext {
            native_token_program_id: "NT".into(),
            max_fee: Some(100),
            ..Default::default()
        };
        let s = summarize("p1", "app", &intent, Precheck::Passed { available: "9".into() }, &ctx);
        assert!(s.digest_is_valid());
        let mut tampered = s.clone();
        tampered.amount = "500".into();
        assert!(!tampered.digest_is_valid());
    }

    #[test]
    fn registry_entry_is_known_but_never_claimed_as_verified_source() {
        let intent = TxIntent::SubmitTestimonial {
            author: "Public/author1".into(),
            username: "alice".into(),
            text: "Lezer works!".into(),
        };
        let ctx_absent = SummarizeContext {
            target_program_id: Some("Public/prog1".into()),
            registry_entry: None,
            ..Default::default()
        };
        let s_absent = summarize("p1", "dapp", &intent, Precheck::Passed { available: "100".into() }, &ctx_absent);
        assert_eq!(s_absent.verification_badge.state, VerificationBadgeState::NoRegistryEntry);
        assert!(s_absent.requires_unverified_ack);

        let ctx_registered = SummarizeContext {
            target_program_id: Some("Public/prog1".into()),
            registry_entry: Some(RegistryEntry {
                program_id: "Public/prog1".into(),
                name: "Lezer Testimonial".into(),
                source_url: "https://github.com/zLabsHQ/lezer-wallet".into(),
                commit: "1a003bfb998877".into(),
                version: "0.3.1".into(),
            }),
            ..Default::default()
        };
        let s = summarize("p2", "dapp", &intent, Precheck::Passed { available: "100".into() }, &ctx_registered);
        assert_eq!(s.verification_badge.state, VerificationBadgeState::KnownButUnverified);
        assert!(s.verification_badge.label.contains("Lezer Testimonial 0.3.1"));
        assert!(s.verification_badge.label.contains("not verified"));
        assert!(s.requires_unverified_ack);
        assert!(s.digest_is_valid());
    }

    #[test]
    fn token_intents_and_faucet_claim_summarize_cleanly_with_critical_gates() {
        let ctx = SummarizeContext {
            target_program_id: Some("Public/tokprog".into()),
            registry_entry: Some(RegistryEntry {
                program_id: "Public/tokprog".into(),
                name: "Lezer Token".into(),
                source_url: "https://github.com/zLabsHQ/lezer-wallet".into(),
                commit: "abcdef123456".into(),
                version: "0.3.1".into(),
            }),
            token_info: Some(TokenInfo {
                token_id: "Public/usdx".into(),
                name: "USD Coin".into(),
                symbol: "USDX".into(),
                decimals: 6,
                authority: Some("Public/auth".into()),
                total_supply: "1000000".into(),
            }),
            max_fee: Some(100),
            ..Default::default()
        };

        let xfer = TxIntent::TokenTransfer {
            token_id: "Public/usdx".into(),
            from: "Public/alice".into(),
            to: "Public/bob".into(),
            amount: "250".into(),
        };
        let s_xfer = summarize("p3", "dex", &xfer, Precheck::Passed { available: "1000".into() }, &ctx);
        assert!(s_xfer.headline.contains("USDX"));
        assert_eq!(s_xfer.verification_badge.state, VerificationBadgeState::KnownButUnverified);
        assert!(!s_xfer.requires_critical_override);

        let renounce = TxIntent::TokenSetAuthority {
            token_id: "Public/usdx".into(),
            authority: "Public/auth".into(),
            new_authority: None,
        };
        let s_renounce = summarize("p4", "dex", &renounce, Precheck::Passed { available: "1000".into() }, &ctx);
        assert!(s_renounce.requires_critical_override);
        assert_eq!(s_renounce.warnings[0].severity, WarningSeverity::CriticalRedOverride);

        let claim = TxIntent::FaucetClaim { recipient: "Public/alice".into() };
        let s_claim = summarize("p5", "faucet", &claim, Precheck::Passed { available: "0".into() }, &ctx);
        assert_eq!(s_claim.instruction_name, "lezer_faucet::Claim");
        assert_eq!(s_claim.amount, DEFAULT_DRIP_AMOUNT.to_string());
    }
}
