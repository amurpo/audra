#!/bin/bash
# Launch a locally built audra binary with Homebrew GTK libraries on the loader path.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/audra"

if [ ! -x "$BIN" ]; then
  echo "Binary not found. Build first: bash packaging/build-macos.sh" >&2
  exit 1
fi

if [ -d /opt/homebrew/lib ]; then
  BREW_LIB="/opt/homebrew/lib"
elif [ -d /usr/local/lib ]; then
  BREW_LIB="/usr/local/lib"
else
  echo "ERROR: Could not find Homebrew lib directory." >&2
  exit 1
fi

export DYLD_FALLBACK_LIBRARY_PATH="${BREW_LIB}${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
exec "$BIN" "$@"
