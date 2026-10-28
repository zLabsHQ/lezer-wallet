#!/usr/bin/env bash
# tooling/release-catalog.sh — Validates `logos-repo.json` and all Basecamp `.lgx` module manifests

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
import json
import pathlib

root = pathlib.Path(".")
catalog = json.loads((root / "logos-repo.json").read_text())
assert catalog["schemaVersion"] == 1
assert len(catalog["modules"]) == 4

for mod in catalog["modules"]:
    meta_path = root / mod["metadata"]
    assert meta_path.is_file(), f"Missing metadata: {meta_path}"
    meta = json.loads(meta_path.read_text())
    assert meta["name"] == mod["name"]
    assert meta["type"] == mod["type"]

print("  [PASS] logos-repo.json and all 4 Basecamp module manifests validated.")
PY
