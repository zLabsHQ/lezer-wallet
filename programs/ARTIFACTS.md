# Lezer Program Artifacts

Every entry here is reproducible: rebuild the guest binary with Docker and compare the image id.

## lezer_testimonial

LP-0021 testimonial program (`programs/testimonial`).

| Field | Value |
| :--- | :--- |
| Source | `programs/testimonial/{core,guest}` |
| Guest binary | `lezer_testimonial.bin` (387,528 bytes) |
| Image ID | `912c3b34a8e58def7000f2c64bd7e6fd0eed9854ee922c220f9587828806e74b` (v2, adds `RegisterSelf`) |
| Build | `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/testimonial/Cargo.toml -p lezer_testimonial` |

Deployments (program account ids — verify with `lezer program image-id` and `getProgramIds`/`getAccountView`):

| Network | Program account id | Deployed |
| :--- | :--- | :--- |
| localnet (standalone v0.3.1 debug genesis) | `Public/9cn1zB8UxQLDYpEbkdCEVJvZdvASZUmNhDXxUuXMH9Y7` | v2; also registered itself in `lezer_registry` |
| localnet (v1, no RegisterSelf) | `Public/85YmBk7v6keCoRVMiGpCv8M5LF83KMxx799p98gNLiMh` | superseded |
| testnet (`https://testnet.lez.logos.co`) | — | not yet deployed (needs funded account) |

Rebuild + deploy yourself:

```bash
RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build \
  --manifest-path programs/testimonial/Cargo.toml -p lezer_testimonial
lezer program image-id programs/testimonial/target/riscv32im-risc0-zkvm-elf/docker/lezer_testimonial.bin
lezer program deploy programs/testimonial/target/.../lezer_testimonial.bin --payer <funded>
```

## lezer_registry (LP-0023)

On-chain program directory (`programs/registry`). Registration is a **chained call**: a
program's own plan emits `ChainedCall` to the registry, which records
`caller_account_id` as the key — a signer cannot forge a listing for someone else's program.
Entries live in registry-shard PDAs (`entry_seed`); enumeration walks 16 deterministic index
buckets by exact-account reads.

| Field | Value |
| :--- | :--- |
| Source | `programs/registry/{core,guest}` |
| Guest binary | `lezer_registry.bin` (387,072 bytes) |
| Image ID | `66ac37a3840cfff5f69fead3f3404ddc9707cd7e807d21c76e328c4cdc5ffb2c` |
| Build | `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/registry/Cargo.toml -p lezer_registry` |

| Network | Program account id | Deployed |
| :--- | :--- | :--- |
| localnet | `Public/GgDbNLcWFWQ8VTZnVzVP9FnX7vKBFA8SrsKSh3LkxB5j` | holds self-registrations by `lezer_testimonial` + `lezer_token` (`lezer registry list`) |

Our registerable programs accept a `RegisterSelf(RegisterSelfParams)` instruction that emits
the chained call; `lezer registry register --subject <name>` builds it.

The old `experimental/registry-program` scaffolding (pre-0.3 model) stays as reference only.

## lezer_token

Fungible token program (`programs/token`): definition shard on a creator-chosen account,
per-(token, owner) holding PDAs, plan-side PDA derivation, apply-side supply/authority checks.
Define/mint/transfer/burn/set-authority are real program calls; balances are exact-account
reads — there is no local token ledger.

| Field | Value |
| :--- | :--- |
| Source | `programs/token/{core,guest}` |
| Guest binary | `lezer_token.bin` (420,324 bytes) |
| Image ID | `581afed957bfc437e8bfb2cd268bdc5428fefd6a6c41a4d88b078c7fd1bbf066` (v2, adds `RegisterSelf`) |
| Build | `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/token/Cargo.toml -p lezer_token` |

| Network | Program account id | Deployed |
| :--- | :--- | :--- |
| localnet | `Public/Boo4RvaEDy4fG6UpB8a2bd4GaEWp8Gvr1YLyKqzDij8r` | full lifecycle verified; self-registered in `lezer_registry` |
| localnet (v1) | `Public/8vrFT3VwuDFLBoiLYt2tdRh7dKdWGxRw4ti6Bk1aVhL7` | superseded |

Note: guests cannot observe signatures — authorization is expressed by which accounts sign the
transaction (`is_authorized`), and a reverted guest apply still lands in a block as a
charged-revert, so the wallet pre-checks shards before signing. "Included" therefore means
"executed" for all `lezer token` commands.

## lezer_faucet (LP-0021)

On-chain native LEZ faucet program (`programs/faucet`). Holds native LEZ in a deterministic
treasury PDA (`VAULT_SEED`) and dispenses `DEFAULT_DRIP_AMOUNT` (50 LEZ = `50_000_000_000`
atomic units) via `lee_core::native_token::custody_transfer` (`with_pda_seeds` delegation).
Enforces 1-per-account rate-limiting via deterministic `claim_seed(recipient)` PDAs on chain,
with local pre-checks before broadcast.

| Field | Value |
| :--- | :--- |
| Source | `programs/faucet/{core,guest}` |
| Guest binary | `lezer_faucet.bin` (392,080 bytes) |
| Image ID | `c22cd2cc627ca79657124c456d61e4fbdd1d7e27e49571b9d86a82fe37a4ea28` |
| Build | `RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build --manifest-path programs/faucet/Cargo.toml -p lezer_faucet` |

| Network | Program account id | Deployed |
| :--- | :--- | :--- |
| localnet | `Public/2uYyyT99Tq6jo1nZvQBBnetXrhvcgMLhmUwFpswXQVUZ` | vault PDA `Public/4URtdg6CDWvUNfP4E6hkqwHFNPELawwt8MTV19NR8ASa`; self-registered in `lezer_registry` |
| testnet (`https://testnet.lez.logos.co`) | — | not yet deployed (needs funded account) |
