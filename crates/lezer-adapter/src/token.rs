//! `lezer_token` client: define/mint/transfer/burn/rotate-authority through real program
//! calls, plus exact-account balance reads (no local ledger, no indexer).

use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use borsh::BorshDeserialize as _;
use lee::{AccountId, program::Program};
use lee_core::{account::ProgramShardSelector, program::PdaSeed};
use sequencer_service_rpc::RpcClient as _;
use serde::{Deserialize, Serialize};
use token_core::{Holding, Instruction, TokenDefinition, holding_seed, validate_definition};
use wallet::{AccountIdentity, ExecutionFailureKind, account::AccountIdWithPrivacy};

use crate::engine::{LezEngine, TransferMode, TxReceipt, TxStatus, parse_public_id};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Token id = the definition account id.
    pub token_id: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub authority: Option<String>,
    pub total_supply: String,
}

/// Local pre-flight check over the already-fetched shards — mirrors what the guest will
/// assert, so a doomed call is rejected before signing instead of landing as a charged
/// revert the sequencer would still include in a block.
enum PreCheck {
    /// `definition` shard must be empty (no token defined there yet).
    DefShardEmpty { definition: AccountId },
    /// Definition's authority must equal `authority` (mints only).
    MintAuthority { definition: AccountId, authority: AccountId },
    /// `holding` shard balance must cover `amount`.
    Spend { holding: AccountId, amount: u128 },
}

/// Rows extracted from `SelectedShard`s — the type itself lives in a private upstream
/// module, so the closure maps shards to plain `(account, shard bytes)` tuples first.
type ShardRow = (AccountId, Vec<u8>);

fn token_pre_check(check: &PreCheck, shards: &[ShardRow]) -> Result<(), ExecutionFailureKind> {
    let reject = |msg: &str| {
        Err(ExecutionFailureKind::TransactionBuildError(lee::error::LeeError::InvalidInput(msg.to_owned())))
    };
    let shard_of = |account: &AccountId| -> Option<Vec<u8>> {
        shards.iter().find(|s| s.0 == *account).map(|s| s.1.clone())
    };
    match check {
        PreCheck::DefShardEmpty { definition } => match shard_of(definition) {
            Some(data) if !data.is_empty() => reject("account already holds a token definition"),
            _ => Ok(()),
        },
        PreCheck::MintAuthority { definition, authority } => {
            let data = shard_of(definition).unwrap_or_default();
            match TokenDefinition::try_from_slice(&data) {
                Ok(def) if def.authority == Some(*authority.value()) => Ok(()),
                Ok(_) => reject("caller is not the mint authority"),
                Err(_) => reject("not a token definition account"),
            }
        }
        PreCheck::Spend { holding, amount } => {
            let data = shard_of(holding).unwrap_or_default();
            let balance = if data.is_empty() { 0 } else {
                Holding::try_from_slice(&data).map(|h| h.balance).unwrap_or(0)
            };
            if balance >= *amount { Ok(()) } else { reject("insufficient token balance") }
        }
    }
}

fn holding_account(program: &AccountId, token: &AccountId, owner: &AccountId) -> AccountId {
    AccountId::for_public_pda(program, &PdaSeed::new(holding_seed(token.value(), owner.value())))
}

impl LezEngine {
    fn public(&self, selector: &str) -> Result<AccountId> {
        match self.resolve(selector)? {
            AccountIdWithPrivacy::Public(id) => Ok(id),
            _ => bail!("'{selector}' is not a public account"),
        }
    }

