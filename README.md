[![Release](https://github.com/amurpo/audra/actions/workflows/release.yml/badge.svg)](https://github.com/amurpo/audra/actions/workflows/release.yml)
# Audra

Reproductor de música nativo para Linux, construido con GTK4 y libadwaita.

## Características

- Biblioteca de música con vistas por álbumes, artistas y canciones
- Navegación jerárquica: artista → álbum → canciones
- Soporte para MP3, FLAC, OGG y WAV
- Scrobbling automático a [Last.fm](https://www.last.fm) con autenticación OAuth
- Shuffle con orden aleatorio fijo (cada canción se escucha una vez)
- Repetición de pista
- Arte de artistas y carátulas descargadas automáticamente
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

Para compilar con integración de Last.fm, exporta la URL del proxy antes de compilar:

```bash
export LASTFM_PROXY_URL=https://tu-proxy.example.com/lastfm
cargo build --release
```

La autenticación de Last.fm usa el flujo OAuth estándar: el usuario autoriza en el sitio oficial
de Last.fm y nunca ingresa credenciales en la aplicación. El proxy (Cloudflare Workers) firma
las peticiones del lado del servidor — el binario solo contiene la URL pública del proxy.

## Construir el RPM

```bash
bash packaging/build-rpm.sh
```

## Licencia

GPL-3.0-or-later — ver [LICENSE](LICENSE).
