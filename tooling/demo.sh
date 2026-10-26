#!/usr/bin/env bash
# tooling/demo.sh — real end-to-end demo against a standalone LEZ v0.3.1 sequencer.
# No simulation flags, no local JSON ledgers: every step below lands on chain.
#
# Requirements: cargo + rust toolchain; Docker (only to build guest binaries when missing);
# a checked-out upstream v0.3.1 sequencer OR an already-running one at $LEZER_SEQUENCER.
#
#   SEQUENCER_HOME  — dir to run a fresh sequencer in (default: mktemp)
#   LEZER_SEQUENCER — URL of an existing sequencer to reuse instead of starting one
#   SKIP_GUEST_BUILD=1 — reuse prebuilt programs/*/target/.../docker/*.bin
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
LEZER_BIN="${LEZER_BIN:-$REPO_ROOT/target/debug/lezer}"
SEQ_URL="${LEZER_SEQUENCER:-http://127.0.0.1:3040}"
UPSTREAM="${UPSTREAM:-$REPO_ROOT/third_party/lez-v031}"

step() { printf '\n\033[1m── %s ──\033[0m\n' "$1"; }

# ── sequencer ────────────────────────────────────────────────────────────────
if curl -sf "$SEQ_URL" -X POST -H 'content-type: application/json' \
      -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' >/dev/null 2>&1; then
  echo "Reusing sequencer at $SEQ_URL"
else
  SEQ_HOME="${SEQUENCER_HOME:-$(mktemp -d)}"
  SEQUENCER_BIN="$UPSTREAM/target/release/sequencer_service"
  SEQ_CONFIG="$UPSTREAM/lez/sequencer/service/configs/debug/sequencer_config.json"
  [ -f "$SEQ_CONFIG" ] || { echo "missing upstream config $SEQ_CONFIG — clone v0.3.1 into $UPSTREAM" >&2; exit 1; }
  [ -x "$SEQUENCER_BIN" ] || { echo "building sequencer_service (once)…"; cargo build --release --manifest-path "$UPSTREAM/Cargo.toml" -p sequencer_service; }
  echo "Starting standalone sequencer (home $SEQ_HOME) on $SEQ_URL"
  ("$SEQUENCER_BIN" "$SEQ_CONFIG" --home "$SEQ_HOME" --listen-address 127.0.0.1       --port "${SEQ_URL##*:}" >"$SEQ_HOME/sequencer.log" 2>&1 &)
  for i in $(seq 1 60); do
    curl -sf "$SEQ_URL" -X POST -H 'content-type: application/json' \
      -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' >/dev/null 2>&1 && break
    sleep 1
  done
fi
echo "Sequencer: $SEQ_URL"

# ── wallet + funded account (upstream debug genesis key, localnet only) ───────
DEMO_DIR="$(mktemp -d)"
export LEZER_HOME="$DEMO_DIR" LEZER_PASSWORD="demo-evaluator-password"
GENESIS_KEY_HEX="10a26a9aec7d34b82364eeae45c5294dbb0a764b000b94eeb9b58511dc487c4d"

step "init wallet on $SEQ_URL"
cargo build -p lezer-cli >/dev/null
"$LEZER_BIN" init --network "$SEQ_URL" >/dev/null
echo "$GENESIS_KEY_HEX" | "$LEZER_BIN" account import --label genesis >/dev/null
"$LEZER_BIN" account new public --label alice >/dev/null
"$LEZER_BIN" balance genesis

# ── real public transfer ─────────────────────────────────────────────────────
step "public transfer genesis -> alice (5 LEZ)"
"$LEZER_BIN" send --from genesis --to alice --amount 5000000000
"$LEZER_BIN" balance alice

# ── guest programs ───────────────────────────────────────────────────────────
build_guest() { # name
  local name="$1" bin="programs/$1/target/riscv32im-risc0-zkvm-elf/docker/lezer_$1.bin"
  if [ ! -f "$bin" ] || [ "${FORCE_GUEST_BUILD:-0}" = "1" ]; then
    command -v docker >/dev/null || { echo "Docker required to build guest $bin" >&2; exit 1; }
    RISC0_DOCKER_CONTAINER_TAG=r0.1.91.1 cargo risczero build \
      --manifest-path "programs/$1/Cargo.toml" -p "lezer_$1" >/dev/null
  fi
  echo "$bin"
}

step "deploy lezer_testimonial"
T_BIN="$(build_guest testimonial)"
"$LEZER_BIN" program deploy "$T_BIN" --payer genesis

step "publish + verify testimonial (wallet-less verifier)"
"$LEZER_BIN" testimonial submit --author genesis --username "@demo" \
  --text "lezer demo.sh real run, no simulation"
"$LEZER_BIN" verify-adoption

step "deploy lezer_token + full lifecycle"
K_BIN="$(build_guest token)"
"$LEZER_BIN" program deploy "$K_BIN" --payer genesis
"$LEZER_BIN" account new public --label demo-def >/dev/null
"$LEZER_BIN" token define --definition demo-def --name "Demo Token" --symbol DEMO \
  --decimals 6 --supply 1000000 --authority genesis --payer genesis
# the definition account's id IS the token id
TOKEN_ID="$("$LEZER_BIN" --json account list 2>/dev/null \
  | python3 -c 'import json,sys; [print(a["account_id"]) for a in json.load(sys.stdin) if "demo-def" in a.get("labels",[])]')"
"$LEZER_BIN" token mint --token "$TOKEN_ID" --authority genesis --to alice --amount 5000 --payer genesis
"$LEZER_BIN" token transfer --token "$TOKEN_ID" --from alice --to genesis --amount 1500 --payer genesis
"$LEZER_BIN" token burn --token "$TOKEN_ID" --from alice --amount 500 --payer genesis
"$LEZER_BIN" token balance --token "$TOKEN_ID" --owner alice
"$LEZER_BIN" token info --token "$TOKEN_ID"

step "negative paths (local rejection, no charged revert)"
"$LEZER_BIN" token mint --token "$TOKEN_ID" --authority alice --to alice --amount 1 --payer genesis \
  && { echo "FAIL: unauthorized mint accepted"; exit 1; } || echo "  unauthorized mint rejected ✓"
"$LEZER_BIN" token transfer --token "$TOKEN_ID" --from alice --to genesis --amount 999999999 --payer genesis \
  && { echo "FAIL: overspend accepted"; exit 1; } || echo "  overspend rejected ✓"

step "DONE — every command above changed or read real chain state"
