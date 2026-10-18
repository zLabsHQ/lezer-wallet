# Lezer Wallet — System Architecture

> **For users, integrators, and security reviewers.** This document describes the technical architecture of Lezer Wallet, including the privacy model, cryptographic design, component boundaries, and trust assumptions.

---

## 1. Design Philosophy

Lezer Wallet is built on three non-negotiable principles:

1. **Rust is the Sole Source of Wallet Truth.** All cryptographic operations, key custody, session management, approval enforcement, and state transitions live inside `lezer-core` — a native Rust engine with zero JavaScript/QML dependencies.
2. **Basecamp QML is the Native Launch UI.** The wallet runs as a first-class Logos Basecamp module (`lezer_wallet_ui`) with a C-ABI provider (`lezer_wallet_provider`) — no Electron, no WebView wrapper, no remote RPC.
3. **Zero Blind Signing, Zero Telemetry.** Every transaction is decoded into human-readable call effects before signing. No analytics, no third-party APIs, no external endpoints beyond the user's configured LEZ sequencer and optional Logos Storage node.

---

## 2. Layered Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         USER SURFACES                                │
│  ┌────────────┐  ┌────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │  lezer     │  │ lezer_     │  │  apps/       │  │  lezer     │  │
│  │  (CLI)     │  │ wallet_ui  │  │  lezer-portal│  │  extension │  │
│  │            │  │  (QML)     │  │  (Next.js)   │  │  (MV3)     │  │
│  └─────┬──────┘  └─────┬──────┘  └──────┬───────┘  └─────┬──────┘  │
│        │               │                │                │         │
│  ┌─────┴───────────────┴────────────────┴────────────────┴─────┐   │
│  │              IPC / LIDL / JSON-RPC Boundary                  │   │
│  │         (Session tokens, capability ACLs, no secrets)        │   │
│  └────────────────────────────┬───────────────────────────────┘   │
└───────────────────────────────┼───────────────────────────────────┘
                                │
┌───────────────────────────────┼───────────────────────────────────┐
│                    NATIVE RUST CORE (`lezer-core`)                 │
│  ┌────────────────────────────┴───────────────────────────────┐   │
│  │  Vault  │  Cache  │  Sessions  │  Broker  │  Decoder  │    │   │
│  │  AutoLock │ ProverQueue │ Registry │ Storage │ Summary │    │   │
│  │  Errors │ Logging │ Migration │ Concurrency │ Testimonial│    │   │
│  └────────────────────────────┬───────────────────────────────┘   │
│                               │                                   │
│  ┌────────────────────────────┴───────────────────────────────┐   │
│  │                  lezer-adapter                             │   │
│  │  RPC │ Profile │ Accounts │ Transfer │ Token │ ATA │      │   │
│  │  Faucet │ Simulate │ Prover │ Codecs │                   │   │
│  │  (Sole importer of upstream `wallet`, `lee_core`, SPEL)    │   │
│  └────────────────────────────┬───────────────────────────────┘   │
└───────────────────────────────┼───────────────────────────────────┘
                                │ JSON-RPC over HTTPS
