# Audra

Reproductor de música nativo para Linux, construido con GTK4 y libadwaita.

## Características

- Biblioteca de música con vistas por álbumes, artistas y canciones
- Soporte para MP3, FLAC, OGG y WAV
- Scrobbling automático a [Last.fm](https://www.last.fm)
- Interfaz nativa siguiendo las guías de diseño de GNOME

## Requisitos

- GTK4
- libadwaita
- ALSA

## Instalación

### RPM (Fedora / RHEL)

```bash
sudo dnf install audra-*.rpm
```

### Desde el código fuente

```bash
cargo build --release
```

El binario queda en `target/release/audra`.

Para compilar con integración de Last.fm, exporta tus claves antes de compilar:

```bash
export LASTFM_API_KEY=tu_api_key
export LASTFM_API_SECRET=tu_api_secret
cargo build --release
```

## Construir el RPM

```bash
bash packaging/build-rpm.sh
```

## Licencia

GPL-3.0-or-later — ver [LICENSE](LICENSE).
