# Lezer Wallet (`lezer-wallet`)

**The Native Privacy-Preserving Execution Zone Wallet, Provider SDK & Program Verification Suite for Logos**

> **Document Suite Version: `3.3.0` (Final Locked Specification)**  
> **LP-0021 / LP-0023 Launch Scope:** Native Rust Core (`lezer-adapter`, `lezer-core`), Logos Basecamp Core Module (`lezer_wallet_provider`), Basecamp Native QML UI (`lezer_wallet_ui`, `lezer_faucet_ui`, `lezer_testimonial_ui`), `lezer` CLI, `@lezer/provider` SDK, SPEL Programs (`testimonial`, `registry`), and Non-Custodial Next.js Docs/Explorer Portal (`apps/lezer-portal`).  
> **Strictly Post-Prize Scope (`docs/07`):** Web Browser Extension, `lezer-wasm`, `lezerd` Native Messaging, Mobile (iOS/Android), 2-of-3 Shamir/Passkey Recovery (`pointer` program), and LP-0022 Multi-Zone / L1 Staking.

| Attribute | Canonical Value |
| :--- | :--- |
| **Product Name** | **Lezer Wallet** |
| **Repository** | `lezer-wallet` |
| **Primary λPrize Target** | **LP-0021: LEZ Wallet and Provider SDK** ($20,000, X-Large) |
| **Companion λPrize Target** | **LP-0023: LEZ Program Registry** ($2,000, Medium — Integrated Source Verification) |
| **Forward-Design Seam** | **LP-0022: Combined Blockchain and Zone Wallet** (L1, Staking, Bridging, Multi-Zone) |
| **Frozen Submission ID** | `lp0021-lezer` |
| **Logos Core Module ID** | `lezer_wallet_provider` (`type: "core"`, `interface: "cdylib"`, LIDL-backed) |
| **Logos Basecamp UI Module ID** | `lezer_wallet_ui` (`type: "ui_qml"`, `interface: "universal"`) |
| **Reference Mini-App Modules** | `lezer_faucet_ui`, `lezer_testimonial_ui` (`type: "ui_qml"`) |
| **CLI Binary** | `lezer` (Full-parity CLI + `lezer verify-adoption --submission-id lp0021-lezer`) |
| **SDK Packages** | `@lezer/provider`, `@lezer/registry` (plus `LezerProvider.qml` for QML apps) |
| **Target Network** | Logos Execution Zone Testnet v0.3 / v0.4 (`https://testnet.lez.logos.co`) |
| **License** | Dual-licensed under `MIT OR Apache-2.0` |

---

## Executive Summary

**Lezer Wallet** is the canonical, production-grade wallet, intent broker, and developer SDK for the **Logos Execution Zone (LEZ)**. Named after the community identity for LEZ builders and runners (*"LEZers"*), Lezer solves the foundational adoption bottleneck of the Logos ecosystem: giving users a unified, non-custodial, privacy-first key and asset manager while giving third-party Basecamp dApp developers a standard **Wallet Provider SDK** (`@lezer/provider` + `lezer_wallet_provider` LIDL module) with zero-blind-signing approval prompts and on-chain **LP-0023** program source verification.

Unlike basic CLI wrappers or partial prototypes, Lezer is engineered from day one around the complete **LEZ v0.3 / v0.4** specification:

1. **Full Asset & Privacy Matrix:** Native LEZ (`AUTHENTICATED_TRANSFER_ID`) and Fungible Token Program (`TOKEN_ID`) + Associated Token Accounts (`ASSOCIATED_TOKEN_ACCOUNT_ID`) across all four privacy permutations (`Public -> Public`, `Public -> Private` Shielding, `Private -> Public` Deshielding, and `Private -> Private` ZK transfers).
2. **True In-Daemon Security & Session Broker:** Enforces strict per-dApp origin/module authentication, scoped session tokens, and explicit per-account private balance gates inside the Rust core (`lezer-core`), closing the critical LIDL privilege-escalation vulnerability present in competing implementations.
3. **Semantic Call-Effect Decoder (Zero Blind Signing):** Built-in binary deserializers for LEZ native programs (`AUTHENTICATED_TRANSFER_ID`, `TOKEN_ID`, `ASSOCIATED_TOKEN_ACCOUNT_ID`, `PINATA_ID`) combined with dynamic **SPEL IDL** resolution from **Logos Storage** via the on-chain **LP-0023 Program Registry**. Highlights exact token outflows, authority rotations (`SetAuthority`), and unlimited approvals before signing.
4. **Zero Mandatory Third-Party Services:** Talks exclusively to the user's configured LEZ sequencer (`https://testnet.lez.logos.co` or local node) and optional **Logos Storage (Codex)** nodes for client-side encrypted vault backup (`SealedVaultEnvelopeV1` sealed with `Argon2id` + `XChaCha20-Poly1305`). Zero telemetry by default.

