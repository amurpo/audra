[![Release](https://github.com/amurpo/audra/actions/workflows/release.yml/badge.svg)](https://github.com/amurpo/audra/actions/workflows/release.yml) [![Nightly (dev)](https://github.com/amurpo/audra/actions/workflows/nightly.yml/badge.svg)](https://github.com/amurpo/audra/actions/workflows/nightly.yml)
# Audra

Native music player for Linux, built with GTK4 and libadwaita.

## Screenshots

![Audra album view](docs/screenshot1.png)
![Audra songs view](docs/screenshot2.png)

## Features

- Music library with albums, artists and songs views
- Hierarchical navigation: artist → album → songs
- MP3, FLAC, OGG and WAV support
- Shuffle with fixed random order (each song plays once) and track repeat
- ReplayGain volume normalization (track and album modes)
- MPRIS2 media controls on Linux (play/pause/skip from the desktop shell, lock screen, etc.)
- Automatic scrobbling and now-playing updates to [Last.fm](https://www.last.fm) with OAuth authentication
- Artist art and album covers downloaded automatically; right-click any album or artist to pick a custom image or search for one
- Automatic album and artist grouping that handles inconsistent tags — accent normalization is still partial
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

macOS (Apple Silicon, Homebrew):

```bash
brew install gtk4 libadwaita gettext pkg-config
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"
```

See [docs/BUILD-macos-arm64.md](docs/BUILD-macos-arm64.md) for the full macOS arm64 build, run, and verify steps.

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

On macOS (arm64), use `bash packaging/build-macos.sh` and `bash packaging/verify-macos-build.sh` to build and test; run with `bash packaging/run-macos.sh` so Homebrew GTK libraries are found.

To build with Last.fm integration, export the proxy URL before building:

```bash
export LASTFM_PROXY_URL=https://your-proxy.example.com/lastfm
cargo build --release
```

### Why a proxy instead of embedding the API key?

Last.fm's API requires every request to be **signed** with an API secret — not just the login,
but also every scrobble and now-playing update. The signature is an MD5 hash over the request
parameters plus that secret. Embedding the secret in an open-source binary is equivalent to
publishing it: anyone can extract it with `strings audra` and use your app's quota.

The solution is a small BFF (Backend-for-Frontend) proxy that holds the secret server-side and
signs requests on behalf of the client. The binary only needs to know the proxy's public URL.
The user's **session key** (obtained after OAuth) is stored locally, which is safe: it
authenticates the user to Last.fm but cannot be used to sign arbitrary API calls without the
secret.

Authentication uses the standard Last.fm OAuth flow: the user approves the app on the official
Last.fm site and never types credentials into Audra.

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

## Building on macOS (Apple Silicon)

```bash
bash packaging/build-macos.sh
bash packaging/verify-macos-build.sh   # build + arm64 check + cargo test
bash packaging/run-macos.sh            # launch with Homebrew libs on the loader path
```

Details: [docs/BUILD-macos-arm64.md](docs/BUILD-macos-arm64.md).

## Credits

The dynamic, album-art-based background (stacked diagonal color gradients
extracted from the cover) is adapted from [Amberol](https://gitlab.gnome.org/World/amberol)
by Emmanuele Bassi, also licensed GPL-3.0-or-later. Thank you.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
