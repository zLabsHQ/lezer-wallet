//! `lezer_faucet` host client: status queries (`faucet_status_rpc`), treasury PDA
//! derivation (`faucet_vault_id`), and on-chain native-LEZ claims (`faucet_claim`) with
//! local pre-check against rate-limit and vault depletion.

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use borsh::BorshDeserialize as _;
use faucet_core::{ClaimRecord, DEFAULT_DRIP_AMOUNT, Instruction, VAULT_SEED, claim_seed};
use lee::{AccountId, program::Program};
use lee_core::{
    account::ProgramShardSelector,
    native_token::{NATIVE_TOKEN_PROGRAM_ID, decode_balance},
    program::PdaSeed,
};
use sequencer_service_rpc::{RpcClient as _, SequencerClient};
use serde::{Deserialize, Serialize};
use wallet::{AccountIdentity, ExecutionFailureKind};

use crate::engine::{LezEngine, TransferMode, TxReceipt, TxStatus, parse_public_id};
use crate::profile::NetworkProfile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaucetStatus {
    pub faucet_program_id: String,
    pub vault_account_id: String,
    pub vault_balance: String,
    pub drip_amount: String,
    pub recipient: Option<String>,
    pub already_claimed: bool,
    pub claimed_amount: String,
}

fn vault_account(program: &AccountId) -> AccountId {
    AccountId::for_public_pda(program, &PdaSeed::new(VAULT_SEED))
}

fn claim_ticket_account(program: &AccountId, recipient: &AccountId) -> AccountId {
    AccountId::for_public_pda(program, &PdaSeed::new(claim_seed(recipient.value())))
}

/// Deterministic `Public/<id>` address of `program_id`'s native-LEZ treasury PDA.
/// Send native LEZ to this address to fund or top up the faucet.
pub fn faucet_vault_id(program_id: &str) -> Result<String> {
    let program = parse_public_id(program_id)?;
    Ok(format!("Public/{}", vault_account(&program)))
}

type ShardRow = (AccountId, AccountId, Vec<u8>);

fn faucet_pre_check(
    program: AccountId,
    vault: AccountId,
    ticket: AccountId,
    shards: &[ShardRow],
) -> Result<(), ExecutionFailureKind> {
    let reject = |msg: &str| {
        Err(ExecutionFailureKind::TransactionBuildError(
            lee::error::LeeError::InvalidInput(msg.to_owned()),
        ))
    };
    let shard_of = |account: AccountId, shard_prog: AccountId| -> Vec<u8> {
        shards
            .iter()
            .find(|(a, p, _)| *a == account && *p == shard_prog)
            .map(|(_, _, d)| d.clone())
            .unwrap_or_default()
    };

    let ticket_bytes = shard_of(ticket, program);
    if !ticket_bytes.is_empty() {
        return reject("faucet rate limit exceeded: account already claimed its allocation");
    }

    let vault_bytes = shard_of(vault, NATIVE_TOKEN_PROGRAM_ID);
    let vault_balance = decode_balance(&vault_bytes).unwrap_or(0);
    if vault_balance < DEFAULT_DRIP_AMOUNT {
        return reject("faucet treasury vault has insufficient funds");
    }
    Ok(())
}

