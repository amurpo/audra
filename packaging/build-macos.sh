#!/bin/bash
# Build a release audra binary on macOS (Apple Silicon or Intel Homebrew layouts).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${VER:-$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')}"

# Homebrew: Apple Silicon vs Intel default prefix
if [ -d /opt/homebrew/lib/pkgconfig ]; then
  BREW_PKG="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"
  BREW_LIB="/opt/homebrew/lib"
elif [ -d /usr/local/lib/pkgconfig ]; then
  BREW_PKG="/usr/local/lib/pkgconfig:/usr/local/share/pkgconfig"
  BREW_LIB="/usr/local/lib"
else
  echo "ERROR: Homebrew pkg-config paths not found. Install gtk4 and libadwaita with brew."
  exit 1
fi

export PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-$BREW_PKG}"

# msgfmt is often keg-only on macOS
if [ -d /opt/homebrew/opt/gettext/bin ]; then
  export PATH="/opt/homebrew/opt/gettext/bin:$PATH"
elif [ -d /usr/local/opt/gettext/bin ]; then
  export PATH="/usr/local/opt/gettext/bin:$PATH"
fi

if [ -z "${LASTFM_PROXY_URL:-}" ] && [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck source=/dev/null
  source "$ROOT/.env"
  set +a
fi
if [ -z "${LASTFM_PROXY_URL:-}" ]; then
  echo "WARNING: LASTFM_PROXY_URL is not set — binary will be built without Last.fm scrobbling"
fi

echo "==> Building audra v$VER for $(uname -m)..."
cargo build --release --manifest-path "$ROOT/Cargo.toml"

BIN="$ROOT/target/release/audra"
echo "==> Done: $BIN"
file "$BIN"
echo ""
echo "Run with: DYLD_FALLBACK_LIBRARY_PATH=\"$BREW_LIB\" $BIN"
echo "Or:       bash packaging/run-macos.sh"
