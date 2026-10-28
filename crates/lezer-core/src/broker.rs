//! Session Manager & Approval Broker (`crates/lezer-core/src/broker.rs`).
//!
//! Enforces:
//! - Stateful per-dApp capability grants (`SessionGrant` with `lzr_sess_<hex>` bearer tokens).
//! - Strict private-balance gate: calling `check_balance_access` on a `Private/<id>` account
//!   without `PermissionScope::ReadPrivateBalance { account_id }` returns
//!   `LZR-2003: PrivateBalanceForbidden`.
//! - 5-step immutable approval pipeline: `propose_transaction` queues a `PendingProposal`
//!   with its `TransactionSummary` and `summary_digest_hex`, and `verify_approve_decision`
//!   verifies the returned `ApproveDecision` matches `summary_digest_hex` (`LZR-3005` on mismatch)
//!   and satisfies any unverified/critical risk gates (`LZR-3004`).

use std::collections::BTreeMap;

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::intent::{SummarizeContext, TxIntent, summarize};
use crate::errors::{ErrorCode, LezerError, LezerResult};
use crate::summary::{ApproveDecision, Precheck, TransactionSummary};
use crate::vault::{PermissionScope, SessionGrant};

/// Default dApp session TTL: 24 hours (86,400 seconds).
pub const DEFAULT_SESSION_TTL_SECS: u64 = 24 * 60 * 60;

/// Metadata identifying a connecting Basecamp dApp module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DappConnectRequest {
    pub request_id: String,
    pub dapp_name: String,
    pub dapp_origin: String,
    pub request_private_balance_read: bool,
    pub created_at: u64,
}

/// Queued transaction proposal awaiting user approval in `lezer_wallet_ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingProposal {
    pub proposal_id: String,
    pub session_token: String,
    pub dapp_name: String,
    pub intent: TxIntent,
    pub summary: TransactionSummary,
    pub created_at: u64,
}

/// Resolution state of a connection or transaction proposal request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RequestResolution {
    Pending {
        request_id: String,
        kind: String,
    },
    ApprovedConnect {
        request_id: String,
        session_token: String,
        authorized_accounts: Vec<String>,
    },
    ApprovedProposal {
        request_id: String,
        tx_hash: String,
        result: serde_json::Value,
    },
    Rejected {
        request_id: String,
        code: String,
        reason: String,
    },
}

/// Stateful Session & Approval Broker owned by `WalletEngine`.
#[derive(Debug, Default, Clone)]
pub struct ApprovalBroker {
    next_id: u64,
    pending_connects: BTreeMap<String, DappConnectRequest>,
    pending_proposals: BTreeMap<String, PendingProposal>,
    resolutions: BTreeMap<String, RequestResolution>,
}

