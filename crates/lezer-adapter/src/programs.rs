//! Program deployment (official `program_loader`), the LP-0021 testimonial program client,
//! and a wallet-less chain reader used by `verify-adoption`.

use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use borsh::BorshDeserialize as _;
use lee::{AccountId, program::Program};
use lee_core::{account::ProgramShardSelector, program::PdaSeed};
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use serde::{Deserialize, Serialize};
use testimonial_core::{IndexPage, Instruction as TestimonialInstruction, MAX_ENTRIES_PER_PAGE, Submit, Testimonial, bucket_of, page_seed, validate};
use wallet::{AccountIdentity, account::AccountIdWithPrivacy, program_facades::program_loader::ProgramLoader};

use crate::engine::{LezEngine, TransferMode, TxReceipt, TxStatus};
use crate::profile::NetworkProfile;
use crate::rpc::AllowedEndpointsGuard;

/// Same chunk size the official program loader uses.
const SEGMENT_LEN: usize = 96 * 1024;
/// Safety bound when walking index pages.
const MAX_PAGES_PER_BUCKET: u16 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployedProgram {
    /// Program account id (the header account); this is what transactions address.
    pub program_id: String,
    /// RISC0 image id (hex) of the deployed ELF — compare with a reproducible rebuild.
    pub image_id: String,
    pub segments: Vec<String>,
    pub elf_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestimonialEntry {
    pub author: String,
    pub username: String,
    pub text: String,
    pub bucket: u8,
    pub page: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionReport {
    pub sequencer_url: String,
    pub program_id: String,
    pub last_block_id: u64,
    pub total_testimonials: usize,
    pub distinct_authors: usize,
    pub index_pages_read: usize,
    pub testimonials: Vec<TestimonialEntry>,
}

pub fn image_id_hex(elf: &[u8]) -> Result<String> {
    let digest = risc0_binfmt::compute_image_id(elf).map_err(|e| anyhow!("not a RISC0 program binary: {e}"))?;
    Ok(hex::encode(digest.as_bytes()))
}

pub fn index_account(program: &AccountId, bucket: u8, page: u16) -> AccountId {
    AccountId::for_public_pda(program, &PdaSeed::new(page_seed(bucket, page)))
}

impl LezEngine {
    /// Deploy a RISC0 program through the official program loader. Fresh header and segment
    /// accounts are created in this wallet; `payer` (an owned, funded public account) pays.
    pub async fn deploy_program(&mut self, elf: &[u8], payer: &str) -> Result<DeployedProgram> {
        let AccountIdWithPrivacy::Public(payer_id) = self.resolve(payer)? else {
            bail!("the payer must be a public account");
        };
        let image_id = image_id_hex(elf)?;
        let binary = risc0_binfmt::ProgramBinary::decode(elf).map_err(|e| anyhow!("invalid program binary: {e}"))?;
        let n_segments = binary.user_elf.len().div_ceil(SEGMENT_LEN).max(1);
        let header = self.core_mut().create_new_account_public(None).0;
        let segments: Vec<AccountId> = (0..n_segments).map(|_| self.core_mut().create_new_account_public(None).0).collect();
        ProgramLoader(self.core())
            .deploy(header, &segments, elf.to_vec(), true, Some(payer_id))
            .await
            .context("program deployment failed")?;
        Ok(DeployedProgram {
            program_id: header.to_string(),
            image_id,
            segments: segments.iter().map(ToString::to_string).collect(),
            elf_bytes: elf.len(),
        })
    }

    /// Submit an LP-0021 testimonial from `author` (an owned public account that signs).
    /// `payer` covers the fee (may equal `author`). Picks the first index page with room.
    pub async fn submit_testimonial(
        &mut self,
        program_id: &str,
        author: &str,
        payer: &str,
        username: &str,
        text: &str,
    ) -> Result<TxReceipt> {
        let program = crate::engine::parse_public_id(program_id)?;
        let AccountIdWithPrivacy::Public(author_id) = self.resolve(author)? else {
            bail!("testimonials are public: the author must be a public account");
        };
        let AccountIdWithPrivacy::Public(payer_id) = self.resolve(payer)? else {
            bail!("the payer must be a public account");
        };
        let author_bytes = *author_id.value();
        let bucket = bucket_of(&author_bytes);

        // Find a page with room (and make sure this author isn't indexed yet).
        let mut page = 0u16;
        loop {
            let idx = index_account(&program, bucket, page);
            let current = read_index_page(self.core().client(), &program, idx).await?;
            if current.authors.contains(&author_bytes) {
                bail!("{author_id} already submitted a testimonial");
            }
            if current.authors.len() < MAX_ENTRIES_PER_PAGE {
                break;
            }
            page = page.checked_add(1).filter(|p| *p < MAX_PAGES_PER_BUCKET).context("index is full")?;
        }

        let submit = Submit { username: username.trim().to_string(), text: text.trim().to_string(), page };
        validate(&submit).map_err(|e| anyhow!(e))?;
        let instruction = Program::serialize_instruction(TestimonialInstruction::Submit(submit)).map_err(|e| anyhow!("{e:?}"))?;
        let accounts = vec![
            AccountIdentity::Public(author_id).select_program_shard(program),
            AccountIdentity::PublicNoSign(index_account(&program, bucket, page)).select_program_shard(program),
        ];

        let started = Instant::now();
        let tx_hash = self
            .core()
            .send_pub_tx_paid_by(accounts, instruction, program, Some(payer_id))
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
            from: format!("Public/{author_id}"),
            to: format!("program {program}"),
            amount: "0".into(),
            build_ms,
        })
    }
}