---

## Documentation Architecture (`docs/`)

This folder contains the complete, industry-standard engineering and product documentation for **Lezer Wallet**:

| Document | Title | Scope & Contents |
| :--- | :--- | :--- |
| **[`docs/00-INDEX.md`](docs/00-INDEX.md)** | **Master Documentation Index & Traceability Matrix** | Complete LP-0021 / LP-0023 / LP-0022 requirement traceability matrix, glossary of LEZ primitives, and architectural decision records (ADRs). |
| **[`docs/01-SYSTEM-ARCHITECTURE.md`](docs/01-SYSTEM-ARCHITECTURE.md)** | **System Architecture & Cryptographic Engine** | Workspace layout, `lezer-core` + `lezer-adapter` native Rust engine, sealed vault (`Argon2id` + `XChaCha20-Poly1305`), ED25519 + ML-KEM-768 / ZK note management, Basecamp C-ABI & QML WebView IPC, and Logos Storage encrypted vault backup. |
| **[`docs/02-FEATURES-AND-PRODUCT-SPEC.md`](docs/02-FEATURES-AND-PRODUCT-SPEC.md)** | **Product Features, Token/ATA Engine & UX Specification** | Complete functional specification: public/private accounts, `TOKEN_ID` + `ASSOCIATED_TOKEN_ACCOUNT_ID` (public & private ATAs, LP-0013/LP-0014 integration), Connect & 4-State Approval Sheet UX wireframes, public testimonial privacy warning, gas/cycle estimation, and `lezer_faucet_ui` + `lezer_testimonial_ui` specs. |
| **[`docs/03-PROTOCOL-SDK-AND-IPC-SPEC.md`](docs/03-PROTOCOL-SDK-AND-IPC-SPEC.md)** | **Wire Protocol, LIDL Contract & `@lezer/provider` SDK** | Complete `lezer_wallet_provider.lidl` interface, Rust `Op`/`Output` JSON wire protocol, session capability ACLs, `@lezer/provider` TypeScript & QML SDK API reference, and semantic instruction decoder specification. |
| **[`docs/04-ONCHAIN-PROGRAMS-AND-REGISTRY.md`](docs/04-ONCHAIN-PROGRAMS-AND-REGISTRY.md)** | **On-Chain SPEL Programs & LP-0023 Registry** | Full Rust guest specifications for `lezer-testimonial` (with **SHA-256 16-shard paginated on-chain index PDAs** `["testimonial-index", submission_id, shard_key, page_key]` + `lezer verify-adoption --submission-id lp0021-lezer`) and `lezer-registry` (LP-0023 `register_self` chained-call proof + 4-state source verification badge), plus RISC Zero cycle benchmarks. |
| **[`docs/05-COMPETITOR-TEARDOWN-AND-EDGE.md`](docs/05-COMPETITOR-TEARDOWN-AND-EDGE.md)** | **Competitor Teardown (`widespread-logos`) & Lezer's Winning Edge** | Code-level audit of `widespread-logos` (`TerexitariusStomp/widespread-logos`), identifying 7 critical compliance/security gaps in their codebase and detailing how Lezer Wallet solves every single one. |
| **[`docs/06-PACKAGING-CI-AND-EXECUTION-ROADMAP.md`](docs/06-PACKAGING-CI-AND-EXECUTION-ROADMAP.md)** | **Nix Packaging, CI/CD, Master 24-Day Schedule & 60-Day Adoption Playbook** | **Canonical source of truth for dates, deliverables, CI gates & release checks.** `mkLogosModule` / `mkLogosQmlModule` flakes, `logos-repo.json` catalog pipeline, 3 Release-Critical Checks (`RC-01` to `RC-03` including 50-account shard contention & overflow test), 4 Vertical Slices, and 60-day adoption engine. |
| **[`docs/07-VISION-AND-PLATFORM-STRATEGY.md`](docs/07-VISION-AND-PLATFORM-STRATEGY.md)** | **Post-Prize Vision & Platform Expansion (Web Extension, Mobile, Shamir & LP-0022)** | Post-LP-0021 roadmap: Manifest V3 Browser Extension, iOS/Android Mobile Wallet, 2-of-3 Shamir + Passkey PRF recovery, and LP-0022 Multi-Zone / L1 Staking. |
| **[`docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md`](docs/08-ARCHITECTURE-DECISIONS-AND-STACK.md)** | **Architecture Decisions (v3.3 — Final Locked Basecamp-First & Rust-Core ADR)** | Locks in Rust-owned `WalletStateSnapshot`, 5-step immutable `TransactionSummary` -> `ApproveDecision` pipeline, `vault.enc` vs `cache.json` storage/migration split, pre-flight simulation policy, offline mode, `LZR-1xxx..6xxx` error taxonomy, concurrency table, redacted logging, and conformance test suite. |
| **[`docs/09-REPOSITORY-AND-FOLDER-STRUCTURE.md`](docs/09-REPOSITORY-AND-FOLDER-STRUCTURE.md)** | **Complete Repository & Folder Structure Blueprint** | File-by-file monorepo tree (`crates/*`, `modules/*`, `programs/*`, `packages/provider`, `packages/registry`, `templates/*`, `apps/lezer-portal`, `tests/conformance`, `tooling/*`), strict compile-time import boundary diagram, and `~/.lezer/` runtime disk layout (`vault.enc` vs `cache.json`). |
| **[`docs/10-PHASE-WISE-IMPLEMENTATION-PLAN.md`](docs/10-PHASE-WISE-IMPLEMENTATION-PLAN.md)** | **Phase-Wise Implementation Plan (`PHASE.md`)** | Sequential Phase 0 → Phase 5 engineering execution plan detailing exact files created, features implemented, and conformance/release gate commands per phase. |

