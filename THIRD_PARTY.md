# Third-Party OSS & License Compliance Inventory

Every upstream repository and library consumed by **Lezer Wallet** (`lezer-wallet`) is inventoried below with its exact source, license, and role. All direct and transitive dependencies carry permissive or LGPL-compatible licenses suitable for open-source distribution under **MIT OR Apache-2.0**.

## 1. Direct Upstream Logos & Protocol Dependencies

| Repository | Pin / Version | License | Role | Consumed By |
| :--- | :--- | :--- | :--- | :--- |
| [`logos-blockchain/logos-execution-zone`](https://github.com/logos-blockchain/logos-execution-zone) | `v0.2.4` / `v0.3` / `v0.4` | MIT | LEZ wallet engine (`wallet`), account/program types (`lee`, `lee_core`), `sequencer_service_rpc`, `system_accounts`, `programs` | `crates/lezer-adapter` (exclusively) |
| [`logos-co/logos-rust-sdk`](https://github.com/logos-co/logos-rust-sdk) | `bcc36420d7a15fb39cbf8079c85a18650cdae968` (v0.3.0) | MIT OR Apache-2.0 | Logos Core module SDK (`logos_rust_sdk`) and `logos-lidl-gen` C-ABI codegen | `modules/lezer_wallet_provider` |
| [`logos-co/logos-module-builder`](https://github.com/logos-co/logos-module-builder) | Flake input | MIT OR Apache-2.0 | Nix module builder (`mkLogosModule`, `mkLogosQmlModule`) | `flake.nix`, `modules/*/flake.nix` |
| [`logos-co/nix-bundle-lgx`](https://github.com/logos-co/nix-bundle-lgx) | Flake input | MIT OR Apache-2.0 | `.lgx` module archive packager for `lgpd` | `modules/*/flake.nix` |
| [`logos-co/spel`](https://github.com/logos-co/spel) | `6c0598a409ab801df843d99a46c04a13105df663` | MIT OR Apache-2.0 | SPEL RISC Zero guest framework (`#[lez_program]`, `#[instruction]`) | `programs/testimonial`, `programs/registry` |

## 2. Cryptographic & Proving Stack Dependencies

| Component / Crate | Source | License | Role |
| :--- | :--- | :--- | :--- |
| `logos-blockchain-circuits` | [`logos-blockchain/logos-blockchain-circuits`](https://github.com/logos-blockchain/logos-blockchain-circuits) (`v0.5.3`) | MIT | Prebuilt witness-generator circuit archives (`LBC_ROOT_DIR`) |
| `rapidsnark` | [`iden3/rapidsnark`](https://github.com/iden3/rapidsnark) (`v0.0.8`) | LGPL-3.0 | Groth16 prover (`RAPIDSNARK_LIB_DIR`), reproducible source-available Nix build |
| `risc0-zkvm` / `risc0-binfmt` | [`risc0/risc0`](https://github.com/risc0/risc0) (`3.0.6`) | Apache-2.0 | RISC Zero zkVM guest execution & receipt proving |
| `argon2` | crates.io | MIT OR Apache-2.0 | `Argon2id` password key derivation for `SealedVaultEnvelopeV1` (`vault.enc`) |
| `chacha20poly1305` | crates.io | MIT OR Apache-2.0 | `XChaCha20-Poly1305` AEAD vault encryption |
| `bip39` | crates.io | MIT OR Apache-2.0 | 24-word BIP-39 mnemonic generation and seed derivation |
| `sha2` | crates.io | MIT OR Apache-2.0 | SHA-256 PDA seed hashing, ATA derivation, and `TransactionSummary` digests |
| `zeroize` | crates.io | MIT OR Apache-2.0 | Deterministic memory wiping on vault lock/drop |

## 3. Runtime & System Dependencies

| Component | Source | License | Role |
| :--- | :--- | :--- | :--- |
| Qt 6 (`qtdeclarative`, `qtwebview`) | Qt Project (via Nixpkgs) | LGPL-3.0 | Dynamically linked runtime for Basecamp `ui_qml` modules (`lezer_wallet_ui`, `lezer_faucet_ui`, `lezer_testimonial_ui`) |
| `libpcsclite` | PC/SC Lite | BSD-3-Clause | Optional hardware keycard smartcard interface |
| `tokio`, `reqwest`, `serde`, `borsh`, `clap`, `tracing` | crates.io | MIT OR Apache-2.0 | Async runtime, allowlisted JSON-RPC client, serialization, CLI, and redacted logging |

## 4. License Compliance Summary

- All first-party code in this repository is dual-licensed under **MIT OR Apache-2.0** (`LICENSE-MIT` and `LICENSE-APACHE`).
- **Import Boundary Enforcement:** Only `crates/lezer-adapter` imports upstream `wallet` and `lee_core` crates (`./tooling/check-boundaries.sh`).
- **Zero Telemetry:** No third-party analytics, crash reporters, or external telemetry SDKs are included anywhere in the dependency tree.
