#!/bin/bash
# Verify release build and tests on macOS (intended for Apple Silicon; works on Intel too).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: This script is for macOS only (got $(uname -s))." >&2
  exit 1
fi

for cmd in cargo pkg-config msgfmt; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Missing required command: $cmd" >&2
    echo "Install Rust (rustup), Homebrew gtk4/libadwaita/gettext, and ensure msgfmt is on PATH." >&2
    exit 1
  fi
done

echo "==> pkg-config probe (graphene-gobject-1.0)..."
if [ -d /opt/homebrew/lib/pkgconfig ]; then
  export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"
elif [ -d /usr/local/lib/pkgconfig ]; then
  export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:/usr/local/share/pkgconfig"
fi
pkg-config --exists 'graphene-gobject-1.0 >= 1.10' || {
  echo "ERROR: graphene-gobject-1.0 not found. Run: brew install gtk4 libadwaita" >&2
  exit 1
}

echo "==> Release build..."
bash "$ROOT/packaging/build-macos.sh"

BIN="$ROOT/target/release/audra"
echo "==> Architecture check..."
file "$BIN" | grep -q 'executable arm64' || {
  echo "ERROR: Expected arm64 binary; got:" >&2
  file "$BIN" >&2
  exit 1
}

echo "==> cargo test..."
export PKG_CONFIG_PATH
cargo test --manifest-path "$ROOT/Cargo.toml" --quiet

echo "==> All checks passed."