    async fn call_program(
        &mut self,
        program: AccountId,
        accounts: Vec<wallet::AccountMention>,
        instruction: Instruction,
        payer: AccountId,
        check: PreCheck,
    ) -> Result<TxReceipt> {
        let instruction_data = Program::serialize_instruction(instruction).map_err(|e| anyhow!("{e:?}"))?;
        let started = Instant::now();
        let tx_hash = self
            .core()
            .send_pub_tx_with_pre_check(accounts, instruction_data, program, Some(payer), move |shards| {
                let rows: Vec<ShardRow> =
                    shards.iter().map(|s| (s.selector.account_id, s.data.as_ref().to_vec())).collect();
                token_pre_check(&check, &rows)
            })
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
            inclusion_ms: matches!(status, TxStatus::Included { .. }).then(|| polled.elapsed().as_millis() as u64),
            status,
            mode: TransferMode::Public,
            from: format!("Public/{payer}"),
            to: format!("program {program}"),
            amount: "0".into(),
            build_ms,
        })
    }

    /// Define a token. `definition` is a fresh public account owned by this wallet (its id
    /// becomes the token id; it must sign). `authority` controls minting; `None` = fixed supply.
    #[allow(clippy::too_many_arguments)]
    pub async fn token_define(
        &mut self,
        program_id: &str,
        definition: &str,
        payer: &str,
        name: &str,
        symbol: &str,
        decimals: u8,
        authority: Option<&str>,
        initial_supply: u128,
    ) -> Result<TokenInfo> {
        let program = parse_public_id(program_id)?;
        let def_id = self.public(definition)?;
        let payer_id = self.public(payer)?;
        let authority_id = match authority {
            Some(a) => Some(*self.public(a)?.value()),
            None => None,
        };
        let creator = authority_id.unwrap_or(*def_id.value());
        let holding = holding_account(&program, &def_id, &AccountId::new(creator));
        validate_definition(name, symbol, decimals).map_err(|e| anyhow!(e))?;
        let accounts = vec![
            AccountIdentity::Public(def_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(holding).select_program_shard(program),
        ];
        let instruction = Instruction::Define {
            name: name.trim().into(),
            symbol: symbol.trim().into(),
            decimals,
            authority: authority_id,
            initial_supply,
        };
        self.call_program(program, accounts, instruction, payer_id, PreCheck::DefShardEmpty { definition: def_id }).await?;
        Ok(TokenInfo {
            token_id: format!("Public/{def_id}"),
            name: name.trim().into(),
            symbol: symbol.trim().into(),
            decimals,
            authority: authority_id.map(|a| format!("Public/{}", AccountId::new(a))),
            total_supply: initial_supply.to_string(),
        })
    }

    /// Read a token definition from chain.
    pub async fn token_info(&self, program_id: &str, token_id: &str) -> Result<TokenInfo> {
        token_info_on(self.core().client(), program_id, token_id).await
    }

    /// Token balance of `owner` read straight from the holding account's shard.
    pub async fn token_balance(&self, program_id: &str, token_id: &str, owner: &str) -> Result<u128> {
        parse_public_id(program_id)?; // validate the id shape; reads go through the client
        let owner_id = self.public(owner)?;
        token_balance_on(self.core().client(), program_id, token_id, &owner_id).await
    }

    /// Mint to `owner`'s holding; the current authority signs and `payer` pays the fee.
    pub async fn token_mint(
        &mut self,
        program_id: &str,
        token_id: &str,
        authority: &str,
        owner: &str,
        payer: &str,
        amount: u128,
    ) -> Result<TxReceipt> {
        let program = parse_public_id(program_id)?;
        let def_id = parse_public_id(token_id)?;
        let authority_id = self.public(authority)?;
        let owner_id = self.public(owner)?;
        let payer_id = self.public(payer)?;
        let holding = holding_account(&program, &def_id, &owner_id);
        let accounts = vec![
            AccountIdentity::Public(authority_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(def_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(holding).select_program_shard(program),
        ];
        self.call_program(
            program, accounts,
            Instruction::Mint { owner: *owner_id.value(), amount },
            payer_id,
            PreCheck::MintAuthority { definition: def_id, authority: authority_id },
        ).await
    }

    /// Transfer tokens the caller owns; `owner` signs, `payer` pays.
    pub async fn token_transfer(
        &mut self,
        program_id: &str,
        token_id: &str,
        owner: &str,
        to_owner: &str,
        payer: &str,
        amount: u128,
    ) -> Result<TxReceipt> {
        let program = parse_public_id(program_id)?;
        let def_id = parse_public_id(token_id)?;
        let owner_id = self.public(owner)?;
        let to_id = self.public(to_owner)?;
        let payer_id = self.public(payer)?;
        let accounts = vec![
            AccountIdentity::Public(owner_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(def_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(holding_account(&program, &def_id, &owner_id)).select_program_shard(program),
            AccountIdentity::PublicNoSign(holding_account(&program, &def_id, &to_id)).select_program_shard(program),
        ];
        self.call_program(
            program, accounts,
            Instruction::Transfer { to_owner: *to_id.value(), amount },
            payer_id,
            PreCheck::Spend { holding: holding_account(&program, &def_id, &owner_id), amount },
        ).await
    }

    /// Burn tokens the caller owns.
    pub async fn token_burn(
        &mut self,
        program_id: &str,
        token_id: &str,
        owner: &str,
        payer: &str,
        amount: u128,
    ) -> Result<TxReceipt> {
        let program = parse_public_id(program_id)?;
        let def_id = parse_public_id(token_id)?;
        let owner_id = self.public(owner)?;
        let payer_id = self.public(payer)?;
        let accounts = vec![
            AccountIdentity::Public(owner_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(def_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(holding_account(&program, &def_id, &owner_id)).select_program_shard(program),
        ];
        self.call_program(
            program, accounts,
            Instruction::Burn { amount },
            payer_id,
            PreCheck::Spend { holding: holding_account(&program, &def_id, &owner_id), amount },
        ).await
    }

    /// Rotate the mint authority (`new_authority` = `None` renounces permanently).
    pub async fn token_set_authority(
        &mut self,
        program_id: &str,
        token_id: &str,
        authority: &str,
        payer: &str,
        new_authority: Option<&str>,
    ) -> Result<TxReceipt> {
        let program = parse_public_id(program_id)?;
        let def_id = parse_public_id(token_id)?;
        let authority_id = self.public(authority)?;
        let payer_id = self.public(payer)?;
        let new_id = match new_authority {
            Some(a) => Some(*self.public(a)?.value()),
            None => None,
        };
        let accounts = vec![
            AccountIdentity::Public(authority_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(def_id).select_program_shard(program),
        ];
        self.call_program(
            program, accounts,
            Instruction::SetAuthority { new_authority: new_id },
            payer_id,
            PreCheck::MintAuthority { definition: def_id, authority: authority_id },
        ).await
    }
}

/// Read a token definition straight from the sequencer — wallet-less, like `verify_adoption`.
pub async fn token_info_rpc(profile: &crate::profile::NetworkProfile, program_id: &str, token_id: &str) -> Result<TokenInfo> {
    token_info_on(&crate::sequencer_client(profile)?, program_id, token_id).await
}

/// Token balance of any `Public/..` owner — wallet-less exact-account read.
pub async fn token_balance_rpc(profile: &crate::profile::NetworkProfile, program_id: &str, token_id: &str, owner: &AccountId) -> Result<u128> {
    token_balance_on(&crate::sequencer_client(profile)?, program_id, token_id, owner).await
}

/// Read a token definition straight from the sequencer — wallet-less, like `verify_adoption`.
pub async fn token_info_on(client: &sequencer_service_rpc::SequencerClient, program_id: &str, token_id: &str) -> Result<TokenInfo> {
    let program = parse_public_id(program_id)?;
    let def_id = parse_public_id(token_id)?;
    let view = client
        .get_account_view(ProgramShardSelector::new(def_id, program))
        .await
        .with_context(|| format!("reading {def_id}"))?;
    let raw = view.data.shard(program).to_vec();
    if raw.is_empty() {
        bail!("no token defined on {token_id}");
    }
    let def = TokenDefinition::try_from_slice(&raw).map_err(|e| anyhow!("malformed definition: {e}"))?;
    Ok(TokenInfo {
        token_id: token_id.to_string(),
        name: def.name,
        symbol: def.symbol,
        decimals: def.decimals,
        authority: def.authority.map(|a| format!("Public/{}", AccountId::new(a))),
        total_supply: def.total_supply.to_string(),
    })
}

/// Token balance of a `Public/..` owner — wallet-less exact-account read.
pub async fn token_balance_on(client: &sequencer_service_rpc::SequencerClient, program_id: &str, token_id: &str, owner: &AccountId) -> Result<u128> {
    let program = parse_public_id(program_id)?;
    let def_id = parse_public_id(token_id)?;
    let holding = holding_account(&program, &def_id, owner);
    let view = client
        .get_account_view(ProgramShardSelector::new(holding, program))
        .await
        .with_context(|| format!("reading holding of {owner}"))?;
    let raw = view.data.shard(program).to_vec();
    if raw.is_empty() {
        return Ok(0);
    }
    let h = Holding::try_from_slice(&raw).map_err(|e| anyhow!("malformed holding: {e}"))?;
    if h.definition != *def_id.value() {
        bail!("holding account belongs to a different token");
    }
    Ok(h.balance)
}
