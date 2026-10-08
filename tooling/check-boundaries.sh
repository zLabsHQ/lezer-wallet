#!/usr/bin/env bash
# tooling/check-boundaries.sh — Enforces Lezer Wallet Monorepo Import Boundaries & Zero-Telemetry Rules
#
# Architectural Rules Verified:
#   1. `crates/lezer-adapter` is the ONLY crate allowed to depend on or import
#      upstream `wallet`, `lee`, `lee_core`, `sequencer_service_rpc`, `system_accounts`, or `programs`.
#   2. `crates/lezer-core` must NEVER import `wallet` or `lee_core` directly.
#   3. `modules/` and `apps/` and `packages/` must NEVER import internal vault structs
#      (`SealedVaultEnvelopeV1`, `CanonicalWalletStateV1`, `Argon2ParamsRecord`, `upstream_storage_blob`).
#   4. Zero-telemetry & zero third-party analytics SDKs across the entire repository.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

FAILURES=0

pass() {
  printf "  [PASS] %s\n" "$1"
}

fail() {
  printf "  [FAIL] %s\n" "$1" >&2
  FAILURES=$((FAILURES + 1))
}

echo "================================================================================"
echo " Lezer Wallet — Architectural Boundary & Security Guardrail Check"
echo "================================================================================"

# 1. Verify required Phase 0 monorepo skeleton directories & files exist
for required in \
  "Cargo.toml" \
  "package.json" \
  "tsconfig.base.json" \
  "flake.nix" \
  "LICENSE-MIT" \
  "LICENSE-APACHE" \
  "THIRD_PARTY.md" \
  "logos-repo.json" \
  "crates/lezer-adapter/Cargo.toml" \
  "crates/lezer-adapter/build.rs" \
  "crates/lezer-adapter/src/profile.rs" \
  "crates/lezer-adapter/src/rpc.rs" \
  "crates/lezer-core/Cargo.toml" \
  "crates/lezer-cli/Cargo.toml" \
  "modules/lezer_wallet_provider/metadata.json" \
  "modules/lezer_wallet_ui/metadata.json"
do
  if [[ ! -f "$required" ]]; then
    fail "Missing required workspace file: $required"
  fi
done
pass "All required root workspace, crate, and module manifest files exist"

# 2. Boundary Rule 1: `crates/lezer-core/Cargo.toml` and `crates/lezer-cli/Cargo.toml`
#    and `modules/lezer_wallet_provider/Cargo.toml` must NOT depend on upstream LEZ crates.
FORBIDDEN_UPSTREAM_DEPS='^(wallet|lee|lee_core|sequencer_service_rpc|system_accounts|programs)\s*[=.]'
for manifest in \
  "crates/lezer-core/Cargo.toml" \
  "crates/lezer-cli/Cargo.toml" \
  "modules/lezer_wallet_provider/Cargo.toml"
do
  if grep -Eq "$FORBIDDEN_UPSTREAM_DEPS" "$manifest"; then
    fail "$manifest directly depends on upstream LEZ crates (only crates/lezer-adapter is permitted)"
  fi
done
pass "Cargo.toml manifests outside crates/lezer-adapter do not depend on upstream LEZ crates"

# 3. Boundary Rule 2: `crates/lezer-core/src` must NEVER import `wallet::` or `lee_core::` or `lee::`
if grep -rnE '(use\s+(wallet|lee|lee_core|sequencer_service_rpc|system_accounts)(::|;)|extern\s+crate\s+(wallet|lee|lee_core))' crates/lezer-core/src/ >/dev/null 2>&1; then
  fail "crates/lezer-core/src imports upstream wallet/lee/lee_core directly"
else
  pass "crates/lezer-core/src is 100% isolated from upstream wallet/lee_core imports"
fi

# 4. Boundary Rule 3: `modules/`, `packages/`, and `apps/` must NEVER import internal vault structs
FORBIDDEN_VAULT_INTERNALS='(SealedVaultEnvelopeV1|CanonicalWalletStateV1|Argon2ParamsRecord|upstream_storage_blob)'
for surface_dir in modules packages apps; do
  if [[ -d "$surface_dir" ]]; then
    if grep -rnE "$FORBIDDEN_VAULT_INTERNALS" "$surface_dir" >/dev/null 2>&1; then
      fail "$surface_dir/ references internal vault structures ($FORBIDDEN_VAULT_INTERNALS)"
    fi
  fi
done
pass "modules/, packages/, and apps/ do not import internal vault structs"

# 5. Boundary Rule 4: Zero-Telemetry & Third-Party Service Guard
FORBIDDEN_TELEMETRY='(sentry\.io|posthog|mixpanel|google-analytics|googletagmanager|segment\.io|amplitude\.com)'
if grep -rnEi "$FORBIDDEN_TELEMETRY" crates/ modules/ packages/ apps/ 2>/dev/null | grep -v "check-boundaries" >/dev/null 2>&1; then
  fail "Detected forbidden telemetry/analytics endpoint in source tree"
else
  pass "Zero-telemetry check passed (no analytics or third-party tracking endpoints)"
fi

if [[ "$FAILURES" -gt 0 ]]; then
  echo "--------------------------------------------------------------------------------"
  echo " Boundary check FAILED with $FAILURES violation(s)." >&2
  exit 1
fi

echo "--------------------------------------------------------------------------------"
echo " All architectural boundary and security guardrails PASSED."
