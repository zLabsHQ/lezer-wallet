# Lezer Wallet (`lezer-wallet`)

**The Native Privacy-Preserving Execution Zone Wallet, Provider SDK & Program Verification Suite for Logos**

> **LP-0021 / LP-0023 Launch Scope:** Native Rust Core (`lezer-adapter`, `lezer-core`), Logos Basecamp Core Module (`lezer_wallet_provider`), Basecamp Native QML UI (`lezer_wallet_ui`, `lezer_faucet_ui`, `lezer_testimonial_ui`), `lezer` CLI, `@lezer/provider` SDK, SPEL Programs (`testimonial`, `registry`), and Non-Custodial Next.js Docs/Explorer Portal (`apps/lezer-portal`).

| Attribute | Canonical Value |
| :--- | :--- |
| **Product Name** | **Lezer Wallet** |
| **Repository** | [`github.com/zLabsHQ/lezer-wallet`](https://github.com/zLabsHQ/lezer-wallet) |
| **Primary λPrize Target** | **LP-0021: LEZ Wallet and Provider SDK** ($20,000, X-Large) |
| **Companion λPrize Target** | **LP-0023: LEZ Program Registry** ($2,000, Medium — Integrated Source Verification) |
| **Frozen Submission ID** | `lp0021-lezer` |
| **Logos Core Module ID** | `lezer_wallet_provider` (`type: "core"`, `interface: "cdylib"`, LIDL-backed) |
| **Logos Basecamp UI Module ID** | `lezer_wallet_ui` (`type: "ui_qml"`, `interface: "universal"`) |
| **Reference Mini-App Modules** | `lezer_faucet_ui`, `lezer_testimonial_ui` (`type: "ui_qml"`) |
| **CLI Binary** | `lezer` (Full-parity CLI + `lezer verify-adoption --submission-id lp0021-lezer`) |
| **SDK Packages** | `@lezer/provider`, `@lezer/registry`, `@lezer/recovery` (plus `LezerProvider.qml` for QML apps) |
| **Target Network** | Logos Execution Zone Testnet v0.3 / v0.4 (`https://testnet.lez.logos.co`) |
| **License** | Dual-licensed under `MIT OR Apache-2.0` |

---

## Executive Summary

**Lezer Wallet** is the canonical, production-grade wallet, intent broker, and developer SDK for the **Logos Execution Zone (LEZ)**. Named after the community identity for LEZ builders and runners (*"LEZers"*), Lezer solves the foundational adoption bottleneck of the Logos ecosystem: giving users a unified, non-custodial, privacy-first key and asset manager while giving third-party Basecamp dApp developers a standard **Wallet Provider SDK** (`@lezer/provider` + `lezer_wallet_provider` LIDL module) with zero-blind-signing approval prompts and on-chain **LP-0023** program source verification.

Unlike basic CLI wrappers or partial prototypes, Lezer is engineered from day one around the complete **LEZ v0.3 / v0.4** specification:

1. **Full Asset & Privacy Matrix:** Native LEZ (`AUTHENTICATED_TRANSFER_ID`) and Fungible Token Program (`TOKEN_ID`) + Associated Token Accounts (`ASSOCIATED_TOKEN_ACCOUNT_ID`) across all four privacy permutations (`Public -> Public`, `Public -> Private` Shielding, `Private -> Public` Deshielding, and `Private -> Private` ZK transfers).
2. **True In-Daemon Security & Session Broker:** Enforces strict per-dApp origin/module authentication, scoped session tokens, and explicit per-account private balance gates inside the Rust core (`lezer-core`), closing critical IPC privilege-escalation vulnerabilities.
3. **Semantic Call-Effect Decoder (Zero Blind Signing):** Built-in binary deserializers for LEZ native programs (`AUTHENTICATED_TRANSFER_ID`, `TOKEN_ID`, `ASSOCIATED_TOKEN_ACCOUNT_ID`, `PINATA_ID`) combined with dynamic **SPEL IDL** resolution from **Logos Storage** via the on-chain **LP-0023 Program Registry**. Highlights exact token outflows, authority rotations (`SetAuthority`), and unlimited approvals before signing.
4. **Zero Mandatory Third-Party Services:** Talks exclusively to the user's configured LEZ sequencer (`https://testnet.lez.logos.co` or local node) and optional **Logos Storage (Codex)** nodes for client-side encrypted vault backup (`SealedVaultEnvelopeV1` sealed with `Argon2id` + `XChaCha20-Poly1305`). Zero telemetry by default.

---

## Monorepo Layout

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
│   ├── lezer-cli/                     # `lezer` binary — full CLI parity + `lezer verify-adoption --submission-id lp0021-lezer`
│   ├── lezer-wasm/                    # Pure-WASM Gear 1 signing engine for Browser Extension
│   └── lezerd/                        # Chromium/Firefox Native Messaging host daemon (Gear 2 local prover)
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
│   ├── registry/                      # `@lezer/registry` — LP-0023 client, RISC0 serde IDL decoder & verifier
│   └── recovery/                      # `@lezer/recovery` — 2-of-3 Shamir Secret Sharing & Passkey Recovery Kit
├── templates/
│   └── lezer-dapp-template/           # Ready-to-fork Basecamp `ui_qml` dApp starter for the 10 external developers
├── apps/
│   ├── lezer-portal/                  # Next.js 15 Static Portal (`output: 'export'`): Docs, Playground, Explorer, Adoption
│   └── lezer-extension/               # Manifest V3 Web Browser Extension (window.lezer + lezerd bridge)
├── tests/
│   ├── conformance/                   # Mandatory golden JSON fixtures validated across Core, CLI, LIDL & SDK
│   └── e2e/                           # Standalone sequencer E2E + Gates RC-01, RC-02, RC-03
├── tooling/
│   ├── check-boundaries.sh            # Enforces crate/module/app import boundaries & secret-redaction lint
│   ├── gen-conformance-fixtures.rs    # Regenerates & validates golden JSON vectors
│   ├── run-spikes.sh                  # Executes Day 1–2 Spikes S-01, S-02, and S-03
│   ├── verify-program.sh              # Reproducible Docker RISC0 ELF builder & image-ID bytecode verifier
│   ├── release-catalog.sh             # Generates `.lgx` module packages + `index.json` catalog
│   └── demo.sh                        # Unmodified clean-environment evaluator E2E script
└── .github/workflows/                 # Hermetic Nix CI + Basecamp `.lgx` catalog publishing
```

---

## Quickstart Guide (Evaluator & Developer)

### 1. Build the Native `lezer` CLI
```bash
cargo build -p lezer-cli --release
```

### 2. Initialize & Unlock the Vault
```bash
lezer init --mnemonic <your-24-word-mnemonic> --password <password>
lezer unlock --password <password>
```

### 3. Create Accounts & Manage Assets
```bash
# Public Account
lezer account new --privacy public --label main
# Private Shielded Account (derives lezer:priv1... URI)
lezer account new --privacy private --label savings

# Faucet Claim & Public Token Operations
lezer faucet claim --auto-shield-to savings
lezer token create --name LEZERUSD --symbol LEZU --decimals 18 --initial-supply 1000
lezer token ata-create --definition 0x... --owner Public/<acct>
lezer token send --from main --to Public/<recipient> --amount 50
```

### 4. LP-0023 Program Registry & Encrypted Backup
```bash
lezer registry lookup --program lezer_testimonial
lezer registry verify --program lezer_testimonial --bytecode <bytes>
lezer backup push --node http://localhost:8080
lezer backup pull --cid zDvZRwzm... --password <password>
```

### 5. Stateless 16-Shard Adoption Verification (`LEZER_NO_VAULT=1`)
```bash
LEZER_NO_VAULT=1 lezer verify-adoption --submission-id lp0021-lezer
```

### 6. Basecamp Integration (`lgpd` Module Catalog)
```bash
lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json
lgpd install lezer_wallet_provider lezer_wallet_ui lezer_faucet_ui lezer_testimonial_ui
```