impl ApprovalBroker {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate_request_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}_{}", self.next_id)
    }

    pub fn pending_connects(&self) -> &BTreeMap<String, DappConnectRequest> {
        &self.pending_connects
    }

    pub fn pending_proposals(&self) -> &BTreeMap<String, PendingProposal> {
        &self.pending_proposals
    }

    pub fn get_resolution(&self, request_id: &str) -> Option<&RequestResolution> {
        self.resolutions.get(request_id)
    }

    /// Queue a new dApp connection prompt (`connect` LIDL method).
    pub fn queue_connect_request(
        &mut self,
        dapp_name: &str,
        dapp_origin: &str,
        request_private_balance_read: bool,
        now_unix: u64,
    ) -> DappConnectRequest {
        let request_id = self.allocate_request_id("conn");
        let req = DappConnectRequest {
            request_id: request_id.clone(),
            dapp_name: dapp_name.to_string(),
            dapp_origin: dapp_origin.to_string(),
            request_private_balance_read,
            created_at: now_unix,
        };
        self.pending_connects.insert(request_id.clone(), req.clone());
        self.resolutions.insert(
            request_id.clone(),
            RequestResolution::Pending {
                request_id,
                kind: "connect".to_string(),
            },
        );
        req
    }

    /// Approve a pending connection request, minting a cryptographic `SessionGrant`.
    pub fn approve_connect(
        &mut self,
        request_id: &str,
        authorized_accounts: Vec<String>,
        allow_private_balances: &[String],
        now_unix: u64,
    ) -> LezerResult<SessionGrant> {
        let req = self.pending_connects.remove(request_id).ok_or_else(|| {
            LezerError::new(
                ErrorCode::ProposalExpired,
                format!("no pending connect request with id '{request_id}'"),
            )
        })?;

        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let session_token = format!("lzr_sess_{}", hex::encode(token_bytes));

        let mut scopes = Vec::new();
        for acct in &authorized_accounts {
            scopes.push(PermissionScope::AccountUse {
                account_id: acct.clone(),
            });
            scopes.push(PermissionScope::ProposeTransaction {
                account_id: acct.clone(),
            });
            if acct.starts_with("Public/") {
                scopes.push(PermissionScope::ReadPublicBalance {
                    account_id: acct.clone(),
                });
            } else if acct.starts_with("Private/") {
                scopes.push(PermissionScope::ReadPrivateReceiveBundle {
                    account_id: acct.clone(),
                });
                if allow_private_balances.iter().any(|a| a == acct) {
                    scopes.push(PermissionScope::ReadPrivateBalance {
                        account_id: acct.clone(),
                    });
                }
            }
        }

        let grant = SessionGrant {
            session_token: session_token.clone(),
            dapp_name: req.dapp_name,
            dapp_origin: req.dapp_origin,
            authorized_accounts: authorized_accounts.clone(),
            scopes,
            created_at: now_unix,
            expires_at: now_unix + DEFAULT_SESSION_TTL_SECS,
        };

        self.resolutions.insert(
            request_id.to_string(),
            RequestResolution::ApprovedConnect {
                request_id: request_id.to_string(),
                session_token,
                authorized_accounts,
            },
        );

        Ok(grant)
    }

    /// Queue a decoded transaction proposal for user review (`propose_transaction` LIDL method).
    pub fn queue_proposal(
        &mut self,
        session: &SessionGrant,
        intent: TxIntent,
        precheck: Precheck,
        ctx: SummarizeContext,
        now_unix: u64,
    ) -> LezerResult<PendingProposal> {
        if now_unix >= session.expires_at {
            return Err(LezerError::new(ErrorCode::SessionExpiredOrInvalid, "session token has expired"));
        }
        match &intent {
            TxIntent::NativeTransfer { .. }
            | TxIntent::TokenTransfer { .. }
            | TxIntent::TokenMint { .. }
            | TxIntent::TokenBurn { .. }
                if intent.amount().is_none_or(|a| a == 0) =>
            {
                return Err(LezerError::new(ErrorCode::SimulationReverted, "amount must be a positive integer"));
            }
            _ => {}
        }
        if let Some(prog_name) = intent.program_binary_name()
            && ctx.target_program_id.is_none()
        {
            return Err(LezerError::new(
                ErrorCode::ProgramNotDeployed,
                format!("program '{prog_name}' is not deployed on this network"),
            ));
        }
        let signing_account = intent.signing_account();
        let has_propose_scope = session.scopes.iter().any(|s| {
            matches!(
                s,
                PermissionScope::ProposeTransaction { account_id } if account_id == signing_account
            )
        });
        if !has_propose_scope {
            return Err(LezerError::new(
                ErrorCode::AccountNotAuthorized,
                format!(
                    "session '{}' is not authorized to propose transactions for account '{}'",
                    session.dapp_name, signing_account
                ),
            ));
        }

        let proposal_id = self.allocate_request_id("prop");
        let summary = summarize(
            &proposal_id,
            &session.dapp_name,
            &intent,
            precheck,
            &ctx,
        );

        let proposal = PendingProposal {
            proposal_id: proposal_id.clone(),
            session_token: session.session_token.clone(),
            dapp_name: session.dapp_name.clone(),
            intent,
            summary,
            created_at: now_unix,
        };

        self.pending_proposals
            .insert(proposal_id.clone(), proposal.clone());
        self.resolutions.insert(
            proposal_id.clone(),
            RequestResolution::Pending {
                request_id: proposal_id,
                kind: "proposal".to_string(),
            },
        );

        Ok(proposal)
    }

    /// Verify an `ApproveDecision` returned by the UI against the queued `PendingProposal`:
    /// - Rejects `LZR-3005: ProposalDigestMismatch` if `summary_digest_hex` does not match.
    /// - Rejects `LZR-3003: SimulationReverted` if pre-flight simulation deterministically reverted.
    /// - Rejects `LZR-3004: UnacknowledgedRiskGate` if unverified or critical override flags are missing.
    pub fn take_verified_proposal(&mut self, decision: &ApproveDecision) -> LezerResult<PendingProposal> {
        let proposal = self
            .pending_proposals
            .get(&decision.proposal_id)
            .cloned()
            .ok_or_else(|| {
                LezerError::new(
                    ErrorCode::ProposalExpired,
                    format!("no pending proposal with id '{}'", decision.proposal_id),
                )
            })?;

        if decision.summary_digest_hex != proposal.summary.summary_digest_hex
            || !proposal.summary.digest_is_valid()
        {
            return Err(LezerError::new(
                ErrorCode::ProposalDigestMismatch,
                format!(
                    "ApproveDecision summary_digest_hex '{}' does not match queued proposal digest '{}'",
                    decision.summary_digest_hex, proposal.summary.summary_digest_hex
                ),
            ));
        }

        if proposal.summary.blocks_approval() {
            return Err(LezerError::new(
                ErrorCode::SimulationReverted,
                "cannot approve: sender balance does not cover the amount",
            ));
        }

        if proposal.summary.requires_unverified_ack && !decision.acknowledged_unverified {
            return Err(LezerError::new(
                ErrorCode::UnacknowledgedRiskGate,
                "unverified program requires acknowledged_unverified = true",
            ));
        }

        if proposal.summary.requires_critical_override && !decision.critical_override {
            return Err(LezerError::new(
                ErrorCode::UnacknowledgedRiskGate,
                "critical authority/mismatch operation requires critical_override = true",
            ));
        }

        self.pending_proposals.remove(&decision.proposal_id);
        Ok(proposal)
    }

    /// Record successful execution of an approved proposal.
    pub fn record_proposal_executed(
        &mut self,
        proposal_id: &str,
        tx_hash: String,
        result: serde_json::Value,
    ) {
        self.resolutions.insert(
            proposal_id.to_string(),
            RequestResolution::ApprovedProposal {
                request_id: proposal_id.to_string(),
                tx_hash,
                result,
            },
        );
    }

    /// Record that an approved proposal failed to execute.
    pub fn record_proposal_failed(&mut self, proposal_id: &str, err: &LezerError) {
        self.resolutions.insert(
            proposal_id.to_string(),
            RequestResolution::Rejected {
                request_id: proposal_id.to_string(),
                code: err.code.as_str().to_string(),
                reason: err.message.clone(),
            },
        );
    }

    /// Reject a pending connect or proposal request (`LZR-3001: UserRejected`).
    pub fn reject_request(&mut self, request_id: &str, reason: Option<&str>) -> LezerResult<RequestResolution> {
        let existed = self.pending_connects.remove(request_id).is_some()
            || self.pending_proposals.remove(request_id).is_some();
        if !existed {
            return Err(LezerError::new(
                ErrorCode::ProposalExpired,
                format!("no pending request '{request_id}' to reject"),
            ));
        }
        let res = RequestResolution::Rejected {
            request_id: request_id.to_string(),
            code: ErrorCode::UserRejected.as_str().to_string(),
            reason: reason.unwrap_or("Rejected by user").to_string(),
        };
        self.resolutions.insert(request_id.to_string(), res.clone());
        Ok(res)
    }

    /// Cancel all pending proposals belonging to a revoked `session_token`.
    pub fn cancel_proposals_for_session(&mut self, session_token: &str) {
        let ids: Vec<String> = self
            .pending_proposals
            .iter()
            .filter(|(_, p)| p.session_token == session_token)
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.pending_proposals.remove(&id);
            self.resolutions.insert(
                id.clone(),
                RequestResolution::Rejected {
                    request_id: id,
                    code: ErrorCode::SessionExpiredOrInvalid.as_str().to_string(),
                    reason: "session_revoked".to_string(),
                },
            );
        }
    }
}

