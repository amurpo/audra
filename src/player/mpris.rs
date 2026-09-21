//! OS media-controls integration via `souvlaki`: MPRIS2 on Linux, SMTC on
//! Windows and MPNowPlayingInfoCenter on macOS — one API for all three.
//!
//! The rest of the app never sees souvlaki types: events are translated into
//! our own [`MprisCommand`] and pushed through a channel to the GTK main
//! thread (souvlaki invokes the handler from its own thread).

use crate::library::Track;
use crate::player::PlayerState;
use libadwaita as adw;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use std::time::Duration;

/// Transport command translated from the OS media controls (DIP: the app
/// depends on this, not on souvlaki).
#[derive(Debug)]
pub enum MprisCommand {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Stop,
    Raise,
}

pub struct Mpris {
    controls: MediaControls,
    last_track: Option<String>,
    cover_dir: std::path::PathBuf,
}

impl Mpris {
    /// Build the OS media controls and forward their events into `tx`.
    /// Returns `None` if the platform service is unavailable (e.g. no D-Bus
    /// session bus, or the native HWND is not allocated yet on Windows).
    pub fn new(
        window: &adw::ApplicationWindow,
        tx: async_channel::Sender<MprisCommand>,
    ) -> Option<Self> {
        let hwnd = window_handle(window);

        // Windows: souvlaki panics on a None HWND ("Windows media controls
        // require an HWND...") and SMTC's GetForWindow rejects a zero
        // handle. Bail out cleanly here so callers can retry once the
        // surface is realized.
        #[cfg(windows)]
        {
            if hwnd.is_none() {
                return None;
            }
        }

        let config = PlatformConfig {
            dbus_name: "audra",
            display_name: "Audra",
            hwnd,
        };
        let mut controls = MediaControls::new(config).ok()?;
        if controls
            .attach(move |event: MediaControlEvent| {
                let cmd = match event {
                    MediaControlEvent::Toggle => MprisCommand::PlayPause,
                    MediaControlEvent::Play => MprisCommand::Play,
                    MediaControlEvent::Pause => MprisCommand::Pause,
                    MediaControlEvent::Next => MprisCommand::Next,
                    MediaControlEvent::Previous => MprisCommand::Previous,
                    MediaControlEvent::Stop | MediaControlEvent::Quit => MprisCommand::Stop,
                    MediaControlEvent::Raise => MprisCommand::Raise,
                    _ => return,
                };
                // Unbounded channel: send_blocking never actually blocks
                // souvlaki's event thread.
                let _ = tx.send_blocking(cmd);
            })
            .is_err()
        {
            return None;
        }
        Some(Self {
            controls,
            last_track: None,
            cover_dir: cover_cache_dir(),
        })
    }

    /// MPRIS/SMTC need a file URL, not raw bytes. Write the cover to a
    /// content-addressed file (so identical art reuses one file and the URL
    /// changes when the art changes) and return `file://…`.
    fn cover_url(&self, bytes: &[u8]) -> Option<String> {
        if bytes.is_empty() {
            return None; // user removed the art on purpose
        }
        let _ = std::fs::create_dir_all(&self.cover_dir);
        // Name by the ORIGINAL bytes so identical art reuses one file without
        // re-decoding, but write a downscaled copy: covers are often huge
        // originals (megapixels) and MPRIS only shows them thumbnail-sized.
        let name = format!("{:x}.img", md5::compute(bytes));
        let path = self.cover_dir.join(name);
        if !path.exists() {
            let data = downscale_jpeg(bytes, 512).unwrap_or_else(|| bytes.to_vec());
            if std::fs::write(&path, &data).is_err() {
                return None;
            }
        }
        Some(format!("file://{}", path.display()))
    }

    /// Push title/artist/album/duration/cover, but only when the track
    /// actually changes (this is called every timer tick).
    pub fn update_track(&mut self, track: Option<&Track>, cover: Option<&[u8]>) {
        let path = track.map(|t| t.path.clone());
        if path == self.last_track {
            return;
        }
        self.last_track = path;
        let Some(track) = track else {
            let _ = self.controls.set_metadata(MediaMetadata::default());
            return;
        };
        // Windows: souvlaki issue #39 — loading cover art via file:// URL
        // hangs or fails silently on some Windows versions. Skip cover art
        // on Windows; MPRIS on Linux handles file:// URLs correctly.
        #[cfg(not(windows))]
        let cover_url = cover.and_then(|b| self.cover_url(b));
        #[cfg(windows)]
        let cover_url: Option<String> = None;
        let _ = self.controls.set_metadata(MediaMetadata {
            title: track.title.as_deref(),
            artist: track.artist.as_deref(),
            album: track.album.as_deref(),
            cover_url: cover_url.as_deref(),
            duration: track
                .duration_secs
                .map(|d| Duration::from_secs(d.max(0) as u64)),
        });
    }

