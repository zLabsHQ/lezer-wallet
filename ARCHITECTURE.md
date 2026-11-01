# Lezer Wallet — System Architecture

> **For users, integrators, and security reviewers.** This document describes the production architecture of Lezer Wallet on **Logos Execution Zone (LEZ) `v0.3.1`**, including the privacy model, cryptographic design, component boundaries, and trust assumptions.

---

## 1. Design Philosophy

Lezer Wallet is built on three non-negotiable principles:

1. **Rust is the Sole Source of Wallet Truth.** All key custody, encrypted vault persistence, session management, approval digest enforcement, and chain interactions live inside `lezer-core` and `lezer-adapter` (wrapping the official upstream `wallet::WalletCore` `v0.3.1`).
2. **Basecamp QML is the Native Launch UI.** The wallet runs as a Logos Basecamp module (`lezer_wallet_ui`) paired with a caller-authenticated C-ABI provider (`lezer_wallet_provider`) — privileged operations check `logos_rust_sdk::current_caller()` rather than any shared string secret.
3. **Zero Blind Signing, Zero Charged Reverts, Zero Telemetry.** Every dApp proposal is summarized into a canonical `TransactionSummary` bound by SHA-256 digest, enriched with a live on-chain `lezer_registry` (LP-0023) source-status badge, and pre-checked against shard state before signing so doomed calls are rejected locally before broadcast.

---

## 2. Layered Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         USER & DAPP SURFACES                        │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────┐  │
│  │   lezer    │  │ lezer_wallet │  │ lezer_faucet │  │  lezer_   │  │
│  │   (CLI)    │  │  _ui (QML)   │  │  _ui (QML)   │  │testimonial│  │
│  └─────┬──────┘  └──────┬───────┘  └──────┬───────┘  └─────┬─────┘  │
│        │                └─────────────────┼────────────────┘        │
│        │                                  │ LIDL / IPC              │
│        │         ┌────────────────────────┴────────────────┐        │
│        │         │  lezer_wallet_provider / lezer-provider │        │
│        │         │  (Host-authenticated Caller + Jobs)     │        │
│        └─────────┼────────────────────────┬────────────────┘        │
└──────────────────┼─────────────────────────┼────────────────────────┘
                   │                         │
┌──────────────────┴─────────────────────────┴────────────────────────┐
│                    NATIVE RUST CORE (`lezer-core`)                  │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │  SealedVaultEnvelopeV1 (Argon2id + XChaCha20-Poly1305)        │  │
│  │  ApprovalBroker │ TxIntent & Summarize │ AutoLockFsm │ Cache  │  │
│  │  ErrorCode (LZR-1xxx..6xxx) │ Redacted AuditLogger            │  │
│  └───────────────────────────────┬───────────────────────────────┘  │
│                                  │                                  │
│  ┌───────────────────────────────┴───────────────────────────────┐  │
│  │                   `lezer-adapter`                             │  │
│  │  Official `wallet::WalletCore` (v0.3.1) │ Public & Private Tx │  │
│  │  ProgramLoader │ Token Client │ Faucet Client │ Registry      │  │
│  │  AllowedEndpointsGuard (sole importer of upstream LEZ crates) │  │
│  └───────────────────────────────┬───────────────────────────────┘  │
└──────────────────────────────────┼──────────────────────────────────┘
                                   │ JSON-RPC
┌──────────────────────────────────┼──────────────────────────────────┐
│                 LOGOS EXECUTION ZONE (LEZ v0.3.1)                   │
│       https://testnet.lez.logos.co  or  http://127.0.0.1:3040       │
│  native_token │ privacy_preserving_circuit │ program_loader         │
│  lezer_testimonial │ lezer_token │ lezer_faucet │ lezer_registry    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. Component Responsibilities

### 3.1 `crates/lezer-adapter` — Exclusive Protocol Boundary

The **only** crate permitted to import upstream LEZ crates (`wallet`, `lee`, `lee_core`, `sequencer_service_rpc`, `key_protocol`). Enforced in CI by `tooling/check-boundaries.sh`.

- `engine.rs`: Wraps upstream `wallet::WalletCore` (`v0.3.1`) for deterministic key trees, public (`Public/<base58>`) and private (`Private/<base58>`) accounts, `lezer:priv1…` receive addresses, private block syncing, and all 4 native transfer modes (`Public`, `Shield`, `Deshield`, `Private` with real RISC Zero ZK proving).
- `programs.rs`: Deploys RISC0 ELF binaries via the official `ProgramLoader` facade, submits LP-0021 testimonials (`lezer_testimonial`), and provides the wallet-less `verify_adoption` chain reader.
- `token.rs`: Host client for `lezer_token` (`token_define`, `token_mint`, `token_transfer`, `token_burn`, `token_set_authority`, `token_info_rpc`, `token_balance_rpc`) with local shard pre-checks (`send_pub_tx_with_pre_check`).
- `faucet.rs`: Host client for `lezer_faucet` (`faucet_vault_id`, `faucet_status_rpc`, `faucet_claim`) with local pre-checks for per-account claim tickets and treasury balance.
- `registry.rs`: Host client for `lezer_registry` (`register_program_self` via chained calls, wallet-less `list_registry`, and O(1) `lookup_registry_entry` for live approval badges).
- `profile.rs` & `rpc.rs`: `NetworkProfile` (`lez-testnet`, `lez-localnet`, `lez-custom`) + `AllowedEndpointsGuard` egress allowlist.

