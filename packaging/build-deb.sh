#!/bin/bash
set -e

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

PKG="audra_${VER}_amd64"
STAGE="$ROOT/target/debpkg/$PKG"

echo "==> Staging package tree..."
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"

install -Dm755 "$ROOT/target/release/audra"                                   "$STAGE/usr/bin/audra"
install -Dm644 "$ROOT/data/com.audra.player.desktop"                          "$STAGE/usr/share/applications/com.audra.player.desktop"
install -Dm644 "$ROOT/data/icons/hicolor/scalable/apps/com.audra.player.svg"  "$STAGE/usr/share/icons/hicolor/scalable/apps/com.audra.player.svg"
mkdir -p "$STAGE/usr/share/locale/es/LC_MESSAGES"
msgfmt "$ROOT/po/es.po" -o "$STAGE/usr/share/locale/es/LC_MESSAGES/audra.mo"

# Depends are hardcoded to mirror the RPM spec's `Requires` (gtk4, libadwaita;
# sqlite is bundled via rusqlite) plus ALSA, which rodio links at runtime.
# `libasound2t64` is the package name on Ubuntu 24.04 (the CI runner).
cat > "$STAGE/DEBIAN/control" <<EOF
Package: audra
Version: $VER
Architecture: amd64
Maintainer: Daniel Avila <daigo.tnt@gmail.com>
Section: sound
Priority: optional
Depends: libgtk-4-1, libadwaita-1-0, libasound2t64
Description: Native music player for Linux with Last.fm scrobbling
 Audra is a native music player for Linux (GTK4/libadwaita)
 with Last.fm integration and automatic scrobbling.
EOF

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database >/dev/null 2>&1 || true
EOF

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database >/dev/null 2>&1 || true
EOF

chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"

echo "==> Building DEB..."
dpkg-deb --root-owner-group --build "$STAGE" "$ROOT/target/debpkg/$PKG.deb"

echo "==> Done: $ROOT/target/debpkg/$PKG.deb"
