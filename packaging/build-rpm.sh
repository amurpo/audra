#!/bin/bash
set -e

SPEC="$(dirname "$0")/audra.spec"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER=$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')

# Load credentials so build.rs embeds them in the binary
if [ -f "$ROOT/.env" ]; then
    set -a
    # shellcheck source=/dev/null
    source "$ROOT/.env"
    set +a
else
    echo "WARNING: .env not found — binary will be built without Last.fm credentials"
fi

echo "==> Building audra v$VER..."
cargo build --release --manifest-path "$ROOT/Cargo.toml"

RPMBUILD="$ROOT/target/rpmbuild"

echo "==> Preparing sources for rpmbuild..."
mkdir -p "$RPMBUILD/SOURCES"
cp "$ROOT/target/release/audra"                                      "$RPMBUILD/SOURCES/"
cp "$ROOT/data/com.audra.player.desktop"                             "$RPMBUILD/SOURCES/"
cp "$ROOT/data/com.audra.player.metainfo.xml"                        "$RPMBUILD/SOURCES/"
cp "$ROOT/data/icons/hicolor/scalable/apps/com.audra.player.svg"    "$RPMBUILD/SOURCES/"
cp "$ROOT/po/es.po"                                                  "$RPMBUILD/SOURCES/"

echo "==> Building RPM..."
rpmbuild -bb --define "_topdir $RPMBUILD" --define "ver $VER" "$SPEC"

echo "==> Done: $RPMBUILD/RPMS/x86_64/audra-$VER-1.*.rpm"