### 3.2 `crates/lezer-core` — Wallet State & Approval Engine

Owns all wallet state and security invariants with zero direct upstream imports:

- `vault.rs`: `SealedVaultEnvelopeV1` (Argon2id `m=64 MiB, t=3, p=4` → XChaCha20-Poly1305 → SHA-256 header MAC) wrapping the upstream wallet storage + Lezer sessions, activity journal, and tracked tokens. Atomic write-temp-fsync-rename with `0600` permissions.
- `autolock.rs`: 15-minute inactivity timer with `DeferredLockPending` while a ZK proof or program deployment is in flight.
- `broker.rs`: `ApprovalBroker` managing `DappConnectRequest`, `SessionGrant` (`PermissionScope::ReadPrivateBalance` gate), `PendingProposal`, and `take_verified_proposal` (enforcing `LZR-3005` digest binding and `LZR-3004` risk gates).
- `intent.rs` & `summary.rs`: Translates all 7 `TxIntent` variants (`NativeTransfer`, `SubmitTestimonial`, `FaucetClaim`, `TokenTransfer`, `TokenMint`, `TokenBurn`, `TokenSetAuthority`) into `TransactionSummary` with `CriticalRedOverride` warnings on authority changes and a live `lezer_registry` badge: `KnownButUnverified` (program self-registered with a declared source@commit) or `NoRegistryEntry`. `VerifiedSource` is reserved for a reproducible-build attestation, which is not implemented yet, so the wallet never shows it.

### 3.3 `crates/lezer-provider` & `modules/lezer_wallet_provider`

- `lezer-provider`: Thread-safe provider state machine with background `tokio` job execution (`job_id` polling + `approvalRequested`/`requestResolved` events) so long-running ZK proofs never block the Basecamp IPC loop.
- `Caller::is_wallet_ui()` verifies via `logos_rust_sdk::current_caller()` that privileged `wallet_call` operations originate exclusively from `lezer_wallet_ui`, and binds dApp `session_token`s to the originating module name.

### 3.4 On-Chain SPEL Programs (`programs/`)

All four guest programs are built reproducibly with `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build`:

1. **`programs/testimonial` (`lezer_testimonial`)**: Stores one `Testimonial` per signing author shard and indexes authors across 16 deterministic bucket PDAs (`page_seed(bucket, page)`). Supports `RegisterSelf`.
2. **`programs/token` (`lezer_token`)**: Fungible token program using the LEZ v0.3 Plan/Apply model: `TokenDefinition` on the definition account, per-`(definition, owner)` `Holding` PDAs, supply conservation in `u128`, mint authority rotation/renouncement, and `RegisterSelf`.
3. **`programs/faucet` (`lezer_faucet`)**: Holds native LEZ in a deterministic treasury PDA (`VAULT_SEED`) and dispenses 50 LEZ via `lee_core::native_token::custody_transfer` (`with_pda_seeds`) while writing a per-recipient `ClaimRecord` PDA to enforce rate-limiting on chain. Supports `RegisterSelf`.
4. **`programs/registry` (`lezer_registry`)**: LP-0023 on-chain program registry. Keyed by `input.caller_account_id` from a chained call (`RegisterSelf`), making it cryptographically impossible for an arbitrary account to forge a metadata listing for another program.

---

## 4. Trust Boundaries & Verification Gates

| Gate | Description | Verification Command |
| :--- | :--- | :--- |
| Boundary Check | Only `lezer-adapter` imports upstream LEZ crates; zero telemetry | `./tooling/check-boundaries.sh` |
| Module Catalog | Validates `logos-repo.json` and all 4 Basecamp module manifests | `./tooling/release-catalog.sh` |
| Unit & Conformance | Vault, broker, intent summaries, provider caller ACLs, guest cores | `cargo test --workspace` |
| Live Sequencer E2E | Real v0.3.1 sequencer: transfers, testimonial, token, registry, faucet | `cargo test -p lezer-cli --test e2e_sequencer` |
| Clean-Room Demo | Full evaluator walkthrough without simulation | `./tooling/demo.sh` |
