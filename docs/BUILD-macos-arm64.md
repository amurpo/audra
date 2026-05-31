# Building Audra on macOS (Apple Silicon)

Audra is primarily developed for Linux (GTK4 + libadwaita). You can compile and run a native **arm64** binary on macOS for local development and testing. There is no official `.dmg` or Homebrew formula yet; this guide covers building from source.

## Requirements

| Tool | Purpose |
|------|---------|
| [Homebrew](https://brew.sh/) | GTK4, libadwaita, gettext, pkg-config |
| [Rust](https://rustup.rs/) (stable) | `cargo build` |
| Xcode Command Line Tools | C linker (`xcode-select --install`) |

### Homebrew packages

Install the GTK stack and build tools:

```bash
brew install gtk4 libadwaita gettext pkg-config
```

`gtk4` pulls in dependencies such as **graphene**, **gdk-pixbuf**, and **fontconfig** that `pkg-config` needs at compile time.

Ensure Homebrew’s `.pc` files are visible to `pkg-config`:

```bash
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"
```

Add that line to your shell profile (`~/.zshrc`) if you build often.

## Quick build

From the repository root:

```bash
bash packaging/build-macos.sh
```

The release binary is written to:

```text
target/release/audra
```

Confirm it is an Apple Silicon executable:

```bash
file target/release/audra
# Mach-O 64-bit executable arm64
```

## Manual build

```bash
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"

# Optional: Last.fm scrobbling via your BFF proxy (see README)
export LASTFM_PROXY_URL=https://your-proxy.example.com/lastfm

cargo build --release
```

`build.rs` compiles the Spanish catalog with `msgfmt` (from **gettext**) and links **fontconfig** (provided by Homebrew).

## Running the binary

GTK is linked against Homebrew libraries under `/opt/homebrew/lib`. If the app fails to start with missing-library errors, set:

```bash
export DYLD_FALLBACK_LIBRARY_PATH="/opt/homebrew/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
./target/release/audra
```

Or use the helper script:

```bash
bash packaging/run-macos.sh
```

On macOS, media keys and Now Playing use **MPNowPlayingInfoCenter** (via `souvlaki`), not MPRIS/D-Bus.

## Last.fm proxy URL

Same as Linux: set `LASTFM_PROXY_URL` before building so the URL is embedded at compile time. For local builds you can put it in a `.env` file at the repo root; `packaging/build-macos.sh` sources it when the variable is unset.

## Verify the build

Run unit tests and check the binary architecture:

```bash
bash packaging/verify-macos-build.sh
```

This script:

1. Ensures `pkg-config` can find GTK/graphene
2. Builds `audra` in release mode
3. Asserts the output is `arm64`
4. Runs `cargo test`

## Differences from Linux builds

| Topic | Linux | macOS (arm64) |
|-------|--------|----------------|
| Audio backend | ALSA | Core Audio (via `rodio`) |
| Desktop integration | MPRIS2 | Now Playing / media keys |
| Packaging | RPM, DEB, Flatpak | Ad-hoc binary only |
| System deps | `dnf` / `apt` dev packages | Homebrew `gtk4`, `libadwaita` |

## Troubleshooting

### `Package 'graphene-gobject-1.0' not found`

Install GTK and set `PKG_CONFIG_PATH` as above. If Homebrew is on Intel Mac paths, use `/usr/local/lib/pkgconfig` instead of `/opt/homebrew`.

### `msgfmt not found`

```bash
brew install gettext
# Homebrew gettext is keg-only; ensure msgfmt is on PATH:
export PATH="/opt/homebrew/opt/gettext/bin:$PATH"
```

### `libadwaita` / GTK version errors

Audra needs **libadwaita ≥ 1.6** (Cargo feature `v1_6`). Upgrade Homebrew packages:

```bash
brew upgrade gtk4 libadwaita
```

### App window does not appear

Run from Terminal to see `RUST_LOG` / GTK messages:

```bash
RUST_LOG=audra=debug bash packaging/run-macos.sh
```

## CI note

Linux CI (Fedora container) remains the canonical build pipeline. macOS arm64 builds are validated locally with `packaging/verify-macos-build.sh`; add a `macos-latest` workflow later if you want automated macOS binaries.