/// Validate that `session` is permitted to read the balance of `account_id`.
/// Enforces `LZR-2003: PrivateBalanceForbidden` if `account_id` is `Private/<id>` and
/// `PermissionScope::ReadPrivateBalance` was not explicitly granted.
pub fn verify_session_balance_permission(
    session: &SessionGrant,
    account_id: &str,
    now_unix: u64,
) -> LezerResult<()> {
    if now_unix >= session.expires_at {
        return Err(LezerError::new(
            ErrorCode::SessionExpiredOrInvalid,
            "session token has expired",
        ));
    }
    if !session.authorized_accounts.iter().any(|a| a == account_id) {
        return Err(LezerError::new(
            ErrorCode::AccountNotAuthorized,
            format!("account '{account_id}' is not shared with session '{}'", session.dapp_name),
        ));
    }
    if account_id.starts_with("Private/") {
        let allowed = session.scopes.iter().any(|s| {
            matches!(
                s,
                PermissionScope::ReadPrivateBalance { account_id: a } if a == account_id
            )
        });
        if !allowed {
            return Err(LezerError::new(
                ErrorCode::PrivateBalanceForbidden,
                format!(
                    "dApp '{}' does not hold ReadPrivateBalance permission for '{account_id}'",
                    session.dapp_name
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_private_balance_gate_and_proposal_digest_verification() {
        let mut broker = ApprovalBroker::new();
        let req = broker.queue_connect_request("zk_chat_ui", "logos://zk_chat_ui", false, 1000);

        let pub_acct = "Public/BH2zpUcwfB6cTKjYp2N8FFQNYSxjighmDfMYveFCC1Wf".to_string();
        let priv_acct = "Private/9xLm2P8vQa111111111111111111111111111111111".to_string();

        // Grant connection WITHOUT ReadPrivateBalance
        let grant = broker
            .approve_connect(
                &req.request_id,
                vec![pub_acct.clone(), priv_acct.clone()],
                &[],
                1000,
            )
            .unwrap();

        assert!(verify_session_balance_permission(&grant, &pub_acct, 1005).is_ok());
        let priv_err = verify_session_balance_permission(&grant, &priv_acct, 1005).unwrap_err();
        assert_eq!(priv_err.code, ErrorCode::PrivateBalanceForbidden);
        assert_eq!(priv_err.code.as_str(), "LZR-2003");

        // Propose a native transfer
        let proposal = broker
            .queue_proposal(
                &grant,
                TxIntent::NativeTransfer {
                    from: pub_acct.clone(),
                    to: pub_acct,
                    amount: "25".to_string(),
                },
                Precheck::Passed { available: "100".to_string() },
                SummarizeContext {
                    native_token_program_id: lezer_adapter::native_token_program_id(),
                    max_fee: Some(lezer_adapter::DEFAULT_MAX_FEE),
                    ..Default::default()
                },
                1010,
            )
            .unwrap();

        // Tampered digest must fail with LZR-3005
        let bad_decision = ApproveDecision {
            proposal_id: proposal.proposal_id.clone(),
            summary_digest_hex: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            acknowledged_unverified: false,
            critical_override: false,
        };
        let digest_err = broker.take_verified_proposal(&bad_decision).unwrap_err();
        assert_eq!(digest_err.code, ErrorCode::ProposalDigestMismatch);

        // Exact matching digest succeeds
        let good_decision = ApproveDecision {
            proposal_id: proposal.proposal_id.clone(),
            summary_digest_hex: proposal.summary.summary_digest_hex.clone(),
            acknowledged_unverified: false,
            critical_override: false,
        };
        let verified = broker.take_verified_proposal(&good_decision).unwrap();
        assert_eq!(verified.proposal_id, proposal.proposal_id);
    }
}