impl LezEngine {
    /// Claim [`DEFAULT_DRIP_AMOUNT`] native LEZ from `program_id` into `recipient` (must be an
    /// owned public account that authorizes the claim). `payer` covers the transaction fee.
    pub async fn faucet_claim(
        &mut self,
        program_id: &str,
        recipient: &str,
        payer: &str,
    ) -> Result<TxReceipt> {
        let program = parse_public_id(program_id)?;
        let wallet::account::AccountIdWithPrivacy::Public(recipient_id) = self.resolve(recipient)? else {
            anyhow::bail!("faucet claims require a public recipient account ('{recipient}')");
        };
        let wallet::account::AccountIdWithPrivacy::Public(payer_id) = self.resolve(payer)? else {
            anyhow::bail!("the fee payer must be a public account ('{payer}')");
        };
        let vault = vault_account(&program);
        let ticket = claim_ticket_account(&program, &recipient_id);
        let accounts = vec![
            AccountIdentity::PublicNoSign(vault).select_program_shard(NATIVE_TOKEN_PROGRAM_ID),
            AccountIdentity::Public(recipient_id).select_program_shard(NATIVE_TOKEN_PROGRAM_ID),
            AccountIdentity::PublicNoSign(ticket).select_program_shard(program),
        ];
        let instruction_data =
            Program::serialize_instruction(Instruction::Claim).map_err(|e| anyhow!("{e:?}"))?;
        let started = Instant::now();
        let tx_hash = self
            .core()
            .send_pub_tx_with_pre_check(
                accounts,
                instruction_data,
                program,
                Some(payer_id),
                move |shards| {
                    let rows: Vec<ShardRow> = shards
                        .iter()
                        .map(|s| {
                            (
                                s.selector.account_id,
                                s.selector.program_account_id,
                                s.data.as_ref().to_vec(),
                            )
                        })
                        .collect();
                    faucet_pre_check(program, vault, ticket, &rows)
                },
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        let build_ms = started.elapsed().as_millis() as u64;
        let polled = Instant::now();
        let status = match self.core().poll_transaction(tx_hash).await {
            Ok((_, block_id)) => TxStatus::Included { block_id },
            Err(e) => TxStatus::Submitted { detail: format!("{e:#}") },
        };
        Ok(TxReceipt {
            tx_hash: tx_hash.to_string(),
            inclusion_ms: matches!(status, TxStatus::Included { .. })
                .then(|| polled.elapsed().as_millis() as u64),
            status,
            mode: TransferMode::Public,
            from: format!("Public/{vault}"),
            to: format!("Public/{recipient_id}"),
            amount: DEFAULT_DRIP_AMOUNT.to_string(),
            build_ms,
        })
    }
}

/// Read faucet vault balance and (optionally) `recipient`'s claim status straight from chain.
pub async fn faucet_status_rpc(
    profile: &NetworkProfile,
    program_id: &str,
    recipient_id: Option<&str>,
) -> Result<FaucetStatus> {
    faucet_status_on(&crate::sequencer_client(profile)?, program_id, recipient_id).await
}

pub async fn faucet_status_on(
    client: &SequencerClient,
    program_id: &str,
    recipient_id: Option<&str>,
) -> Result<FaucetStatus> {
    let program = parse_public_id(program_id)?;
    let vault = vault_account(&program);
    let vault_view = client
        .get_account_view(ProgramShardSelector::native_balance(vault))
        .await
        .with_context(|| format!("reading faucet vault {vault}"))?;
    let vault_balance = decode_balance(vault_view.data.shard(NATIVE_TOKEN_PROGRAM_ID)).unwrap_or(0);

    let (recipient_norm, already_claimed, claimed_amount) = match recipient_id {
        Some(r) if !r.trim().is_empty() => {
            let rec = parse_public_id(r)?;
            let ticket = claim_ticket_account(&program, &rec);
            let ticket_view = client
                .get_account_view(ProgramShardSelector::new(ticket, program))
                .await
                .with_context(|| format!("reading faucet ticket {ticket}"))?;
            let raw = ticket_view.data.shard(program);
            if raw.is_empty() {
                (Some(format!("Public/{rec}")), false, 0u128)
            } else {
                let record = ClaimRecord::try_from_slice(raw)
                    .map_err(|e| anyhow!("malformed faucet claim record: {e}"))?;
                (Some(format!("Public/{rec}")), record.claims > 0, record.total_claimed)
            }
        }
        _ => (None, false, 0u128),
    };

    Ok(FaucetStatus {
        faucet_program_id: format!("Public/{program}"),
        vault_account_id: format!("Public/{vault}"),
        vault_balance: vault_balance.to_string(),
        drip_amount: DEFAULT_DRIP_AMOUNT.to_string(),
        recipient: recipient_norm,
        already_claimed,
        claimed_amount: claimed_amount.to_string(),
    })
}
