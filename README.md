[![Release](https://github.com/amurpo/audra/actions/workflows/release.yml/badge.svg)](https://github.com/amurpo/audra/actions/workflows/release.yml)
# Audra

Native music player for Linux, built with GTK4 and libadwaita.

## Screenshots

![Audra album view](docs/screenshot1.png)
![Audra songs view](docs/screenshot2.png)

> Preliminary design — not the final design, subject to change.

## Features

- Music library with albums, artists and songs views
- Hierarchical navigation: artist → album → songs
- MP3, FLAC, OGG and WAV support
- Automatic scrobbling to [Last.fm](https://www.last.fm) with OAuth authentication
- Shuffle with fixed random order (each song plays once)
- Track repeat
- Artist art and album covers downloaded automatically
- Native interface following GNOME design guidelines

## Requirements

Runtime: GTK4, libadwaita, ALSA.

Build from source additionally needs a Rust toolchain and **gettext**
(`msgfmt`, used to compile the translation catalog — the build fails
loudly if it is missing).

### Build dependencies

Fedora / RHEL:

```bash
sudo dnf install \
  gcc pkg-config \
  gtk4-devel libadwaita-devel \
  alsa-lib-devel fontconfig-devel \
  gettext
```

Debian / Ubuntu:

```bash
sudo apt install \
  build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev \
  libgdk-pixbuf-2.0-dev libasound2-dev \
  gettext
```

## Installation

### RPM (Fedora / RHEL)

```bash
sudo dnf install audra-*.rpm
```

### DEB (Debian / Ubuntu)

```bash
sudo apt install ./audra_*_amd64.deb
```

### From source

```bash
cargo build --release
```

The binary will be at `target/release/audra`.

To build with Last.fm integration, export the proxy URL before building:

```bash
export LASTFM_PROXY_URL=https://your-proxy.example.com/lastfm
cargo build --release
```

Last.fm authentication uses the standard OAuth flow: the user authorizes on the official Last.fm
site and never enters credentials in the app. The proxy (Cloudflare Workers) signs requests
server-side — the binary only contains the public proxy URL.

## Uninstalling

Removing the package (`sudo dnf remove audra` / `sudo apt remove audra`) never
touches your personal data: package managers only delete files the package owns
under system paths, so your library, settings and cached covers are always kept
for a future reinstall.

To also wipe all per-user data, delete these directories manually:

```bash
rm -rf ~/.local/share/audra   # library database and downloaded covers
rm -rf ~/.config/audra        # bundled fonts and settings
```

On Windows the uninstaller asks whether to keep or delete this data, and the
installer reports whether it is repairing the same version or updating an
existing one.

## Building the RPM

```bash
bash packaging/build-rpm.sh
```

## Building the DEB

```bash
bash packaging/build-deb.sh
```

The package is written to `target/debpkg/audra_<version>_amd64.deb`.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