---

## Target Monorepo Layout (`lezer-wallet` — LP-0021 Basecamp-First Scope)

```text
lezer-wallet/
├── Cargo.toml                         # Rust workspace root (pinned to LEZ v0.3 / v0.4)
├── Cargo.lock
├── package.json                       # TypeScript/Node workspace root
├── logos-repo.json                    # Basecamp module catalog descriptor (lgpd installable)
├── LICENSE-MIT
├── LICENSE-APACHE
├── THIRD_PARTY.md                     # Complete OSS dependency & LGPL/MIT/Apache compliance ledger
├── crates/
│   ├── lezer-adapter/                 # Isolated LEZ v0.3/v0.4 RPC, Token, ATA, Faucet & Prover wrapper
│   ├── lezer-core/                    # SealedVaultEnvelopeV1, cache.json, 15m Auto-Lock, Session Broker, Prover Queue & Decoder
│   └── lezer-cli/                     # `lezer` binary — full CLI parity + `lezer verify-adoption --submission-id lp0021-lezer`
├── modules/
│   ├── lezer_wallet_provider/         # Logos Core module (`type: "core"`, `cdylib`, `lezer_wallet_provider.lidl`)
│   ├── lezer_wallet_ui/               # Logos Basecamp Native QML Wallet GUI (`type: "ui_qml"`, `mkLogosQmlModule`)
│   ├── lezer_faucet_ui/               # Reference Faucet & Funding Mini-App (`type: "ui_qml"`)
│   └── lezer_testimonial_ui/          # Reference On-Chain Testimonial Mini-App (`type: "ui_qml"`)
├── programs/
│   ├── testimonial/                   # SPEL RISC0 program: per-author PDA + 16-shard SHA-256 paginated index PDAs
│   └── registry/                      # SPEL RISC0 program: LP-0023 Program Registry (`register_self` chained-call auth)
├── packages/
│   ├── provider/                      # `@lezer/provider` + `LezerProvider.qml` + `MockLezerProvider`
│   └── registry/                      # `@lezer/registry` — LP-0023 client, RISC0 serde IDL decoder & verifier
├── templates/
│   └── lezer-dapp-template/           # Ready-to-fork Basecamp `ui_qml` dApp starter for the 10 external developers
├── apps/
│   └── lezer-portal/                  # Next.js 15 Static Portal (`output: 'export'`): Docs, Playground, Explorer, Adoption
├── tests/
│   ├── conformance/                   # Mandatory golden JSON fixtures validated across Core, CLI, LIDL & SDK
│   └── e2e/                           # Standalone sequencer E2E + Gates RC-01, RC-02, RC-03
├── tooling/
│   ├── check-boundaries.sh            # Enforces crate/module/app import boundaries & secret-redaction lint
│   ├── gen-conformance-fixtures.rs    # Regenerates & validates golden JSON vectors
│   ├── run-spikes.sh                  # Executes Day 1–2 Spikes S-01, S-02, and S-03
│   ├── verify-program.sh              # Reproducible Docker RISC0 ELF builder & image-ID bytecode verifier
│   └── demo.sh                        # Unmodified clean-environment evaluator E2E script
└── docs/                              # Industry-standard architecture & specification suite (00–10)
```
