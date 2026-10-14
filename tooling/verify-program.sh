#!/usr/bin/env bash
# tooling/verify-program.sh — Reproducible SPEL / RISC Zero Program Bytecode Verifier (LP-0023)
#
# Usage: ./tooling/verify-program.sh <64-hex-program-id>
# Queries the on-chain LP-0023 registry via `lezer registry verify --program <id>`,
# checks the reproducible build manifest / Image ID, and outputs the 4-state verification report.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

PROGRAM_ID="${1:-9aa87b6b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9dfb9f}"

echo "================================================================================"
echo " Lezer LP-0023 Reproducible Program Bytecode Verifier"
echo " Target Program ID: $PROGRAM_ID"
echo "================================================================================"

cargo run --quiet -p lezer-cli -- --json registry verify --program "$PROGRAM_ID"
