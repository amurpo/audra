#!/bin/bash
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${VER:-$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')}"
# DEB requiere que la versión empiece con dígito; pre-releases llevan prefijo 0~
DEB_VER=$(echo "$VER" | tr '-' '~')
case "$DEB_VER" in [0-9]*) ;; *) DEB_VER="0~${DEB_VER}" ;; esac

# En CI, LASTFM_PROXY_URL viene del entorno del workflow.
# En local, se carga desde .env solo si aún no está definida.
if [ -z "$LASTFM_PROXY_URL" ] && [ -f "$ROOT/.env" ]; then
    set -a
    # shellcheck source=/dev/null
    source "$ROOT/.env"
    set +a
fi
if [ -z "$LASTFM_PROXY_URL" ]; then
    echo "WARNING: LASTFM_PROXY_URL no está configurado — el binario no tendrá scrobbling de Last.fm"
fi

echo "==> Building audra v$VER..."
cargo build --release --manifest-path "$ROOT/Cargo.toml"

PKG="audra_${DEB_VER}_amd64"
STAGE="$ROOT/target/debpkg/$PKG"

echo "==> Staging package tree..."
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"

install -Dm755 "$ROOT/target/release/audra"                                   "$STAGE/usr/bin/audra"
install -Dm644 "$ROOT/data/io.github.amurpo.audra.desktop"                          "$STAGE/usr/share/applications/io.github.amurpo.audra.desktop"
install -Dm644 "$ROOT/data/io.github.amurpo.audra.metainfo.xml"                     "$STAGE/usr/share/metainfo/io.github.amurpo.audra.metainfo.xml"
install -Dm644 "$ROOT/data/icons/hicolor/scalable/apps/io.github.amurpo.audra.svg"  "$STAGE/usr/share/icons/hicolor/scalable/apps/io.github.amurpo.audra.svg"
mkdir -p "$STAGE/usr/share/locale/es/LC_MESSAGES"
msgfmt "$ROOT/po/es.po" -o "$STAGE/usr/share/locale/es/LC_MESSAGES/audra.mo"

# Depends are hardcoded to mirror the RPM spec's `Requires` (gtk4, libadwaita;
# sqlite is bundled via rusqlite) plus ALSA, which rodio links at runtime.
# `libasound2t64` is the package name since the time_t 64-bit transition
# (Debian trixie / Ubuntu 24.04+); the DEB is built in a debian:trixie container.
cat > "$STAGE/DEBIAN/control" <<EOF
Package: audra
Version: $DEB_VER
Architecture: amd64
Maintainer: Daniel Avila <daigo.tnt@gmail.com>
Section: sound
Priority: optional
Homepage: https://github.com/amurpo/audra
Depends: libgtk-4-1, libadwaita-1-0, libasound2t64, adwaita-icon-theme
Description: Native music player for Linux with Last.fm scrobbling
 Audra is a native music player for Linux (GTK4/libadwaita)
 with Last.fm integration and automatic scrobbling.
EOF

# Trigger AppStream catalog rebuild so GNOME Software / KDE Discover pick up
# the metainfo and expose "Details" / "Uninstall" for the app.
cat > "$STAGE/DEBIAN/triggers" <<'EOF'
activate-noawait update-app-info
EOF

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database >/dev/null 2>&1 || true
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli refresh --force >/dev/null 2>&1 \
        || appstreamcli refresh-cache --force >/dev/null 2>&1 \
        || true
fi
EOF

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
update-desktop-database >/dev/null 2>&1 || true
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli refresh --force >/dev/null 2>&1 \
        || appstreamcli refresh-cache --force >/dev/null 2>&1 \
        || true
fi
EOF

chmod 644 "$STAGE/DEBIAN/triggers"
chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"

echo "==> Building DEB..."
dpkg-deb --root-owner-group --build "$STAGE" "$ROOT/target/debpkg/$PKG.deb"

echo "==> Done: $ROOT/target/debpkg/$PKG.deb"
