//! `lezer_registry` client: `register_self` via chained calls (only a program can register
//! itself) and a wallet-less directory reader for `lezer registry list`.

use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use borsh::BorshDeserialize as _;
use lee::{AccountId, program::Program};
use lee_core::account::ProgramShardSelector;
use lee_core::program::PdaSeed;
use registry_core::{Entry, IndexPage, MAX_PAGES_PER_BUCKET, bucket_of, entry_seed, index_page_seed};
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use serde::{Deserialize, Serialize};
use wallet::AccountIdentity;

use crate::engine::{LezEngine, TransferMode, TxReceipt, TxStatus, parse_public_id};
use crate::profile::NetworkProfile;
use crate::rpc::AllowedEndpointsGuard;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub program_id: String,
    pub name: String,
    pub source_url: String,
    pub commit: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryReport {
    pub registry_program_id: String,
    pub sequencer_url: String,
    pub last_block_id: u64,
    pub total_programs: usize,
    pub index_pages_read: usize,
    pub entries: Vec<RegistryEntry>,
}

fn pda(program: &AccountId, seed: [u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &PdaSeed::new(seed))
}

async fn read_shard(client: &SequencerClient, account: AccountId, program: &AccountId) -> Result<Vec<u8>> {
    let view = client
        .get_account_view(ProgramShardSelector::new(account, *program))
        .await
        .with_context(|| format!("reading {account}"))?;
    Ok(view.data.shard(*program).to_vec())
}

impl LezEngine {
    /// Self-register a program in the registry. `subject_program_id` must be a deployed
    /// program whose guest supports `RegisterSelf`; `subject_ix` is that program's borsh
    /// instruction bytes (each program wraps `RegisterSelfParams` in its own instruction
    /// enum). `page` chooses the index page in the subject's bucket; callers should use
    /// [`first_page_with_room`] to pick it.
    pub async fn register_program_self(
        &mut self,
        registry_id: &str,
        subject_program_id: &str,
        subject_ix: Vec<u8>,
        payer: &str,
        page: u8,
    ) -> Result<TxReceipt> {
        let registry = parse_public_id(registry_id)?;
        let subject = parse_public_id(subject_program_id)?;
        let wallet::account::AccountIdWithPrivacy::Public(payer_id) = self.resolve(payer)? else {
            bail!("the payer must be a public account");
        };
        let subject_bytes = *subject.value();
        let entry = pda(&registry, entry_seed(&subject_bytes));
        let index = pda(&registry, index_page_seed(bucket_of(&subject_bytes), page));

        // The subject's plan derives these same PDAs and re-selects them as registry
        // shards for the chained call — they are mentioned at the registry shard here so
        // the execution state carries the registry data the callee reads.
        let accounts = vec![
            AccountIdentity::PublicNoSign(entry).select_program_shard(registry),
            AccountIdentity::PublicNoSign(index).select_program_shard(registry),
        ];
        let started = Instant::now();
        let tx_hash = self
            .core()
            .send_pub_tx_paid_by(accounts, subject_ix, subject, Some(payer_id))
            .await?;
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
            from: format!("Public/{payer_id}"),
            to: format!("registry {registry}"),
            amount: "0".into(),
            build_ms,
        })
    }
}

/// [`first_page_with_room`] through the network profile (client built internally).
pub async fn first_page_with_room_rpc(profile: &NetworkProfile, registry_id: &str, program_id: &str) -> Result<u8> {
    let client = crate::sequencer_client(profile)?;
    let registry = parse_public_id(registry_id)?;
    let subject = parse_public_id(program_id)?;
    first_page_with_room(&client, &registry, &subject).await
}

/// First index page in `program_id`'s bucket that still has room (or doesn't exist yet).
pub async fn first_page_with_room(client: &SequencerClient, registry: &AccountId, program_id: &AccountId) -> Result<u8> {
    let bucket = bucket_of(program_id.value());
    for page in 0..MAX_PAGES_PER_BUCKET {
        let idx = pda(registry, index_page_seed(bucket, page));
        let raw = read_shard(client, idx, registry).await?;
        if raw.is_empty() {
            return Ok(page);
        }
        let page_data = IndexPage::try_from_slice(&raw).map_err(|e| anyhow!("malformed index page: {e}"))?;
        if page_data.entries.contains(program_id.value()) {
            return Ok(page); // re-registration: already indexed
        }
        if page_data.entries.len() < registry_core::MAX_ENTRIES_PER_PAGE {
            return Ok(page);
        }
    }
    bail!("registry bucket is full");
}

/// List every registered program straight from the sequencer — wallet-less, like
/// `verify_adoption`. Reads 16 buckets × pages until each bucket's first empty page.
pub async fn list_registry(profile: &NetworkProfile, registry_id: &str) -> Result<RegistryReport> {
    AllowedEndpointsGuard::from_profile(profile).check_url(&profile.sequencer_url)?;
    let client = SequencerClientBuilder::default().build(profile.sequencer_url.as_str()).context("invalid sequencer URL")?;
    let registry = parse_public_id(registry_id)?;
    let last_block_id = client.get_last_block_id().await.context("sequencer unreachable")?;

    let mut program_ids = Vec::new();
    let mut pages_read = 0usize;
    for bucket in 0..registry_core::BUCKETS {
        for page in 0..MAX_PAGES_PER_BUCKET {
            let idx = pda(&registry, index_page_seed(bucket, page));
            let raw = read_shard(&client, idx, &registry).await?;
            if raw.is_empty() {
                break; // first missing page ends this bucket
            }
            pages_read += 1;
            let data = IndexPage::try_from_slice(&raw).map_err(|e| anyhow!("malformed index page {bucket}:{page}: {e}"))?;
            program_ids.extend(data.entries);
        }
    }

    let mut entries = Vec::new();
    for pid in program_ids {
        let program_account = AccountId::new(pid);
        let entry_account = pda(&registry, entry_seed(&pid));
        let raw = read_shard(&client, entry_account, &registry).await?;
        if raw.is_empty() {
            continue;
        }
        let e = Entry::try_from_slice(&raw).map_err(|e| anyhow!("malformed entry for {program_account}: {e}"))?;
        entries.push(RegistryEntry {
            program_id: format!("Public/{program_account}"),
            name: e.name,
            source_url: e.source_url,
            commit: e.commit,
            version: e.version,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(RegistryReport {
        registry_program_id: registry_id.to_string(),
        sequencer_url: profile.sequencer_url.clone(),
        last_block_id,
        total_programs: entries.len(),
        index_pages_read: pages_read,
        entries,
    })
}

/// Serialize a `RegisterSelf` instruction for a supported subject program.
pub fn register_self_instruction(
    subject: &str,
    params: registry_core::RegisterSelfParams,
) -> Result<Vec<u8>> {
    let ix = match subject {
        "lezer_testimonial" => Program::serialize_instruction(testimonial_core::Instruction::RegisterSelf(params)),
        "lezer_token" => Program::serialize_instruction(token_core::Instruction::RegisterSelf(params)),
        other => bail!("program '{other}' does not support self-registration (no RegisterSelf instruction)"),
    };
    ix.map_err(|e| anyhow!("{e:?}"))
}