┌───────────────────────────────┼───────────────────────────────────┐
│                 LOGOS EXECUTION ZONE (LEZ)                         │
│         Sequencer: https://testnet.lez.logos.co                    │
│    Public Accounts │ Private Accounts (ZK/ML-KEM) │ SPEL Programs │
└────────────────────────────────────────────────────────────────────┘
```

---

## 3. Component Responsibilities

### 3.1 `lezer-adapter` — Protocol Boundary Layer

The **only** crate permitted to import upstream LEZ crates (`wallet`, `lee_core`, `nssa`, `spel`, `risc0-zkvm`).

- `profile.rs`: `NetworkProfile` (`lez-testnet-0.3`, `lez-testnet-0.4`, `standalone`) + `AllowedEndpointsGuard` (zero-telemetry egress allowlist).
- `rpc.rs`: Sequencer JSON-RPC client (`getProgramIds`, `getLastBlockId`, `getAccount`, `sendTransaction`, `getTransactionReceipt`).
- `accounts.rs`: `Public/<base58>` Ed25519 derivation (`m/44'/9999'/0'/0/i`), `Private/<base58>` ML-KEM-768 + Ed25519 derivation (`m/44'/9999'/1'/0/i`), `lezer:priv1...` URI codec, view-tag scanning.
- `transfer.rs`: `TransferPrivacyMode` classification (`PublicToPublic`, `ShieldPublicToPrivate`, `DeshieldPrivateToPublic`, `PrivateToPrivate`) + ZK transfer executor.
- `token.rs`: `TOKEN_ID` facade (`NewFungibleDefinition`, `Mint`, `Transfer`, `Burn`, `SetAuthority`).
- `ata.rs`: Deterministic `SHA256(owner || definition)` ATA derivation for public and private owners.
- `faucet.rs`: Piñata PoW challenge reader, multi-threaded SHA-256 PoW solver, stale-challenge retry.
- `simulate.rs`: Pre-flight balance delta calculator + deterministic revert detector (`SimDeterministicRevert`).
- `prover.rs`: 4-stage ZK pipeline (`Witness → r0vm → RapidSnark → Broadcast`) with wall-clock + peak-RSS telemetry.
- `codecs.rs`: RISC0 `[u32; 8]` word serialization, program-ID hex round-trips.

### 3.2 `lezer-core` — Native Wallet Engine

Owns all wallet truth. Isolated from upstream protocol crates.

- `vault.rs`: `SealedVaultEnvelopeV1` (Argon2id → XChaCha20-Poly1305 → SHA-256 AAD MAC). Atomic write-temp-fsync-rename. BIP-39 mnemonic generation/validation. `PendingPrivateSpend` nullifier journal.
- `cache.rs`: `CacheEnvelopeV1` rebuildable `~/.lezer/cache.json` for Offline Mode balance inspection.
- `migration.rs`: Forward envelope migration with automatic `.bak` backup.
- `autolock.rs`: `AutoLockFsm` 15-minute inactivity timer + `DeferredLockPending` for in-flight operations.
- `broker.rs`: `ApprovalBroker` + `SessionGrant` + `verify_session_balance_permission` + `take_verified_proposal` (enforces `LZR-3005` proposal digest mismatch and `LZR-3004` unacknowledged risk gate).
- `decoder.rs`: Tier-1 built-in decoders for `AUTHENTICATED_TRANSFER_ID`, `TOKEN_ID`, `ASSOCIATED_TOKEN_ACCOUNT_ID`, `PINATA_ID`, `lezer_testimonial`. Critical-red override gate for `SetAuthority` mutations.
- `summary.rs`: Human-readable `TransactionSummary` + risk warnings.
- `registry.rs`: LP-0023 client — `register_program_entry`, `lookup_program_entry`, `verify_program_bytecode` (4-state badge).
- `testimonial.rs`: Sharded submitter (16 shards, 32-entry page rollover) + stateless paginator for `verify-adoption`.
- `storage.rs`: Client-side encrypted `SealedVaultEnvelopeV1` backup push/pull via Logos Storage/Codex CID.
- `concurrency.rs`: `ProverQueue` with `tokio::sync::Semaphore(1)` + live 4-stage progress streaming.
- `errors.rs`: Unified `LezerError` + `LZR-1xxx..6xxx` taxonomy.
- `logging.rs`: Zero-telemetry `AuditLogger` + `Redacted<T>` secret wrapper.
- `state.rs`: Canonical `WalletStateSnapshot` + `WalletEngine`.

### 3.3 `lezer-cli` — Full-Parity CLI

`lezer` binary with complete feature parity to the GUI. All output is `--json` compatible with stdout isolation (`libc::dup(1)`/`dup2(2,1)`).

Commands: `init`, `restore`, `unlock`, `lock`, `status`, `account new|list|receive`, `balance`, `transfer`, `faucet info|claim`, `token create|ata-address|ata-create|send|mint|set-authority|burn|balances`, `testimonial submit|list`, `registry lookup|list|search|register|verify`, `backup push|pull`, `verify-adoption`.

### 3.4 `lezer-wasm` — Gear 1 Pure-WASM Signing

Pure-browser Ed25519 public account derivation and transaction signing for `apps/lezer-extension`. Zero native binary requirement.

### 3.5 `lezerd` — Gear 2 Native Messaging Daemon

Chromium/Firefox Native Messaging host daemon. Speaks 4-byte little-endian length-prefixed JSON framing over `stdin`/`stdout`. Delegates private RISC0 + RapidSnark Groth16 proofs to the local machine.

### 3.6 `modules/lezer_wallet_provider` — LIDL Core Module

`type: "core"`, `interface: "cdylib"`. Implements `connect`, `get_request_status`, `get_accounts`, `get_balance`, `propose_transaction`, `execute_privileged`. All methods enforce session token + capability ACL checks inside Rust.

### 3.7 `modules/lezer_wallet_ui` — Basecamp QML Wallet

`type: "ui_qml"`. Views: `OnboardingView`, `PortfolioView`, `SendTransferView`, `TokenManagerView`, `ActivityProverView`, `ConnectedAppsView`, `SettingsView`. Modals: `ConnectPromptSheet`, `ApprovalSheet` (with public-on-chain privacy banner + LP-0023 verification badge + gas/cycle estimate).

### 3.8 `modules/lezer_faucet_ui` & `modules/lezer_testimonial_ui`

Reference `ui_qml` mini-apps demonstrating `@lezer/provider` + `LezerProvider.qml` integration.

### 3.9 `programs/testimonial` & `programs/registry`

SPEL RISC0 guest programs. `testimonial`: per-author PDA + 16-shard SHA-256 paginated index PDAs (`["testimonial-index", submission_id, shard_key, page_key]`). `registry`: LP-0023 `register_self` chained-call authentication + 4-state verification.

### 3.10 `packages/provider`, `packages/registry`, `packages/recovery`

- `@lezer/provider`: TypeScript SDK + `LezerProvider.qml` QML singleton + `MockLezerProvider` for testing.
- `@lezer/registry`: LP-0023 client, RISC0 serde IDL decoder, PDA derivation helpers, Logos Storage fetcher.
- `@lezer/recovery`: GF(256) 2-of-3 Shamir Secret Sharing + WebAuthn Passkey PRF + `lezer:recovery1:` URI codec.

---

## 4. Privacy & Security Model

### 4.1 Private Account Cryptography

| Component | Algorithm | Purpose |
| :--- | :--- | :--- |
| Nullifier Keypair (`npk`/`nsk`) | Ed25519 | Spends private notes; nullifies spent commitments |
| Viewing Keypair (`vpk`/`vsk`) | ML-KEM-768 | Decrypts incoming notes without spending authority |
| Receive URI | `lezer:priv1...` | Checksummed single-string encoding of `(vpk, npk)` |
| Note Encryption | ML-KEM-768 + XChaCha20-Poly1305 | On-chain notes are Pedersen commitments; only `vpk` holder can decrypt |
| View-Tag Filter | 4-byte prefix | ~99% skip rate for unrelated notes before decapsulation |

### 4.2 Vault Encryption

```
SealedVaultEnvelopeV1:
  salt       = random(16 bytes)
  argon2id   = Argon2id(password, salt, t=3, m=64MiB, p=4)
  wrapping_key = argon2id[0..32]
  header_mac = SHA256(version || salt || argon2id_params || ciphertext_len)
  nonce      = random(24 bytes)
  ciphertext = XChaCha20-Poly1305(wrapping_key, nonce, plaintext_vault)
