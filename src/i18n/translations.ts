import type { Translations } from "./types";

export const translations: Translations = {
  en: {
    meta: {
      title: "Audra — Native Desktop Music Player",
      description:
        "A native music player for desktop, built with GTK4 and libadwaita.",
    },
    nav: {
      download: "Download",
      viewSource: "View Source",
    },
    hero: {
      title: "Audra",
      subtitle:
        "A native music player for desktop, built with GTK4 and libadwaita.",
      description:
        "Browse your music library with a clean interface, listen to your favorite albums, and enjoy a fast, focused player designed for desktop.",
      downloadNow: "Download Now",
      viewOnGitHub: "View on GitHub",
    },
    overview: {
      title: "Overview",
      p1: "Audra is built for listeners who want a modern desktop music player that feels native on Linux, Windows and macOS. It focuses on the core experience: organizing your library, playing music smoothly, and keeping the interface simple and polished.",
      p2: "The app is designed around GNOME interface guidelines and uses GTK4 plus libadwaita for a consistent native look and feel.",
    },
    features: {
      title: "Features",
      items: [
        {
          title: "Music Library Views",
          desc: "Browse your collection by albums, artists, and songs.",
        },
        {
          title: "Hierarchical Browsing",
          desc: "Navigate seamlessly from artist to album to individual tracks.",
        },
        {
          title: "Multi-Format Support",
          desc: "Play MP3, FLAC, OGG, and WAV files without conversion.",
        },
        {
          title: "Automatic Artwork",
          desc: "Album covers and artist images download automatically.",
        },
        {
          title: "Smart Shuffle",
          desc: "Fixed random order ensures each song plays once before repeating.",
        },
        {
          title: "Track Repeat",
          desc: "Repeat modes for full library, albums, or individual songs.",
        },
        {
          title: "Last.fm Scrobbling",
          desc: "OAuth authentication keeps your listening history up to date.",
        },
        {
          title: "Native Interface",
          desc: "Built with GTK4 and libadwaita for true desktop integration.",
        },
        {
          title: "Fast & Focused",
          desc: "Minimal interface designed to get out of your way.",
        },
      ],
    },
    whyAudra: {
      title: "Why Audra",
      items: [
        {
          title: "Focused Experience",
          desc: "Audra keeps the experience focused on music instead of clutter. It is a good fit for users who want a desktop player that respects Linux design conventions while still handling modern library workflows.",
        },
        {
          title: "Reduced Setup",
          desc: "Because it supports common audio formats and automatically fetches artwork, it reduces the setup work needed to keep a local music collection pleasant to browse.",
        },
        {
          title: "Seamless Integration",
          desc: "With Last.fm integration, listeners can keep their scrobbles up to date without extra steps. Your music history stays synchronized with your account.",
        },
      ],
    },
    howItWorks: {
      title: "How It Works",
      steps: [
        {
          num: 1,
          title: "Open Audra",
          desc: "Launch the app on your desktop.",
        },
        {
          num: 2,
          title: "Browse Your Collection",
          desc: "Navigate by album, artist, or song.",
        },
        {
          num: 3,
          title: "Play Music",
          desc: "Play instantly or shuffle through your library.",
        },
        {
          num: 4,
          title: "Automatic Artwork",
          desc: "Audra downloads album art and artist images automatically.",
        },
        {
          num: 5,
          title: "Enable Scrobbling",
          desc: "Sign in to Last.fm via OAuth to enable automatic scrobble tracking.",
        },
      ],
    },
    installation: {
      title: "Installation & Build",
      gettingStarted: {
        title: "Getting Started",
        p1: "Audra provides packages for RPM-based and DEB-based Linux distributions. You can also build it from source with Cargo if you prefer compiling locally.",
        p2: "Supported runtime requirements include GTK4, libadwaita, and ALSA. Building from source additionally requires a Rust toolchain and gettext, since the translation catalog compilation depends on msgfmt.",
        p3: "If you want to package or compile Audra yourself, the project includes dedicated build instructions for RPM and DEB packages. The repository also documents how to enable Last.fm integration by setting a proxy URL before building.",
      },
      buildCommands: {
        title: "Build Commands",
        fromSource: "From Source (Linux/macOS)",
        debian: "Install Dependencies (Debian/Ubuntu)",
        fedora: "Install Dependencies (Fedora/RHEL)",
      },
      code: {
        fromSource: `git clone https://github.com/amurpo/audra.git
cd audra
cargo build --release`,
        debian: `sudo apt install libgtk-4-dev \\
  libadwaita-1-dev libmpv-dev \\
  gettext cargo rustc`,
        fedora: `sudo dnf install gtk4-devel \\
  libadwaita-devel mpv-devel \\
  gettext cargo`,
      },
    },
    footer: {
      licensing: "Licensing",
      licensingPrefix: "Audra is licensed under ",
      licensingSuffix:
        ". That makes it a strong fit for open-source Linux users who prefer transparent, community-friendly software.",
      copyright: "© 2024 Audra. Built with Rust, GTK4, and libadwaita.",
      github: "GitHub",
      issues: "Issues",
      discussions: "Discussions",
    },
    language: {
      label: "Language",
      en: "English",
      es: "Español",
    },
    a11y: {
      skipToContent: "Skip to content",
      screenshot1Alt: "Audra album view showing the music library interface",
      screenshot2Alt: "Audra songs view showing the track list interface",
    },
  },
  es: {
    meta: {
      title: "Audra — Reproductor de música nativo para escritorio",
      description:
        "Un reproductor de música nativo para escritorio, construido con GTK4 y libadwaita.",
    },
    nav: {
      download: "Descargar",
      viewSource: "Ver código",
    },
    hero: {
      title: "Audra",
      subtitle:
        "Un reproductor de música nativo para escritorio, construido con GTK4 y libadwaita.",
      description:
        "Explora tu biblioteca musical con una interfaz limpia, escucha tus álbumes favoritos y disfruta de un reproductor rápido y enfocado, diseñado para el escritorio.",
      downloadNow: "Descargar ahora",
      viewOnGitHub: "Ver en GitHub",
    },
    overview: {
      title: "Descripción general",
      p1: "Audra está pensado para quienes buscan un reproductor de música moderno que se sienta nativo en Linux, Windows y macOS. Se centra en lo esencial: organizar tu biblioteca, reproducir música con fluidez y mantener una interfaz simple y pulida.",
      p2: "La aplicación sigue las directrices de interfaz de GNOME y utiliza GTK4 junto con libadwaita para lograr un aspecto y una sensación nativos y coherentes.",
    },
    features: {
      title: "Características",
      items: [
        {
          title: "Vistas de biblioteca",
          desc: "Explora tu colección por álbumes, artistas y canciones.",
        },
        {
          title: "Navegación jerárquica",
          desc: "Navega sin esfuerzo de artista a álbum y a pistas individuales.",
        },
        {
          title: "Soporte multiformato",
          desc: "Reproduce archivos MP3, FLAC, OGG y WAV sin conversión.",
        },
        {
          title: "Carátulas automáticas",
          desc: "Las portadas de álbumes e imágenes de artistas se descargan automáticamente.",
        },
        {
          title: "Mezcla inteligente",
          desc: "Un orden aleatorio fijo garantiza que cada canción suene una vez antes de repetirse.",
        },
        {
          title: "Repetición de pistas",
          desc: "Modos de repetición para toda la biblioteca, álbumes o canciones individuales.",
        },
        {
          title: "Scrobbling en Last.fm",
          desc: "La autenticación OAuth mantiene tu historial de escucha actualizado.",
        },
        {
          title: "Interfaz nativa",
          desc: "Construido con GTK4 y libadwaita para una integración real con el escritorio.",
        },
        {
          title: "Rápido y enfocado",
          desc: "Interfaz mínima diseñada para no estorbar.",
        },
      ],
    },
    whyAudra: {
      title: "Por qué Audra",
      items: [
        {
          title: "Experiencia enfocada",
          desc: "Audra mantiene la experiencia centrada en la música, sin distracciones. Es ideal para quienes quieren un reproductor de escritorio que respete las convenciones de diseño de Linux y, al mismo tiempo, gestione flujos de biblioteca modernos.",
        },
        {
          title: "Menos configuración",
          desc: "Al admitir formatos de audio habituales y obtener carátulas automáticamente, reduce el trabajo necesario para mantener una colección local agradable de explorar.",
        },
        {
          title: "Integración fluida",
          desc: "Con la integración de Last.fm, puedes mantener tus scrobbles al día sin pasos extra. Tu historial musical permanece sincronizado con tu cuenta.",
        },
      ],
    },
    howItWorks: {
      title: "Cómo funciona",
      steps: [
        {
          num: 1,
          title: "Abre Audra",
          desc: "Inicia la aplicación en tu escritorio.",
        },
        {
          num: 2,
          title: "Explora tu colección",
          desc: "Navega por álbum, artista o canción.",
        },
        {
          num: 3,
          title: "Reproduce música",
          desc: "Reproduce al instante o mezcla tu biblioteca.",
        },
        {
          num: 4,
          title: "Carátulas automáticas",
          desc: "Audra descarga portadas de álbumes e imágenes de artistas automáticamente.",
        },
        {
          num: 5,
          title: "Activa el scrobbling",
          desc: "Inicia sesión en Last.fm mediante OAuth para habilitar el seguimiento automático de scrobbles.",
        },
      ],
    },
    installation: {
      title: "Instalación y compilación",
      gettingStarted: {
        title: "Primeros pasos",
        p1: "Audra ofrece paquetes para distribuciones Linux basadas en RPM y DEB. También puedes compilarlo desde el código fuente con Cargo si prefieres hacerlo localmente.",
        p2: "Los requisitos de ejecución incluyen GTK4, libadwaita y ALSA. Compilar desde el código fuente requiere además una cadena de herramientas de Rust y gettext, ya que la compilación del catálogo de traducciones depende de msgfmt.",
        p3: "Si quieres empaquetar o compilar Audra por tu cuenta, el proyecto incluye instrucciones dedicadas para paquetes RPM y DEB. El repositorio también documenta cómo habilitar la integración con Last.fm configurando una URL de proxy antes de compilar.",
      },
      buildCommands: {
        title: "Comandos de compilación",
        fromSource: "Desde el código fuente (Linux/macOS)",
        debian: "Instalar dependencias (Debian/Ubuntu)",
        fedora: "Instalar dependencias (Fedora/RHEL)",
      },
      code: {
        fromSource: `git clone https://github.com/amurpo/audra.git
cd audra
cargo build --release`,
        debian: `sudo apt install libgtk-4-dev \\
  libadwaita-1-dev libmpv-dev \\
  gettext cargo rustc`,
        fedora: `sudo dnf install gtk4-devel \\
  libadwaita-devel mpv-devel \\
  gettext cargo`,
      },
    },
    footer: {
      licensing: "Licencia",
      licensingPrefix: "Audra está licenciado bajo ",
      licensingSuffix:
        ". Esto lo convierte en una excelente opción para usuarios de Linux de código abierto que prefieren software transparente y orientado a la comunidad.",
      copyright: "© 2024 Audra. Construido con Rust, GTK4 y libadwaita.",
      github: "GitHub",
      issues: "Incidencias",
      discussions: "Discusiones",
    },
    language: {
      label: "Idioma",
      en: "English",
      es: "Español",
    },
    a11y: {
      skipToContent: "Saltar al contenido",
      screenshot1Alt:
        "Vista de álbumes de Audra mostrando la interfaz de la biblioteca musical",
      screenshot2Alt:
        "Vista de canciones de Audra mostrando la lista de pistas",
    },
  },
};

export const defaultLocale = "en" as const;
export const supportedLocales = ["en", "es"] as const;

export function isLocale(value: string): value is keyof typeof translations {
  return value === "en" || value === "es";
}

export function getCopy(locale: keyof typeof translations) {
  return translations[locale];
}
