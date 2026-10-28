//! `lezer` — command-line wallet for the Logos Execution Zone.
//!
//! Every command runs against the official upstream engine via `lezer-core`. The vault is
//! unlocked per invocation with `LEZER_PASSWORD` (non-interactive) or a hidden prompt.
//! Upstream libraries print progress to stdout; we redirect fd 1 to stderr and write our
//! result to the original stdout, so `--json` output is always clean.

use std::io::Write as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use lezer_adapter::{AccountKind, LezRpcClient, NetworkProfile, TxReceipt, TxStatus, peak_rss_bytes};
use lezer_core::{ErrorCode, LezerError, LezerResult, WalletEngine, default_lezer_home};
use serde_json::{Value, json};
use zeroize::Zeroizing;

#[derive(Parser, Debug)]
#[command(name = "lezer", version, about = "Lezer — wallet for the Logos Execution Zone (LEZ)")]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// Wallet directory (vault.enc, cache.json, logs). Default: $LEZER_HOME or ~/.lezer
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Network: `testnet`, `localnet`, or a sequencer URL. Default: network stored in the vault.
    #[arg(long, global = true)]
    network: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new wallet and print its 24-word recovery phrase once.
    Init,
    /// Restore a wallet from a recovery phrase (read from stdin or a hidden prompt).
    Restore {
        /// Scan the chain for previously used accounts up to this key-tree depth.
        #[arg(long)]
        discover_depth: Option<u32>,
    },
    /// Wallet snapshot and chain status.
    Status,
    /// Sequencer reachability and version check (no wallet needed).
    Network,
    /// Account management.
    Account {
        #[command(subcommand)]
        sub: AccountCmd,
    },
    /// Native balance of an account (label or id).
    Balance {
        #[arg(default_value = "main")]
        account: String,
    },
    /// Scan new blocks for incoming private notes.
    Sync,
    /// Deploy a RISC0 program through the official program loader.
    Program {
        #[command(subcommand)]
        sub: ProgramCmd,
    },
    /// LP-0021 on-chain testimonials.
    Testimonial {
        #[command(subcommand)]
        sub: TestimonialCmd,
    },
    /// LP-0023 program registry: read the directory or self-register a program.
    Registry {
        #[command(subcommand)]
        sub: RegistryCmd,
    },
    /// List every testimonial straight from the sequencer (no wallet, no indexer).
    VerifyAdoption {
        /// Testimonial program id; default: $LEZER_TESTIMONIAL_PROGRAM.
        #[arg(long)]
        program: Option<String>,
    },
    /// Fungible tokens via the deployed `lezer_token` program (real on-chain state).
    Token {
        #[command(subcommand)]
        sub: TokenCmd,
    },
    /// LP-0021 on-chain native LEZ faucet (`lezer_faucet` program).
    Faucet {
        #[command(subcommand)]
        sub: FaucetCmd,
    },
    /// Send native tokens. Mode (public/shielded/deshielded/private) follows from the accounts.
    Send {
        #[arg(long)]
        from: String,
        /// Label, `Public/..`, owned `Private/..`, or a `lezer:priv1…` receive address.
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
}

#[derive(Subcommand, Debug)]
enum AccountCmd {
    /// Create an account.
    New {
        #[arg(value_enum)]
        kind: Kind,
        #[arg(long)]
        label: Option<String>,
    },
    /// List accounts.
    List,
    /// Import a public account; the 32-byte hex signing key is read from stdin or a hidden prompt.
    Import {
        #[arg(long)]
        label: Option<String>,
    },
    /// Add a label to an account.
    Label { account: String, label: String },
    /// Print the receive address of a private account.
    Receive { account: String },
    /// Set the active account.
    Use { account: String },
}

#[derive(Subcommand, Debug)]
enum ProgramCmd {
    /// Deploy a program binary (`.bin` from `cargo risczero build`).
    Deploy {
        elf: PathBuf,
        /// Funded public account that pays the deployment fees.
        #[arg(long, default_value = "main")]
        payer: String,
    },
    /// Print the RISC0 image id of a program binary (compare with a deployment).
    ImageId { elf: PathBuf },
}

