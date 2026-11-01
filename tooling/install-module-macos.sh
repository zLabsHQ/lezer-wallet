#!/usr/bin/env bash
# install-module-macos.sh — install a Logos .lgx module into the locally
# installed LogosBasecamp app (macOS, darwin-arm64).
#
# Usage:
#   ./tooling/install-module-macos.sh path/to/module-1.0.0.lgx [more.lgx ...]
#   ./tooling/install-module-macos.sh --release <tag> [<tag> ...]
#
# --release downloads the .lgx assets of the given GitHub release tags from
# origin's repo via `gh`, then installs them.
#
# Layout produced (what Basecamp scans at startup):
#   core   -> "$BC/modules/<name>/"    { manifest.json, metadata.json?, variant,
#                                       .lgpm-source, assets/, <name>_plugin.dylib, lib*.dylib }
#   ui_qml -> "$BC/plugins/<name>/"    { manifest.json, metadata.json, variant,
#                                       .lgpm-source, assets/, qml/, src/icons/ }
set -euo pipefail

BC="${BASECAMP_DATA:-$HOME/Library/Application Support/Logos/LogosBasecamp}"
REPO="$(cd "$(dirname "$0")/.." && git remote get-url origin 2>/dev/null | sed -E 's#.*github.com[:/]##; s#\.git$##')"
VARIANT="darwin-arm64"

die() { echo "error: $*" >&2; exit 1; }

install_lgx() {
  local lgx="$1" src_url="${2:-local}"
  [ -f "$lgx" ] || die "no such file: $lgx"
  local tmp name type dest
  tmp="$(mktemp -d)"
  tar -xzf "$lgx" -C "$tmp"
  name="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["name"])' "$tmp/manifest.json")"
  type="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("type","core"))' "$tmp/manifest.json")"
  [ -d "$tmp/variants/$VARIANT" ] || die "$name: no $VARIANT variant in $lgx"
  case "$type" in
    ui_qml) dest="$BC/plugins/$name" ;;
    *)      dest="$BC/modules/$name" ;;
  esac
  mkdir -p "$dest"
  cp "$tmp/manifest.json" "$dest/manifest.json"
  [ -d "$tmp/assets" ] && cp -R "$tmp/assets" "$dest/"
  cp -R "$tmp/variants/$VARIANT/." "$dest/"
  printf '%s' "$VARIANT" > "$dest/variant"
  printf '%s' "$src_url" > "$dest/.lgpm-source"
  echo "installed $name ($type) -> $dest"
  rm -rf "$tmp"
}

[ $# -ge 1 ] || die "usage: $0 [--release <tag> ...] <module.lgx> ..."

if [ "$1" = "--release" ]; then
  shift
  [ $# -ge 1 ] || die "--release needs at least one tag"
  dl="$(mktemp -d)"
  for tag in "$@"; do
    gh -R "$REPO" release download "$tag" -p "*.lgx" --dir "$dl" --clobber
  done
  set -- "$dl"/*.lgx
fi

for lgx in "$@"; do
  url="local"
  case "$(basename "$lgx")" in
    *.lgx) url="manual-install" ;;
  esac
  install_lgx "$lgx" "$url"
done

echo
echo "Done. Restart LogosBasecamp to pick up the modules:"
echo "  osascript -e 'quit app \"LogosBasecamp\"' 2>/dev/null; open -a LogosBasecamp"