```

- **Atomic Persistence:** `vault.enc.tmp.<pid>` → fsync → rename to `vault.enc`.
- **Crash Recovery:** Corrupted/missing cache rebuilds from chain; vault migration creates `.bak` before upgrade.
- **Zeroization:** `Redacted<T>` + `zeroize` on drop for all key material.

### 4.3 Session & Approval Gates

```
dApp → LIDL → lezer_wallet_provider → SessionBroker (lezer-core)
                                      │
                                      ├─ session_token issued ONLY after
                                      │  user approves ConnectPromptSheet
                                      │
                                      ├─ get_balance(Private/<id>) requires
                                      │  PermissionScope::ReadPrivateBalance
                                      │  for that specific account
                                      │
                                      ├─ propose_transaction requires
                                      │  decoded summary + explicit approval
                                      │  (ApproveDecision)
                                      │
                                      └─ execute_privileged requires
                                         internal ui_token (wallet UI only)
```

### 4.4 Zero-Telemetry Guard

`AllowedEndpointsGuard` in `lezer-adapter` enforces a strict egress allowlist:
- `sequencer_url` (LEZ sequencer)
- `lez_node_url` (optional local node)
- `logos_storage_url` (optional Codex backup node)

Any other outbound HTTP request is rejected with `LZR-4002`. Verified by `tooling/check-boundaries.sh`.

---

## 5. Trust Boundaries & Threat Model

| Boundary | Trust Level | Mitigation |
| :--- | :--- | :--- |
| `lezer-core` ↔ `lezer-adapter` | Trusted (same process) | Compile-time isolation; adapter is the only upstream importer |
| `lezer-core` ↔ `lezer_wallet_provider` | Trusted (same process) | Session token + capability ACL enforced in Rust |
| `lezer_wallet_provider` ↔ `lezer_wallet_ui` | Untrusted (QML/JS) | `ui_token` required for `execute_privileged`; no secret exposure |
| `lezer_wallet_ui` ↔ dApp (`lezer_faucet_ui`, `lezer_testimonial_ui`) | Untrusted (QML/JS) | Session token issued only after user approval; scoped ACLs |
| `lezer-core` ↔ Sequencer | Untrusted (network) | TLS + JSON-RPC; no auth secrets sent |
| `lezer-core` ↔ Logos Storage | Untrusted (network) | `SealedVaultEnvelopeV1` encrypted client-side; CID-addressed |
| `apps/lezer-extension` ↔ `lezerd` | Untrusted (native messaging) | 4-byte LE framing; extension never holds private keys |

---

## 6. Release & Verification Gates

| Gate | Description | Test |
| :--- | :--- | :--- |
| RC-01 | dApp connect → approval sheet → testimonial submission | `lezer_wallet_provider` unit test |
| RC-02 | Live private native/token transfers, deferred lock, crash recovery | `lezer-core/tests/live_private_token_transfer.rs` |
| RC-03 | 50-account concurrent testimonial shard overflow + stateless verify | `lezer-cli/tests/concurrent_50_testimonials_shard_overflow.rs` |
| Boundary | No upstream imports in `lezer-core`; no secrets in `modules/`/`apps/` | `tooling/check-boundaries.sh` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | CI |
| E2E | `./tooling/demo.sh` — clean-room evaluator suite | CI |

---

## 7. Forward Design (Post-Prize)

- **LP-0022 Multi-Zone & L1 Staking:** `NetworkProfile` abstraction already supports additional zones.
- **Mobile (iOS/Android):** `lezer-core` compiles to `aarch64-apple-ios` / `aarch64-linux-android`; `lezerd` daemon pattern extends to native mobile.
- **2-of-3 Shamir Recovery:** `@lezer/recovery` package provides GF(256) share splitting + WebAuthn PRF sealing + `lezer:recovery1:` URIs.
- **Browser Extension:** `apps/lezer-extension` (MV3) + `lezer-wasm` (Gear 1) + `lezerd` (Gear 2) — live in repo.
