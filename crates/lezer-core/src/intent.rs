//! Transaction intents proposed by dApps / UI, and their translation into a canonical
//! [`TransactionSummary`].

use lezer_adapter::PRIVATE_RECEIVE_PREFIX;
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
}

impl TxIntent {
    pub fn signing_account(&self) -> &str {
        match self {
            Self::NativeTransfer { from, .. } => from,
            Self::SubmitTestimonial { author, .. } => author,
        }
    }

    /// Native amount moved by the intent (`Some(0)` for program calls).
    pub fn amount(&self) -> Option<u128> {
        match self {
            Self::NativeTransfer { amount, .. } => amount.trim().parse().ok(),
            Self::SubmitTestimonial { .. } => Some(0),
        }
    }
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

/// Build the canonical summary the user approves.
pub fn summarize(
    proposal_id: &str,
    dapp: &str,
    intent: &TxIntent,
    precheck: Precheck,
    native_token_program_id: &str,
    max_fee: Option<u128>,
    testimonial_program_id: Option<&str>,
) -> TransactionSummary {
    let (from, to, amount) = match intent {
        TxIntent::NativeTransfer { from, to, amount } => (from, to, amount),
        TxIntent::SubmitTestimonial { author, username, text } => {
            return summarize_testimonial(proposal_id, dapp, author, username, text, precheck, max_fee, testimonial_program_id);
        }
    };
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
        max_fee: (mode == "public").then(|| max_fee.map(|f| f.to_string())).flatten(),
        precheck,
        verification_badge: SourceVerificationBadge {
            state: VerificationBadgeState::ProtocolBuiltin,
            label: "Built-in LEZ native token program".into(),
            program_name: "native_token".into(),
            program_id: native_token_program_id.to_string(),
        },
        warnings,
        requires_unverified_ack: false,
        requires_critical_override: false,
        summary_digest_hex: String::new(),
    }
    .seal_digest()
}

#[allow(clippy::too_many_arguments)]
fn summarize_testimonial(
    proposal_id: &str,
    dapp: &str,
    author: &str,
    username: &str,
    text: &str,
    precheck: Precheck,
    max_fee: Option<u128>,
    program_id: Option<&str>,
) -> TransactionSummary {
    let preview: String = text.chars().take(120).collect();
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
        max_fee: max_fee.map(|f| f.to_string()),
        precheck,
        verification_badge: SourceVerificationBadge {
            state: VerificationBadgeState::KnownButUnverified,
            label: "Lezer testimonial program (configured for this network; source not registry-verified)".into(),
            program_name: "lezer_testimonial".into(),
            program_id: program_id.unwrap_or_default().to_string(),
        },
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
        requires_unverified_ack: true,
        requires_critical_override: false,
        summary_digest_hex: String::new(),
    }
    .seal_digest()
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
        let s = summarize("p1", "app", &intent, Precheck::Passed { available: "9".into() }, "NT", Some(100), None);
        assert!(s.digest_is_valid());
        let mut tampered = s.clone();
        tampered.amount = "500".into();
        assert!(!tampered.digest_is_valid());
    }
}
