#!/bin/bash
set -e

SPEC="$(dirname "$0")/audra.spec"
ROOT="$(dirname "$0")/.."
VER=$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')

# Cargar credenciales para que build.rs las embeba en el binario
if [ -f "$ROOT/.env" ]; then
    set -a
    # shellcheck source=/dev/null
    source "$ROOT/.env"
    set +a
else
    echo "ADVERTENCIA: no se encontró .env — el binario quedará sin credenciales de Last.fm"
fi

echo "==> Compilando audra v$VER..."
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "==> Preparando fuentes para rpmbuild..."
mkdir -p ~/rpmbuild/SOURCES
cp "$ROOT/target/release/audra"                                      ~/rpmbuild/SOURCES/
cp "$ROOT/data/com.audra.player.desktop"                             ~/rpmbuild/SOURCES/
cp "$ROOT/data/icons/hicolor/scalable/apps/com.audra.player.svg"    ~/rpmbuild/SOURCES/

echo "==> Construyendo RPM..."
rpmbuild -bb --define "ver $VER" "$SPEC"

echo "==> Listo: ~/rpmbuild/RPMS/x86_64/audra-$VER-1.*.rpm"
