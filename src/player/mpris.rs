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
use std::sync::mpsc::Sender;
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
    pub fn new(window: &adw::ApplicationWindow, tx: Sender<MprisCommand>) -> Option<Self> {
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
                let _ = tx.send(cmd);
            })
            .is_err()
        {
            return None;
        }
        let cover_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("audra")
            .join("mpris");
        Some(Self {
            controls,
            last_track: None,
            cover_dir,
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
        let name = format!("{:x}.img", md5::compute(bytes));
        let path = self.cover_dir.join(name);
        if !path.exists() && std::fs::write(&path, bytes).is_err() {
            return None;
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
    use gtk4::prelude::*;
    let surface = gtk4::prelude::NativeExt::surface(window)?;
    let win32 = surface.downcast::<gdk4_win32::Win32Surface>().ok()?;
    let raw = win32.handle().0;
    if raw == 0 {
        return None;
    }
    Some(raw as *mut std::ffi::c_void)
}

#[cfg(not(windows))]
fn window_handle(_window: &adw::ApplicationWindow) -> Option<*mut std::ffi::c_void> {
    // Linux (MPRIS) and macOS do not need a native handle.
    None
}
