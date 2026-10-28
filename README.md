# Lezer Wallet

<div align="center">

**The Native Privacy-Preserving Execution Zone Wallet, Provider SDK & Program Verification Suite for Logos**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![CI](https://github.com/zLabsHQ/lezer-wallet/actions/workflows/ci.yml/badge.svg)](https://github.com/zLabsHQ/lezer-wallet/actions/workflows/ci.yml)
[![Network: Logos LEZ Testnet v0.3/v0.4](https://img.shields.io/badge/network-LEZ%20Testnet%20v0.3%2Fv0.4-green)](https://testnet.lez.logos.co)

</div>

---

## The Vision

**Lezer Wallet** is a privacy-first wallet (work in progress — see [Implementation Status](#implementation-status-honest)) and developer platform for the **Logos Execution Zone (LEZ)** — the programmable execution environment of the Logos blockchain where public and private smart contracts execute inside the RISC Zero zkVM.

Named after the community identity for LEZ builders and runners (*"LEZers"*), Lezer solves the foundational adoption bottleneck of the Logos ecosystem:

- **For Users:** A unified, non-custodial, privacy-first key and asset manager that supports both transparent public accounts and fully shielded private accounts.
- **For Developers:** A standard **Wallet Provider SDK** (`@lezer/provider` + `lezer_wallet_provider` LIDL module) enabling any Basecamp dApp to connect, request permissions, and propose transactions — with zero blind signing.
- **For the Ecosystem:** An on-chain **LP-0023 Program Registry** providing cryptographic source verification badges for every program a user interacts with.

## Why Lezer?

The LEZ protocol enables both **public** (transparent, Ed25519-signed) and **private** (ML-KEM-768 encrypted, ZK-proven) accounts. Lezer is designed around the LEZ v0.3 specification. Design targets (see the status table below for what is implemented today):

| Capability | Design |
| :--- | :--- |
| **4-Mode Privacy Matrix** | `Public → Public`, `Public → Private` (Shield), `Private → Public` (Deshield), `Private → Private` (ZK) |
| **Fungible Tokens** | `lezer_token` SPEL program: `Define`/`Mint`/`Transfer`/`Burn`/`SetAuthority` (incl. renounce), per-(token, owner) holding PDAs, authority enforcement in the guest apply |
| **In-Daemon Session Broker** | Rust-enforced `ApprovalBroker` with scoped session tokens and per-account `ReadPrivateBalance` capability gates |
| **Zero Blind Signing** | Dual-layer decoder: built-in genesis program deserializers + LP-0023 SPEL IDL resolution from Logos Storage, with critical-red warnings for authority changes and unlimited approvals |
| **Real ZK Proving** | Upstream `WalletCore` proving pipeline (RISC Zero); measured locally: ~3–6 min per private op, ~9 GB peak RSS on Apple silicon |
| **On-Chain Registry** | `lezer_registry`: programs self-register via chained calls (`caller_account_id` auth) — a signer cannot forge a listing |
| **Client-Side Encrypted Backup** | `SealedVaultEnvelopeV1` (Argon2id + XChaCha20-Poly1305) pushed to Logos Storage/Codex — the node never sees plaintext |
| **Zero Telemetry** | Strict network egress allowlist (`sequencer_url`, `lez_node_url`, optional `logos_storage_url`) — no analytics, no third-party APIs |

## Architecture Overview

Lezer uses a **Basecamp-First & Rust-Core** architecture with strict compile-time boundaries:

```
┌─────────────────────────────────────────────────────────────┐
│                    PRESENTATION LAYER                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│  │ lezer_wallet │  │ lezer_faucet │  │ lezer_testimonial│   │
│  │    _ui       │  │     _ui      │  │       _ui        │   │
│  │  (QML/GUI)   │  │  (QML/GUI)   │  │   (QML/GUI)      │   │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘   │
│         └─────────────────┴───────────────────┘             │
│                           │ LIDL/IPC (JSON)                 │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │           lezer_wallet_provider (cdylib)            │    │
│  │    Caller-authenticated via logos_rust_sdk          │    │
│  └────────────────────────┬────────────────────────────┘    │
└───────────────────────────┼─────────────────────────────────┘
                            │
┌───────────────────────────┼─────────────────────────────────┐
│                  NATIVE RUST CORE LAYER                      │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │                    lezer-core                       │    │
│  │  Vault │ Cache │ Sessions │ Broker │ Intent/Summary │    │
│  │  AutoLock │ State │ Errors │ Redacted AuditLogger   │    │
│  └────────────────────────┬────────────────────────────┘    │
│                           │                                 │
│  ┌────────────────────────┴────────────────────────────┐    │
│  │                   lezer-adapter                     │    │
│  │  WalletCore Engine │ RPC │ Token │ Faucet │         │    │
│  │  Programs │ Registry │ Profile │ Codecs             │    │
│  │  (ONLY crate importing upstream LEZ/wallet crates)  │    │
│  └────────────────────────┬────────────────────────────┘    │
└───────────────────────────┼─────────────────────────────────┘
                            │ JSON-RPC
┌───────────────────────────┼─────────────────────────────────┐
│                    PROTOCOL LAYER                            │
│         Logos Execution Zone Sequencer (LEZ v0.3.1)         │
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
| **Core Wallet Engine** | Rust 2024 (`lezer-core`, `lezer-adapter`, `lezer-cli`, `lezer-provider`) |
| **Zero-Knowledge Proving** | Official `WalletCore` RISC Zero zkVM proving pipeline (v0.3.1 circuit) |
| **Cryptography** | Argon2id, XChaCha20-Poly1305, Ed25519, ML-KEM-768, SHA-256, Pedersen Commitments |
| **Basecamp UI** | Qt6 QML (`ui_qml` modules: `lezer_wallet_ui`, `lezer_testimonial_ui`, `lezer_faucet_ui`) |
| **Core Module Interface** | LIDL C-ABI (`lezer_wallet_provider.lidl` → `cdylib` with `logos_rust_sdk`) |
| **TypeScript SDKs** | `@lezer/provider`, `@lezer/registry` (strict TS, ESM) |
| **On-Chain Programs** | SPEL RISC0 guest programs (`lezer_testimonial`, `lezer_token`, `lezer_faucet`, `lezer_registry`) |
| **Web Portal** | Next.js 15 Static Export (`apps/lezer-portal`) — zero server, zero custody |
| **Build & Package** | Nix flakes (`mkLogosModule`, `mkLogosQmlModule`), `lgpd` module catalog |
| **CI/CD** | GitHub Actions + `logos-co/logos-modules-release-action@v1` |

## Repository Structure

```
lezer-wallet/
├── crates/
│   ├── lezer-adapter/          # Exclusive boundary wrapping official LEZ v0.3.1 WalletCore & RPC
│   ├── lezer-core/             # Vault, cache, sessions, broker, intent summaries, auto-lock
│   ├── lezer-cli/              # `lezer` binary — full CLI parity + E2E sequencer tests
│   └── lezer-provider/         # Caller-authenticated provider state machine & background job runner
├── modules/
│   ├── lezer_wallet_provider/  # Logos Core module (LIDL cdylib shim)
│   ├── lezer_wallet_ui/        # Basecamp QML Wallet GUI (accounts, send/receive, tokens, approvals)
│   ├── lezer_testimonial_ui/   # LP-0021 On-Chain Testimonial Mini-App
│   └── lezer_faucet_ui/        # LP-0021 On-Chain Native LEZ Faucet Mini-App
├── programs/
│   ├── testimonial/            # SPEL program: 16-bucket paginated testimonial PDAs + RegisterSelf
│   ├── token/                  # SPEL program: fungible token definitions, holdings, authority + RegisterSelf
│   ├── faucet/                 # SPEL program: native LEZ custody_transfer vault PDA + rate-limit PDAs
│   └── registry/               # SPEL program: LP-0023 Program Registry (chained-call RegisterSelf)
├── packages/
│   ├── provider/               # `@lezer/provider` TypeScript SDK + MockLezerProvider
│   └── registry/               # `@lezer/registry` LP-0023 client & IDL helpers
├── templates/
│   └── lezer-dapp-template/    # Forkable Basecamp dApp starter
├── apps/
│   └── lezer-portal/           # Next.js static portal (docs, playground, adoption & registry views)
├── tests/
│   └── conformance/            # Error taxonomy conformance fixtures
└── tooling/                    # Boundary checks, module catalog validator, real-chain demo.sh
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
export LEZER_PASSWORD=…
./target/release/lezer init
echo "<32-byte-hex-signing-key>" | ./target/release/lezer account import --label main
./target/release/lezer balance main
./target/release/lezer send --from main --to <account> --amount 1000000000

# Local end-to-end (starts a standalone v0.3.1 sequencer):
./tooling/demo.sh
```

### Install Basecamp Modules

```bash
lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json
lgpd install lezer_wallet_provider lezer_wallet_ui lezer_testimonial_ui lezer_faucet_ui
```

### Run the Evaluator Demo Suite

```bash
./tooling/demo.sh
```

This starts (or reuses) a standalone LEZ v0.3.1 sequencer, then exercises the real chain:
init → import funded genesis key → public transfer → `lezer_testimonial` deploy/submit/
verify-adoption → `lezer_token` deploy/define/mint/transfer/burn + negative pre-checks →
`lezer_faucet` deploy/fund/claim/rate-limit check → `lezer_registry` deploy + chained-call
self-registration of all three programs. No simulation flags; every step lands on chain.
Full E2E also runs in `cargo test -p lezer-cli --test e2e_sequencer` (needs
`LEZER_E2E_SEQUENCER`, optional `LEZER_E2E_{TESTIMONIAL,TOKEN,REGISTRY,FAUCET}_ELF`,
`LEZER_E2E_PRIVATE=1` for the proving legs).

## Implementation Status (honest)

Lezer is under active development. The table below reflects what is verified
against a real LEZ sequencer today. Target protocol: official
`logos-blockchain/logos-execution-zone` **`v0.3.1`** (the version the public
testnet `https://testnet.lez.logos.co` runs).

| Capability | Status |
| :--- | :--- |
| Encrypted vault (`vault.enc`), auto-lock, redacted logs | ✅ Working |
| Approval broker (digest-bound approvals, private-balance gate, live LP-0023 badge lookup) | ✅ Working (unit-tested) |
| Accounts/balances/public transfers via official `WalletCore` | ✅ Verified on local v0.3.1 sequencer (real blocks, real fees) |
| Private transfers (shield / private / deshield), real ZK proofs | ✅ Verified on localnet (image-matched circuit; ~3–6 min, ~9 GB peak RSS per op) |
| `lezer_token` fungible tokens | ✅ Define/mint/transfer/burn/authority on localnet; pre-flight shard checks reject doomed calls locally |
| `lezer_faucet` (LP-0021 on-chain native LEZ faucet) | ✅ Treasury PDA `custody_transfer` + per-recipient rate-limit PDAs verified on localnet |
| `lezer_testimonial` (LP-0021) + `lezer_registry` (LP-0023, chained-call self-registration) | ✅ Built & deployed on localnet; `verify-adoption`/`registry list` are wallet-less chain reads |
| Basecamp core module (callable via LIDL) | 🚧 Authored + unit-tested (caller-authenticated); `.lgx` packaging needs a Nix toolchain — not verified locally |
| Basecamp QML wallet UI + testimonial & faucet mini-apps | 🚧 Implemented + headless-smoke-tested; not yet run inside a live Basecamp host |
| E2E tests against a real standalone sequencer | ✅ Passing (`e2e_sequencer`); CI job authored |
| Private token balances (`Private/` holdings) | ⏸ Not yet — upstream privacy path for arbitrary programs is future work |
| Browser extension, `lezerd`, WASM, Shamir recovery | ⏸ Post-prize (experimental, not built by default) |

Nothing in this repository reports a transaction as successful unless the
sequencer has accepted it — and, for program calls, the wallet pre-checks shard
state before signing so an "included" receipt means the program executed.

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — System design, component boundaries, trust model & release gates
- [`DEVELOPERS.md`](DEVELOPERS.md) — dApp integration guide (`@lezer/provider`, `LezerProvider.qml`, LP-0023)
- [`PRIVACY.md`](PRIVACY.md) — Cryptography, threat model, zero-telemetry enforcement & audit details

## License

Dual-licensed under **MIT OR Apache-2.0**. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

OSS dependency compliance: [`THIRD_PARTY.md`](THIRD_PARTY.md).

---

<div align="center">

**Built for the Logos Execution Zone · LP-0021 & LP-0023 · `lp0021-lezer`**

[Repository](https://github.com/zLabsHQ/lezer-wallet) · [Issues](https://github.com/zLabsHQ/lezer-wallet/issues) · [Releases](https://github.com/zLabsHQ/lezer-wallet/releases)

</div>