async fn read_shard(client: &SequencerClient, account: AccountId, program: &AccountId) -> Result<Vec<u8>> {
    let view = client
        .get_account_view(ProgramShardSelector::new(account, *program))
        .await
        .with_context(|| format!("reading {account}"))?;
    Ok(view.data.shard(*program).to_vec())
}

async fn read_index_page(client: &SequencerClient, program: &AccountId, idx: AccountId) -> Result<IndexPage> {
    let bytes = read_shard(client, idx, program).await?;
    if bytes.is_empty() {
        return Ok(IndexPage::default());
    }
    IndexPage::try_from_slice(&bytes).map_err(|e| anyhow!("index page {idx} is malformed: {e}"))
}

/// Enumerate every testimonial of `program_id` straight from the sequencer, with no wallet,
/// no indexer, and no off-chain list: index PDAs are derived, then read by exact id.
pub async fn verify_adoption(profile: &NetworkProfile, program_id: &str) -> Result<AdoptionReport> {
    AllowedEndpointsGuard::from_profile(profile).check_url(&profile.sequencer_url)?;
    let client = SequencerClientBuilder::default()
        .build(profile.sequencer_url.as_str())
        .context("invalid sequencer URL")?;
    let program = crate::engine::parse_public_id(program_id)?;
    let last_block_id = client.get_last_block_id().await.context("sequencer unreachable")?;

    let mut testimonials = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut pages_read = 0usize;
    for bucket in 0..testimonial_core::BUCKETS {
        for page in 0..MAX_PAGES_PER_BUCKET {
            let index = read_index_page(&client, &program, index_account(&program, bucket, page)).await?;
            pages_read += 1;
            if index.authors.is_empty() {
                break;
            }
            for author in &index.authors {
                let id = AccountId::new(*author);
                if !seen.insert(*author) {
                    continue;
                }
                let raw = read_shard(&client, id, &program).await?;
                let Ok(record) = Testimonial::try_from_slice(&raw) else { continue };
                testimonials.push(TestimonialEntry {
                    author: format!("Public/{id}"),
                    username: record.username,
                    text: record.text,
                    bucket,
                    page,
                });
            }
            if index.authors.len() < MAX_ENTRIES_PER_PAGE {
                break;
            }
        }
    }
    Ok(AdoptionReport {
        sequencer_url: profile.sequencer_url.clone(),
        program_id: format!("Public/{program}"),
        last_block_id,
        total_testimonials: testimonials.len(),
        distinct_authors: seen.len(),
        index_pages_read: pages_read,
        testimonials,
    })
}