#[derive(Subcommand, Debug)]
enum RegistryCmd {
    /// List every registered program straight from the sequencer.
    List {
        /// Registry program id; default: deployment recorded as `lezer_registry`.
        #[arg(long)]
        program: Option<String>,
    },
    /// Register a deployed program in the registry via its RegisterSelf chained call.
    Register {
        /// Deployed program name (e.g. `lezer_testimonial`, `lezer_token`).
        #[arg(long)]
        subject: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        source_url: String,
        #[arg(long, default_value = "")]
        commit: String,
        #[arg(long, default_value = "")]
        version: String,
        #[arg(long, default_value = "main")]
        payer: String,
        /// Registry program id; default: deployment recorded as `lezer_registry`.
        #[arg(long)]
        program: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TokenCmd {
    /// Define a new token. `--definition` is a fresh public account you own; its id becomes the token id.
    Define {
        /// Fresh public account that hosts the definition shard.
        #[arg(long)]
        definition: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        symbol: String,
        #[arg(long, default_value_t = 9)]
        decimals: u8,
        #[arg(long)]
        supply: u128,
        /// Mint authority; omit for a fixed-supply token (supply minted to the definition account).
        #[arg(long)]
        authority: Option<String>,
        #[arg(long, default_value = "main")]
        payer: String,
        /// lezer_token program id; default: deployment recorded by `lezer program deploy`.
        #[arg(long)]
        program: Option<String>,
    },
    /// Read a token definition from chain.
    Info {
        #[arg(long)]
        token: String,
        #[arg(long)]
        program: Option<String>,
    },
    /// Token balance of an account (label or Public/.. id).
    Balance {
        #[arg(long)]
        token: String,
        #[arg(long, default_value = "main")]
        owner: String,
        #[arg(long)]
        program: Option<String>,
    },
    /// Mint to an owner's holding; the mint authority signs.
    Mint {
        #[arg(long)]
        token: String,
        #[arg(long)]
        authority: String,
        /// Owner that receives the minted amount (label or Public/..).
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
        #[arg(long, default_value = "main")]
        payer: String,
        #[arg(long)]
        program: Option<String>,
    },
    /// Transfer tokens between owners; the sender signs.
    Transfer {
        #[arg(long)]
        token: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
        #[arg(long, default_value = "main")]
        payer: String,
        #[arg(long)]
        program: Option<String>,
    },
    /// Burn tokens the caller owns.
    Burn {
        #[arg(long)]
        token: String,
        #[arg(long, default_value = "main")]
        from: String,
        #[arg(long)]
        amount: u128,
        #[arg(long, default_value = "main")]
        payer: String,
        #[arg(long)]
        program: Option<String>,
    },
    /// Rotate the mint authority; `none` renounces it permanently.
    SetAuthority {
        #[arg(long)]
        token: String,
        #[arg(long)]
        authority: String,
        /// New authority account, or `none`.
        #[arg(long)]
        new: String,
        #[arg(long, default_value = "main")]
        payer: String,
        #[arg(long)]
        program: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum FaucetCmd {
    /// Print the faucet treasury PDA (`Public/..`) to send native LEZ into when funding the faucet.
    Vault {
        #[arg(long)]
        program: Option<String>,
    },
    /// Query faucet treasury balance and (optionally) whether an account has already claimed.
    Status {
        #[arg(long)]
        recipient: Option<String>,
        #[arg(long)]
        program: Option<String>,
    },
    /// Claim 50 LEZ from the on-chain faucet into `--recipient` (enforces 1-per-account rate limit).
    Claim {
        #[arg(long, default_value = "main")]
        recipient: String,
        /// Optional fee payer (defaults to `--recipient` if funded, or any funded public account).
        #[arg(long)]
        payer: Option<String>,
        #[arg(long)]
        program: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TestimonialCmd {
    /// Publish a testimonial (public, permanent, linked to the author account).
    Submit {
        #[arg(long)]
        author: String,
        /// Account that pays the fee (default: the author).
        #[arg(long)]
        payer: Option<String>,
        #[arg(long, default_value = "")]
        username: String,
        #[arg(long)]
        text: String,
        /// Testimonial program id; default: $LEZER_TESTIMONIAL_PROGRAM.
        #[arg(long)]
        program: Option<String>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Kind {
    Public,
    Private,
}

impl From<Kind> for AccountKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Public => Self::Public,
            Kind::Private => Self::Private,
        }
    }
}

/// Writes results to the original stdout after fd 1 was redirected to stderr.
struct Output {
    out: Box<dyn std::io::Write>,
    json: bool,
}

impl Output {
    fn setup(json: bool) -> Self {
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd as _;
            // SAFETY: plain fd duplication; the duplicated fd is owned by the File below.
            let saved = unsafe { libc::dup(1) };
            if saved >= 0 && unsafe { libc::dup2(2, 1) } >= 0 {
                let file = unsafe { std::fs::File::from_raw_fd(saved) };
                return Self { out: Box::new(file), json };
            }
        }
        Self { out: Box::new(std::io::stdout()), json }
    }

    fn emit(&mut self, human: &str, value: &Value) {
        let text = if self.json { serde_json::to_string_pretty(value).unwrap_or_default() } else { human.to_string() };
        let _ = writeln!(self.out, "{text}");
        let _ = self.out.flush();
    }
}

fn password(prompt: &str) -> LezerResult<Zeroizing<String>> {
    if let Ok(p) = std::env::var("LEZER_PASSWORD") {
        return Ok(Zeroizing::new(p));
    }
    rpassword::prompt_password(prompt)
        .map(Zeroizing::new)
        .map_err(|e| LezerError::new(ErrorCode::InvalidPassword, format!("cannot read password: {e} (set LEZER_PASSWORD for non-interactive use)")))
}

/// Read a secret from stdin (piped) or a hidden prompt; whitespace is normalized.
fn read_secret(prompt: &str) -> LezerResult<Zeroizing<String>> {
    use std::io::IsTerminal as _;
    let raw = if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt)
    } else {
        std::io::read_to_string(std::io::stdin())
    };
    raw.map(|p| Zeroizing::new(p.split_whitespace().collect::<Vec<_>>().join(" ")))
        .map_err(|e| LezerError::new(ErrorCode::InvalidMnemonic, e.to_string()))
}

fn profile(network: Option<&str>) -> LezerResult<Option<NetworkProfile>> {
    network
        .map(|n| NetworkProfile::resolve(Some(n)))
        .transpose()
        .map_err(|e| LezerError::new(ErrorCode::DisallowedEndpointBlocked, e.to_string()))
}

fn no_program() -> LezerError {
    LezerError::new(
        ErrorCode::ProgramNotDeployed,
        "no testimonial program configured for this network (pass --program or set LEZER_TESTIMONIAL_PROGRAM)",
    )
}

fn no_registry() -> LezerError {
    LezerError::new(
        ErrorCode::ProgramNotDeployed,
        "no lezer_registry program deployed from this wallet (run `lezer program deploy lezer_registry.bin` or pass --program)",
    )
}

fn no_token_program() -> LezerError {
    LezerError::new(
        ErrorCode::ProgramNotDeployed,
        "no lezer_token program deployed from this wallet (run `lezer program deploy lezer_token.bin` or pass --program)",
    )
}

fn no_faucet_program() -> LezerError {
    LezerError::new(
        ErrorCode::ProgramNotDeployed,
        "no lezer_faucet program deployed from this wallet (run `lezer program deploy lezer_faucet.bin` or pass --program)",
    )
}

fn unlock(w: &mut WalletEngine) -> LezerResult<()> {
    w.unlock(&password("Wallet password: ")?)
}

async fn run(cli: Cli, out: &mut Output) -> LezerResult<()> {
    let home = cli.home.clone().unwrap_or_else(default_lezer_home);
    let net = profile(cli.network.as_deref())?;

    if let Command::Network = cli.command {
        let p = net.unwrap_or_else(NetworkProfile::from_env);
        let rpc = LezRpcClient::new(p.clone()).map_err(|e| LezerError::new(ErrorCode::DisallowedEndpointBlocked, e.to_string()))?;
        let block = rpc.get_last_block_id().await.map_err(|e| LezerError::new(ErrorCode::SequencerUnreachable, e.to_string()))?;
        let programs = rpc.get_program_ids().await.unwrap_or_default();
        let expected = lezer_adapter::program_id_words_to_hex(&lezer_adapter::privacy_circuit_id());
        let remote = programs.get("privacy_preserving_circuit").map(lezer_adapter::program_id_words_to_hex);
        let compatible = remote.as_deref() == Some(expected.as_str());
        let v = json!({ "sequencer_url": p.sequencer_url, "last_block_id": block, "privacy_circuit_matches": compatible, "remote_privacy_circuit": remote, "local_privacy_circuit": expected });
        out.emit(
            &format!(
                "{}  block #{block}  privacy circuit {}",
                p.sequencer_url,
                if compatible { "matches (compatible)" } else { "DIFFERS — wallet and network versions are incompatible for private transfers" }
            ),
            &v,
        );
        return Ok(());
    }

    if let Command::VerifyAdoption { program } = &cli.command {
        let p = net
            .or_else(|| WalletEngine::configured_network(&home))
            .unwrap_or_else(NetworkProfile::from_env);
        let program = program
            .clone()
            .or_else(|| WalletEngine::stored_deployment(&home, "lezer_testimonial"))
            .or_else(|| p.deployments.testimonial.clone())
            .ok_or_else(no_program)?;
        let r = lezer_adapter::verify_adoption(&p, &program).await.map_err(|e| LezerError::new(ErrorCode::SequencerUnreachable, format!("{e:#}")))?;
        let mut human = format!(
            "Program {} on {} (block #{})\n{} testimonials from {} distinct authors ({} index pages read)\n",
            r.program_id, r.sequencer_url, r.last_block_id, r.total_testimonials, r.distinct_authors, r.index_pages_read
        );
        for t in &r.testimonials {
            human.push_str(&format!("\n  {}  {}\n    {}\n", t.author, t.username, t.text));
        }
        out.emit(&human, &serde_json::to_value(&r).unwrap_or_default());
        return Ok(());
    }
    if let Command::Program { sub: ProgramCmd::ImageId { elf } } = &cli.command {
        let bytes = std::fs::read(elf).map_err(|e| LezerError::new(ErrorCode::TransactionRejected, e.to_string()))?;
        let id = lezer_adapter::image_id_hex(&bytes).map_err(|e| LezerError::new(ErrorCode::TransactionRejected, e.to_string()))?;
        out.emit(&id, &json!({ "image_id": id }));
        return Ok(());
    }

    let mut w = WalletEngine::new(&home, net);
    match cli.command {
        Command::Network | Command::VerifyAdoption { .. } => unreachable!(),
        Command::Program { sub } => {
            unlock(&mut w)?;
            let ProgramCmd::Deploy { elf, payer } = sub else { unreachable!() };
            let bytes = std::fs::read(&elf).map_err(|e| LezerError::new(ErrorCode::TransactionRejected, format!("{}: {e}", elf.display())))?;
            let stem = elf.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let d = w.deploy_program(&bytes, &payer, stem).await?;
            out.emit(
                &format!("Deployed program {}\n  image id {}\n  {} segment(s), {} bytes", d.program_id, d.image_id, d.segments.len(), d.elf_bytes),
                &serde_json::to_value(&d).unwrap_or_default(),
            );
        }
        Command::Registry { sub } => {
            match sub {
                RegistryCmd::List { program } => {
                    let program = program.or_else(|| w.registry_program()).or_else(|| WalletEngine::stored_deployment(&home, "lezer_registry")).ok_or_else(no_registry)?;
                    let r = w.list_registry(&program).await?;
                    let mut human = format!("Registry {} on {} (block #{})\n{} programs ({} index pages read)\n", r.registry_program_id, r.sequencer_url, r.last_block_id, r.total_programs, r.index_pages_read);
                    for e in &r.entries {
                        human.push_str(&format!("\n  {}  {} v{}\n    {} @{}\n", e.program_id, e.name, e.version, e.source_url, e.commit));
                    }
                    out.emit(&human, &serde_json::to_value(&r).unwrap_or_default());
                }
                RegistryCmd::Register { subject, name, source_url, commit, version, payer, program } => {
                    unlock(&mut w)?;
                    let program = program.or_else(|| w.registry_program()).ok_or_else(no_registry)?;
                    let r = w.register_program(&program, &subject, &name, &source_url, &commit, &version, &payer).await?;
                    emit_receipt(out, "Registry self-registration", &r);
                }
            }
        }
        Command::Token { sub } => {
            unlock(&mut w)?;
            let resolve = |program: Option<String>| -> LezerResult<String> {
                program.or_else(|| w.token_program()).ok_or_else(no_token_program)
            };
            match sub {
                TokenCmd::Define { definition, name, symbol, decimals, supply, authority, payer, program } => {
                    let program = resolve(program)?;
                    let info = w.token_define(&program, &definition, &payer, &name, &symbol, decimals, authority.as_deref(), supply).await?;
                    out.emit(
                        &format!("Token defined: {} {}
  token id {}
  supply {}  authority {}", info.symbol, info.name, info.token_id, info.total_supply, info.authority.as_deref().unwrap_or("renounced (fixed supply)")),
                        &serde_json::to_value(&info).unwrap_or_default(),
                    );
                }
                TokenCmd::Info { token, program } => {
                    let program = resolve(program)?;
                    let info = w.token_info(&program, &token).await?;
                    out.emit(
                        &format!("{} ({})
  token id {}
  decimals {}  supply {}
  authority {}", info.name, info.symbol, info.token_id, info.decimals, info.total_supply, info.authority.as_deref().unwrap_or("renounced")),
                        &serde_json::to_value(&info).unwrap_or_default(),
                    );
                }
                TokenCmd::Balance { token, owner, program } => {
                    let program = resolve(program)?;
                    let b = w.token_balance(&program, &token, &owner).await?;
                    out.emit(&format!("{owner} holds {b}"), &json!({ "owner": owner, "balance": b.to_string() }));
                }
                TokenCmd::Mint { token, authority, to, amount, payer, program } => {
                    let program = resolve(program)?;
                    let r = w.token_mint(&program, &token, &authority, &to, &payer, amount).await?;
                    emit_receipt(out, "Mint", &r);
                }
                TokenCmd::Transfer { token, from, to, amount, payer, program } => {
                    let program = resolve(program)?;
                    let r = w.token_transfer(&program, &token, &from, &to, &payer, amount).await?;
                    emit_receipt(out, "Token transfer", &r);
                }
                TokenCmd::Burn { token, from, amount, payer, program } => {
                    let program = resolve(program)?;
                    let r = w.token_burn(&program, &token, &from, &payer, amount).await?;
                    emit_receipt(out, "Burn", &r);
                }
                TokenCmd::SetAuthority { token, authority, new, payer, program } => {
                    let program = resolve(program)?;
                    let new_auth = (new != "none").then_some(new.as_str());
                    let r = w.token_set_authority(&program, &token, &authority, &payer, new_auth).await?;
                    emit_receipt(out, "Authority change", &r);
                }
            }
        }
        Command::Faucet { sub } => {
            let resolve = |w: &WalletEngine, program: Option<String>| -> LezerResult<String> {
                program
                    .or_else(|| w.faucet_program())
                    .or_else(|| WalletEngine::stored_deployment(&home, "lezer_faucet"))
                    .ok_or_else(no_faucet_program)
            };
            match sub {
                FaucetCmd::Vault { program } => {
                    let program = resolve(&w, program)?;
                    let vault = lezer_adapter::faucet_vault_id(&program)
                        .map_err(|e| LezerError::new(ErrorCode::InvalidAddress, e.to_string()))?;
                    out.emit(&vault, &json!({ "faucet_program_id": program, "vault_account_id": vault }));
                }
                FaucetCmd::Status { recipient, program } => {
                    let program = resolve(&w, program)?;
                    let resolved_rec = match recipient {
                        Some(r) if r.starts_with("Public/") => Some(r),
                        Some(r) => {
                            let _ = unlock(&mut w);
                            w.resolve_account(&r).ok().or(Some(r))
                        }
                        None => None,
                    };
                    let st = w.faucet_status(&program, resolved_rec.as_deref()).await?;
                    out.emit(
                        &format!(
                            "Faucet {} (vault {})\n  vault balance: {}  drip: {}\n  recipient: {}  already_claimed: {}",
                            st.faucet_program_id,
                            st.vault_account_id,
                            st.vault_balance,
                            st.drip_amount,
                            st.recipient.as_deref().unwrap_or("—"),
                            st.already_claimed
                        ),
                        &serde_json::to_value(&st).unwrap_or_default(),
                    );
                }
                FaucetCmd::Claim { recipient, payer, program } => {
                    unlock(&mut w)?;
                    let program = resolve(&w, program)?;
                    let r = w.faucet_claim(&program, &recipient, payer.as_deref()).await?;
                    emit_receipt(out, "Faucet claim", &r);
                }
            }
        }
        Command::Testimonial { sub: TestimonialCmd::Submit { author, payer, username, text, program } } => {
            unlock(&mut w)?;
            let program = program.or_else(|| w.testimonial_program()).ok_or_else(no_program)?;
            let payer = payer.unwrap_or_else(|| author.clone());
            let r = w.submit_testimonial(&program, &author, &payer, &username, &text).await?;
            let human = match &r.status {
                TxStatus::Included { block_id } => format!("Testimonial published in block #{block_id}\n  tx {}", r.tx_hash),
                TxStatus::Submitted { detail } => format!("Testimonial submitted but not yet included\n  tx {}\n  {detail}", r.tx_hash),
            };
            out.emit(&human, &serde_json::to_value(&r).unwrap_or_default());
        }
        Command::Init => {
            let pw = password("New wallet password (min 8 chars): ")?;
            let res = w.init(&pw)?;
            let phrase = res.recovery_phrase.clone().unwrap_or_default();
            let accounts: Vec<String> = res.accounts.iter().map(|a| format!("  {} {}", a.account_id, a.labels.join(","))).collect();
            out.emit(
                &format!(
                    "Wallet created at {}\nNetwork: {}\n\nRECOVERY PHRASE (write it down; it will not be shown again):\n\n  {phrase}\n\nAccounts:\n{}",
                    res.vault_path, res.network_id, accounts.join("\n")
                ),
                &serde_json::to_value(&res).unwrap_or_default(),
            );
        }
        Command::Restore { discover_depth } => {
            let phrase = read_secret("Recovery phrase: ")?;
            let pw = password("New wallet password (min 8 chars): ")?;
            let res = w.restore(&phrase, &pw, discover_depth).await?;
            let accounts: Vec<String> = res.accounts.iter().map(|a| format!("  {} {}", a.account_id, a.labels.join(","))).collect();
            out.emit(&format!("Wallet restored at {}\nAccounts:\n{}", res.vault_path, accounts.join("\n")), &serde_json::to_value(&res).unwrap_or_default());
        }
        Command::Status => {
            unlock(&mut w)?;
            let chain = w.chain_status().await?;
            let snap = w.snapshot();
            let human = format!(
                "Network {} ({})  reachable={} block={}\nAccounts: {}  Sessions: {}  Recent txs: {}",
                snap.network_id,
                chain.sequencer_url,
                chain.reachable,
                chain.last_block_id.map_or("?".into(), |b| b.to_string()),
                snap.accounts.len(),
                snap.active_sessions.len(),
                snap.recent_activity.len()
            );
            out.emit(&human, &json!({ "chain": chain, "snapshot": snap }));
        }
        Command::Account { sub } => {
            unlock(&mut w)?;
            match sub {
                AccountCmd::New { kind, label } => {
                    let a = w.create_account(kind.into(), label.as_deref())?;
                    out.emit(&format!("Created {} {}", a.account_id, a.labels.join(",")), &serde_json::to_value(&a).unwrap_or_default());
                }
                AccountCmd::List => {
                    let accts = w.accounts()?;
                    let lines: Vec<String> = accts
                        .iter()
                        .map(|a| {
                            let name = a.received_via.as_ref().map_or_else(|| a.labels.join(","), |v| format!("{v} (received)"));
                            format!("{:<8} {} {name}", format!("{:?}", a.kind).to_lowercase(), a.account_id)
                        })
                        .collect();
                    out.emit(&lines.join("\n"), &serde_json::to_value(&accts).unwrap_or_default());
                }
                AccountCmd::Import { label } => {
                    let key = read_secret("Public account private key (hex): ")?;
                    let a = w.import_public_key(&key, label.as_deref())?;
                    out.emit(&format!("Imported {} {}", a.account_id, a.labels.join(",")), &serde_json::to_value(&a).unwrap_or_default());
                }
                AccountCmd::Label { account, label } => {
                    let id = w.resolve_account(&account)?;
                    w.create_label(&id, &label)?;
                    out.emit(&format!("Labeled {id} as {label}"), &json!({ "account_id": id, "label": label }));
                }
                AccountCmd::Receive { account } => {
                    let k = w.receive_keys(&account)?;
                    out.emit(&format!("{}\n\n{}", k.account_id, k.receive_address), &serde_json::to_value(&k).unwrap_or_default());
                }
                AccountCmd::Use { account } => {
                    let id = w.set_active_account(&account)?;
                    out.emit(&format!("Active account: {id}"), &json!({ "active_account_id": id }));
                }
            }
        }
        Command::Balance { account } => {
            unlock(&mut w)?;
            let b = w.balance(&account).await?;
            let note = match b.source.as_str() {
                "cached" => " (offline — cached)",
                "local" => " (private — run `lezer sync` to include new incoming notes)",
                _ => "",
            };
            out.emit(&format!("{} {}{note}", b.account_id, b.balance), &serde_json::to_value(&b).unwrap_or_default());
        }
        Command::Sync => {
            unlock(&mut w)?;
            let block = w.sync().await?;
            out.emit(&format!("Synced to block #{block}"), &json!({ "synced_block": block }));
        }
        Command::Send { from, to, amount } => {
            unlock(&mut w)?;
            let r = w.transfer(&from, &to, amount).await?;
            let mut v = serde_json::to_value(&r).unwrap_or_default();
            v["peak_rss_mib"] = json!(peak_rss_bytes() as f64 / 1_048_576.0);
            let human = match &r.status {
                TxStatus::Included { block_id } => format!(
                    "{:?} transfer included in block #{block_id}\n  tx {}\n  build {} ms, inclusion {} ms",
                    r.mode,
                    r.tx_hash,
                    r.build_ms,
                    r.inclusion_ms.unwrap_or_default()
                ),
                TxStatus::Submitted { detail } => {
                    format!("{:?} transfer submitted but not yet included\n  tx {}\n  {detail}", r.mode, r.tx_hash)
                }
            };
            out.emit(&human, &v);
        }
    }
    Ok(())
}

fn emit_receipt(out: &mut Output, what: &str, r: &TxReceipt) {
    let human = match &r.status {
        TxStatus::Included { block_id } => format!("{what} included in block #{block_id}\n  tx {}", r.tx_hash),
        TxStatus::Submitted { detail } => format!("{what} submitted but not yet included\n  tx {}\n  {detail}", r.tx_hash),
    };
    out.emit(&human, &serde_json::to_value(r).unwrap_or_default());
}

fn main() {
    // Silence upstream `wallet`'s verbose tx dumps (they include signatures + full
    // instruction data). Not secrets, but they bury the actual output.
    // SAFETY: set before tokio spawns worker threads; upstream only reads it.
    unsafe { std::env::set_var("SUPPRESS_VERBOSE_PRINTS", "1") };
    let cli = Cli::parse();
    let mut out = Output::setup(cli.json);
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    if let Err(err) = rt.block_on(run(cli, &mut out)) {
        let env = err.to_envelope();
        out.emit(&format!("error {}: {}", env.code, env.message), &json!({ "error": env }));
        std::process::exit(1);
    }
}
