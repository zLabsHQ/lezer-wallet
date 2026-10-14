#!/usr/bin/env bash
# tooling/demo.sh — Clean-Environment Evaluator End-to-End Verification Script (LP-0021 & LP-0023)
#
# Executes in one unmodified command:
#   1. Architectural boundary & zero-telemetry lint (`tooling/check-boundaries.sh`)
#   2. Basecamp `.lgx` catalog & manifest validation (`tooling/release-catalog.sh`)
#   3. Full workspace unit, conformance, and release-gate test suite:
#      - Phase 0: Live Testnet RPC & Private Proof Benchmark (`S-01`, `S-02`, `S-03`)
#      - Phase 1: `SealedVaultEnvelopeV1` conformance (`vault_conformance` & `cli_phase1_e2e`)
#      - Phase 2: Gate RC-01 (`lezer_wallet_provider` Connect/Approve) & Gate RC-03
#                 (55-account concurrent sharded testimonial stress test + page rollover `"00" -> "01"`)
#      - Phase 3: Piñata PoW Faucet, Fungible Token (`LEZERUSD`), SHA-256 ATAs & LP-0013 Authority (`token_ata_e2e`)
#      - Phase 4: Gate RC-02 (`live_private_token_transfer`: Native & Token Shield/Deshield/Private +
#                 `Semaphore(1)` 4-stage prover + `DeferredLockPending` + crash recovery + `LZR-2003`)
#      - Phase 5: LP-0023 Program Registry (`register_self` vs. unauthorized deployer rejection),
#                 4-State Verification Badge, and Encrypted Logos Storage CID Backup (`push`/`pull`).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "================================================================================"
echo " Lezer Wallet (lp0021-lezer) — Clean-Room Evaluator E2E Suite"
echo "================================================================================"

echo ""
echo "── [1/6] Enforcing Architectural Boundaries & Zero-Telemetry Guardrails ──"
./tooling/check-boundaries.sh

echo ""
echo "── [2/6] Validating Basecamp Module Catalog (logos-repo.json) ──"
./tooling/release-catalog.sh

echo ""
echo "── [3/6] Running On-Chain SPEL Guest Program Unit Tests (testimonial & registry) ──"
cargo test --manifest-path programs/testimonial/Cargo.toml
cargo test --manifest-path programs/registry/Cargo.toml

echo ""
echo "── [4/6] Running Full Rust Workspace Conformance & Release Gate Suite (RC-01, RC-02, RC-03) ──"
cargo test --workspace

echo ""
echo "── [5/6] Executing End-to-End CLI Lifecycle (Vault -> Faucet -> Token/ATA -> Shield -> LP-0023 -> Backup -> Verify-Adoption) ──"
DEMO_DIR="$(mktemp -d)"
trap 'rm -rf "$DEMO_DIR"' EXIT

export LEZER_KDF_MEMORY_KIB=1024
export LEZER_SIMULATE_BROADCAST=1
export LEZER_PASSWORD="demo-evaluator-password"
export LEZER_TESTIMONIAL_LEDGER="$DEMO_DIR/testimonials.json"
VAULT_PATH="$DEMO_DIR/vault.enc"
LEZER_BIN="./target/debug/lezer"

$LEZER_BIN --json --vault "$VAULT_PATH" init >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" account new --label alice >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" account new --label alice-shielded --private >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" faucet claim --to alice --auto-shield-to alice-shielded >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" token create --name LEZERUSD --supply 1000000 --creator alice --authority alice >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" token send --from alice --to alice-shielded --mint LEZERUSD --amount 500 >/dev/null
$LEZER_BIN --json --vault "$VAULT_PATH" testimonial submit --author alice --username "@evaluator" --text "Verified Lezer Wallet E2E!" >/dev/null

# Prove LP-0023 unauthorized direct deployer registration is rejected
if $LEZER_BIN --json --vault "$VAULT_PATH" registry register \
  --program "3333333333333333333333333333333333333333333333333333333333333333" \
  --kind deployer --name "spoof" >/dev/null 2>&1; then
  echo "  [FAIL] Unauthorized direct deployer registration should have been rejected!" >&2
  exit 1
fi
echo "  [PASS] LP-0023 unauthorized direct deployer registration rejected as expected"

# Verify self-registered lezer_testimonial bytecode badge
$LEZER_BIN --json --vault "$VAULT_PATH" registry verify \
  --program "9aa87b6b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9dfb9f" >/dev/null
echo "  [PASS] LP-0023 VerifiedSource badge confirmed for lezer_testimonial"

# Verify client-side encrypted Logos Storage backup push & pull
CID="$($LEZER_BIN --json --vault "$VAULT_PATH" backup push | python3 -c 'import sys,json; print(json.load(sys.stdin)["cid"])')"
$LEZER_BIN --json --vault "$DEMO_DIR/restored_vault.enc" backup pull --cid "$CID" >/dev/null
echo "  [PASS] Client-side encrypted SealedVaultEnvelopeV1 backup pushed & pulled via CID: $CID"

echo ""
echo "── [6/6] Running Stateless 16-Shard Evaluator Adoption Verifier (LEZER_NO_VAULT=1) ──"
LEZER_NO_VAULT=1 $LEZER_BIN --json --vault "$DEMO_DIR/no_vault.enc" verify-adoption --submission-id lp0021-lezer

echo ""
echo "================================================================================"
echo " ALL LP-0021 & LP-0023 CHECKS PASSED 100% GREEN."
echo "================================================================================"