    /// Re-push metadata even though the track path hasn't changed — used when
    /// the *cover* changed for the currently playing track, which the
    /// same-path guard in [`update_track`](Self::update_track) would swallow.
    pub fn refresh_metadata(&mut self, track: Option<&Track>, cover: Option<&[u8]>) {
        self.last_track = None;
        self.update_track(track, cover);
    }

    pub fn set_playback(&mut self, state: &PlayerState, position: Duration) {
        let playback = match state {
            PlayerState::Playing => MediaPlayback::Playing {
                progress: Some(MediaPosition(position)),
            },
            PlayerState::Paused => MediaPlayback::Paused {
                progress: Some(MediaPosition(position)),
            },
            PlayerState::Stopped => MediaPlayback::Stopped,
        };
        let _ = self.controls.set_playback(playback);
    }
}

#[cfg(windows)]
fn window_handle(window: &adw::ApplicationWindow) -> Option<*mut std::ffi::c_void> {
    use glib::translate::ToGlibPtr;
    use gtk4::prelude::*;

    // Called directly instead of through the gdk4-win32 crate: its 0.9
    // bindings still reference symbols GTK 4.21 dropped (GdkWin32Screen,
    // gdk_win32_display_get_win32hcursor), so linking it against a current
    // GTK fails even though we never call them. This is the same C function
    // the crate's Win32Surface::handle() wraps.
    extern "C" {
        fn gdk_win32_surface_get_handle(
            surface: *mut gtk4::gdk::ffi::GdkSurface,
        ) -> *mut std::ffi::c_void;
    }

    let surface = gtk4::prelude::NativeExt::surface(window)?;
    // No downcast without the crate, so walk the GType ancestry by hand —
    // calling the getter on a non-Win32 surface would be undefined behaviour.
    // The leaf type is not enough: a real window is a GdkWin32Toplevel, which
    // derives from GdkWin32Surface.
    let is_win32 = std::iter::successors(Some(surface.type_()), |t| t.parent())
        .any(|t| t.name() == "GdkWin32Surface");
    if !is_win32 {
        return None;
    }
    let raw = unsafe { gdk_win32_surface_get_handle(surface.to_glib_none().0) };
    if raw.is_null() {
        return None;
    }
    Some(raw)
}

#[cfg(not(windows))]
fn window_handle(_window: &adw::ApplicationWindow) -> Option<*mut std::ffi::c_void> {
    // Linux (MPRIS) and macOS do not need a native handle.
    None
}

/// Directory holding the downscaled MPRIS thumbnail files. Cache, not data:
/// regenerable on play, so it lives where cache cleaners may reap it. The
/// library reset deliberately leaves this alone — it refills itself as tracks
/// play, so there is nothing to clear.
pub fn cover_cache_dir() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("audra")
        .join("mpris")
}

/// Re-encode `bytes` as a JPEG whose longest side is at most `max` px,
/// preserving aspect ratio. Returns `None` (so the caller keeps the original)
/// when the image already fits or cannot be decoded — re-encoding a small
/// cover only loses quality for no size win.
fn downscale_jpeg(bytes: &[u8], max: i32) -> Option<Vec<u8>> {
    use gdk_pixbuf::prelude::PixbufLoaderExt;
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let src = loader.pixbuf()?;
    let (w, h) = (src.width(), src.height());
    if w <= 0 || h <= 0 || (w <= max && h <= max) {
        return None;
    }
    let (nw, nh) = if w >= h {
        (max, (max * h / w).max(1))
    } else {
        ((max * w / h).max(1), max)
    };
    let scaled = src.scale_simple(nw, nh, gdk_pixbuf::InterpType::Bilinear)?;
    scaled.save_to_bufferv("jpeg", &[("quality", "85")]).ok()
}
