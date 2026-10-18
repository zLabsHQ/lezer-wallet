# Lezer Wallet

<div align="center">

**The Native Privacy-Preserving Execution Zone Wallet, Provider SDK & Program Verification Suite for Logos**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![CI](https://github.com/zLabsHQ/lezer-wallet/actions/workflows/ci.yml/badge.svg)](https://github.com/zLabsHQ/lezer-wallet/actions/workflows/ci.yml)
[![Network: Logos LEZ Testnet v0.3/v0.4](https://img.shields.io/badge/network-LEZ%20Testnet%20v0.3%2Fv0.4-green)](https://testnet.lez.logos.co)

</div>

---

## The Vision

**Lezer Wallet** is a production-grade, privacy-first wallet and developer platform for the **Logos Execution Zone (LEZ)** — the programmable execution environment of the Logos blockchain where public and private smart contracts execute inside the RISC Zero zkVM.

Named after the community identity for LEZ builders and runners (*"LEZers"*), Lezer solves the foundational adoption bottleneck of the Logos ecosystem:

- **For Users:** A unified, non-custodial, privacy-first key and asset manager that supports both transparent public accounts and fully shielded private accounts.
- **For Developers:** A standard **Wallet Provider SDK** (`@lezer/provider` + `lezer_wallet_provider` LIDL module) enabling any Basecamp dApp to connect, request permissions, and propose transactions — with zero blind signing.
- **For the Ecosystem:** An on-chain **LP-0023 Program Registry** providing cryptographic source verification badges for every program a user interacts with.

## Why Lezer?

The LEZ protocol enables both **public** (transparent, Ed25519-signed) and **private** (ML-KEM-768 encrypted, ZK-proven) accounts. Lezer is the first wallet built from day one around the complete LEZ v0.3/v0.4 specification:

| Capability | Implementation |
| :--- | :--- |
| **4-Mode Privacy Matrix** | `Public → Public`, `Public → Private` (Shield), `Private → Public` (Deshield), `Private → Private` (ZK) |
| **Token & ATA Support** | Full `TOKEN_ID` (`NewFungibleDefinition`, `Mint`, `Transfer`, `Burn`, `SetAuthority`) + deterministic `ASSOCIATED_TOKEN_ACCOUNT_ID` for both public and private owners |
| **In-Daemon Session Broker** | Rust-enforced `ApprovalBroker` with scoped session tokens and per-account `ReadPrivateBalance` capability gates — impossible to bypass via raw IPC |
| **Zero Blind Signing** | Dual-layer decoder: built-in genesis program deserializers + LP-0023 SPEL IDL resolution from Logos Storage, with critical-red warnings for authority changes and unlimited approvals |
| **4-Stage ZK Prover** | `Witness → r0vm → RapidSnark → Broadcast` with `Semaphore(1)` queue, live progress streaming, and crash-recovery nullifier journal |
| **On-Chain Verification** | 4-state LP-0023 badge: `VerifiedSource`, `KnownButUnverified`, `NoRegistryEntry`, `MismatchOrError` — fail-closed by default |
| **Client-Side Encrypted Backup** | `SealedVaultEnvelopeV1` (Argon2id + XChaCha20-Poly1305) pushed to Logos Storage/Codex — the node never sees plaintext |
| **Zero Telemetry** | Strict network egress allowlist (`sequencer_url`, `lez_node_url`, optional `logos_storage_url`) — no analytics, no third-party APIs |

## Architecture Overview

Lezer uses a **Basecamp-First & Rust-Core** architecture with strict compile-time boundaries:

```
┌─────────────────────────────────────────────────────────────┐
│                    PRESENTATION LAYER                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│  │ lezer_wallet │  │ lezer_faucet │  │ lezer_testimonial│   │
│  │    _ui       │  │    _ui       │  │       _ui        │   │
│  │  (QML/GUI)   │  │  (QML/GUI)   │  │   (QML/GUI)      │   │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘   │
│         └─────────────────┴───────────────────┘             │
│                           │ LIDL/IPC (JSON)                 │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │           lezer_wallet_provider (cdylib)            │    │
│  └────────────────────────┬────────────────────────────┘    │
└───────────────────────────┼─────────────────────────────────┘
                            │
┌───────────────────────────┼─────────────────────────────────┐
│                  NATIVE RUST CORE LAYER                      │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │                    lezer-core                       │    │
│  │  Vault │ Cache │ Sessions │ Broker │ Decoder │      │    │
│  │  AutoLock │ ProverQueue │ Registry │ Storage │      │    │
│  └────────────────────────┬────────────────────────────┘    │
│                           │                                 │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │                   lezer-adapter                     │    │
│  │  RPC │ Accounts │ Transfers │ Token │ ATA │ Faucet ││    │
│  │  Simulate │ Prover │ Codecs │ (ONLY layer that      │    │
│  │           imports upstream LEZ/wallet crates)       │    │
│  └────────────────────────┬────────────────────────────┘    │
└───────────────────────────┼─────────────────────────────────┘
                            │ JSON-RPC
┌───────────────────────────┼─────────────────────────────────┐
│                    PROTOCOL LAYER                            │
│         Logos Execution Zone Sequencer (LEZ v0.3/v0.4)      │
│    https://testnet.lez.logos.co (or local standalone node)  │
└─────────────────────────────────────────────────────────────┘
```

**Key Principle:** `lezer-core` owns all wallet truth (keys, state, sessions, approvals). `lezer-adapter` is the *only* crate allowed to import upstream LEZ protocol crates. `modules/`, `packages/`, and `apps/` never see internal vault structs or secrets.

## Privacy-First Design

### Private Accounts (`Private/<base58>`)

- **Dual-Key Architecture:** Every private account derives a **Nullifier Keypair** (`npk`/`nsk`, Ed25519) and a **Viewing Keypair** (`vpk`/`vsk`, ML-KEM-768).
- **Encrypted On-Chain State:** Private account state exists on-chain only as a Pedersen commitment in the zone Merkle tree. The owner decrypts notes locally using `vpk`.
- **View-Tag Scanning:** 4-byte fast view tags let `lezer-core` skip ~99% of unrelated notes without full decapsulation.
- **Receive URIs:** Checksummed `lezer:priv1...` single-string URIs encode `(vpk, npk)` for privacy-preserving payments.

### Crash-Safe Vault (`~/.lezer/vault.enc`)

- `SealedVaultEnvelopeV1`: Argon2id key derivation → XChaCha20-Poly1305 AEAD → SHA-256 header MAC.
- **Atomic Persistence:** Write-temp → fsync → rename prevents corruption on power loss.
- **15-Minute Auto-Lock:** `AutoLockFsm` with `DeferredLockPending` protects in-flight ZK proofs while enforcing the lock timer.
- **Nullifier Journal:** Write-ahead `PendingPrivateSpend` log rejects double-spends across restarts.

### Session & Approval Security

Every dApp-facing method (`get_accounts`, `get_balance`, `propose_transaction`) requires a cryptographic `session_token` issued only after explicit user approval in `lezer_wallet_ui`. Private balance reads additionally require `PermissionScope::ReadPrivateBalance` for that specific account.

## Technology Stack

| Layer | Technology |
| :--- | :--- |
| **Core Wallet Engine** | Rust 2021 (`lezer-core`, `lezer-adapter`, `lezer-cli`, `lezer-wasm`, `lezerd`) |
| **Zero-Knowledge Proving** | RISC Zero zkVM + RapidSnark Groth16 (4-stage pipeline) |
| **Cryptography** | Argon2id, XChaCha20-Poly1305, Ed25519, ML-KEM-768, SHA-256, Pedersen Commitments |
| **Basecamp UI** | Qt6 QML (`ui_qml` modules: `lezer_wallet_ui`, `lezer_faucet_ui`, `lezer_testimonial_ui`) |
| **Core Module Interface** | LIDL C-ABI (`lezer_wallet_provider.lidl` → `cdylib`) |
| **TypeScript SDKs** | `@lezer/provider`, `@lezer/registry`, `@lezer/recovery` (strict TS, ESM) |
| **On-Chain Programs** | SPEL RISC0 guest programs (`testimonial`, `registry`) |
| **Web Portal** | Next.js 15 Static Export (`apps/lezer-portal`) — zero server, zero custody |
| **Browser Extension** | Manifest V3 (`apps/lezer-extension`) with two-gear signing (WASM + `lezerd`) |
| **Recovery** | 2-of-3 Shamir Secret Sharing (GF(256)) + WebAuthn Passkey PRF |
| **Build & Package** | Nix flakes (`mkLogosModule`, `mkLogosQmlModule`), `lgpd` module catalog |
| **CI/CD** | GitHub Actions + `logos-co/logos-modules-release-action@v1` |

## Repository Structure

```
lezer-wallet/
├── crates/
│   ├── lezer-adapter/          # LEZ v0.3/v0.4 RPC, Token, ATA, Faucet, Prover wrapper
│   ├── lezer-core/             # Vault, cache, sessions, broker, decoder, registry, storage
│   ├── lezer-cli/              # `lezer` binary — full CLI parity
│   ├── lezer-wasm/             # Pure-WASM Gear 1 public signing (browser)
│   └── lezerd/                 # Native Messaging host daemon (Gear 2 local prover)
├── modules/
│   ├── lezer_wallet_provider/  # Logos Core module (LIDL cdylib)
│   ├── lezer_wallet_ui/        # Basecamp QML Wallet GUI
│   ├── lezer_faucet_ui/        # Faucet & Funding Mini-App
│   └── lezer_testimonial_ui/   # On-Chain Testimonial Mini-App
├── programs/
│   ├── testimonial/            # SPEL program: 16-shard paginated index PDAs
│   └── registry/               # SPEL program: LP-0023 Program Registry
├── packages/
│   ├── provider/               # `@lezer/provider` + `LezerProvider.qml`
│   ├── registry/               # `@lezer/registry` — LP-0023 client & IDL decoder
│   └── recovery/               # `@lezer/recovery` — 2-of-3 Shamir + Passkey Kit
├── templates/
│   └── lezer-dapp-template/    # Forkable Basecamp dApp starter
├── apps/
│   ├── lezer-portal/           # Next.js static portal (docs, playground, explorer)
│   └── lezer-extension/        # MV3 Browser Extension
├── tests/
│   ├── conformance/            # Golden JSON fixtures (vault, errors, shards, summaries)
│   └── e2e/                    # Standalone sequencer E2E + Release Gates RC-01..RC-03
├── tooling/                    # Boundary checks, spikes, catalog, demo, verify scripts
└── docs/                       # Internal engineering specs (00-10)
```

## Quickstart

### Prerequisites

- Rust toolchain (stable, `rustup`)
- Node.js 20+ & npm (for TypeScript packages)
- `lgpd` + `nix` (for Basecamp module installation)
- Access to LEZ Testnet (`https://testnet.lez.logos.co`) or local standalone sequencer

### Build & Run the CLI

```bash
cargo build -p lezer-cli --release
./target/release/lezer init --mnemonic "<24-word-mnemonic>" --password "<password>"
./target/release/lezer unlock --password "<password>"
./target/release/lezer account new --privacy public --label main
./target/release/lezer account new --privacy private --label savings
./target/release/lezer faucet claim --auto-shield-to savings
```

### Install Basecamp Modules

```bash
lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json
lgpd install lezer_wallet_provider lezer_wallet_ui lezer_faucet_ui lezer_testimonial_ui
```

### Run the Evaluator Demo Suite

```bash
./tooling/demo.sh
```

This runs a clean-room E2E suite covering: boundary checks, catalog validation, SPEL guest tests, full workspace conformance (RC-01, RC-02, RC-03), CLI lifecycle (vault → faucet → token/ATA → shield → registry → backup → verify-adoption), and the stateless 16-shard adoption verifier.

## Verification & Release Gates

| Gate | Description | Status |
| :--- | :--- | :---: |
| **RC-01** | End-to-end dApp connect → approval sheet → testimonial submission | ✅ |
| **RC-02** | Live private native & token transfers, deferred lock, crash recovery | ✅ |
| **RC-03** | 50-account concurrent testimonial shard overflow + stateless verify-adoption | ✅ |
| **CI** | `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` | ✅ |

## Documentation

- [`docs/00-INDEX.md`](docs/00-INDEX.md) — Master index & LP-0021/LP-0023 traceability matrix
- [`docs/01-SYSTEM-ARCHITECTURE.md`](docs/01-SYSTEM-ARCHITECTURE.md) — Cryptographic engine & vault design
- [`docs/02-FEATURES-AND-PRODUCT-SPEC.md`](docs/02-FEATURES-AND-PRODUCT-SPEC.md) — Token/ATA engine & UX flows
- [`docs/03-PROTOCOL-SDK-AND-IPC-SPEC.md`](docs/03-PROTOCOL-SDK-AND-IPC-SPEC.md) — LIDL contract & `@lezer/provider` API
- [`docs/04-ONCHAIN-PROGRAMS-AND-REGISTRY.md`](docs/04-ONCHAIN-PROGRAMS-AND-REGISTRY.md) — SPEL programs & LP-0023
- [`docs/06-PACKAGING-CI-AND-EXECUTION-ROADMAP.md`](docs/06-PACKAGING-CI-AND-EXECUTION-ROADMAP.md) — Nix packaging, CI gates, 24-day schedule
- [`docs/07-VISION-AND-PLATFORM-STRATEGY.md`](docs/07-VISION-AND-PLATFORM-STRATEGY.md) — Post-prize vision (extension, mobile, Shamir, LP-0022)
- [`docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md`](docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md) — Finalized ADRs & concurrency rules
- [`docs/09-REPOSITORY-AND-FOLDER-STRUCTURE.md`](docs/09-REPOSITORY-AND-FOLDER-STRUCTURE.md) — File-by-file blueprint
- [`docs/10-PHASE-WISE-IMPLEMENTATION-PLAN.md`](docs/10-PHASE-WISE-IMPLEMENTATION-PLAN.md) — Phase 0→5 execution plan

## License

Dual-licensed under **MIT OR Apache-2.0**. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

OSS dependency compliance: [`THIRD_PARTY.md`](THIRD_PARTY.md).

---

<div align="center">

**Built for the Logos Execution Zone · LP-0021 & LP-0023 · `lp0021-lezer`**

[Repository](https://github.com/zLabsHQ/lezer-wallet) · [Issues](https://github.com/zLabsHQ/lezer-wallet/issues) · [Releases](https://github.com/zLabsHQ/lezer-wallet/releases)

</div>
