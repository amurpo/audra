use gtk4::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::library::db::Database;
use crate::library::{art, Track};
use crate::player::mpris::{Mpris, MprisCommand};
use crate::player::{Player, PlayerState};
use crate::scrobbler::LastFmClient;
use crate::ui::dominant_color;
use crate::ui::player_bar::PlayerBar;
use crate::ui::theme;

/// Shared, mutable slot for the OS media controls. Starts empty and gets
/// populated by `main_window` once the window is realized — on Windows
/// `Mpris::new` must wait for a valid HWND. `start_player_timer` reads
/// this slot on every tick so it picks up the controls as soon as they
/// become available.
pub type MprisHandle = Rc<RefCell<Option<Mpris>>>;

/// Drain OS media-control commands on the GTK thread and apply them by
/// re-emitting the existing transport buttons — zero duplicated logic, so the
/// UI handlers stay the single source of truth (DRY/SRP).
pub fn wire_mpris(
    rx: async_channel::Receiver<MprisCommand>,
    player: Rc<RefCell<Player>>,
    bar: Rc<PlayerBar>,
    window: glib::WeakRef<adw::ApplicationWindow>,
) {
    // No polling: the future sleeps until souvlaki's thread sends a command.
    // It ends when the sender (owned by `Mpris`) is dropped or the window is
    // gone — e.g. after the window rebuild on a language change.
    glib::spawn_future_local(async move {
        while let Ok(cmd) = rx.recv().await {
            let Some(win) = window.upgrade() else {
                return;
            };
            let playing = matches!(player.borrow().state, PlayerState::Playing);
            match cmd {
                MprisCommand::PlayPause => bar.btn_play_pause.emit_clicked(),
                MprisCommand::Play if !playing => bar.btn_play_pause.emit_clicked(),
                MprisCommand::Pause | MprisCommand::Stop if playing => {
                    bar.btn_play_pause.emit_clicked()
                }
                MprisCommand::Next => bar.btn_next.emit_clicked(),
                MprisCommand::Previous => bar.btn_prev.emit_clicked(),
                MprisCommand::Raise => win.present(),
                _ => {}
            }
        }
    });
}

pub type HighlightCb = Rc<dyn Fn(Option<&Track>)>;

#[derive(Default)]
pub struct ScrobbleTracker {
    pub scrobbled: bool,
}

/// Maps every track path to its *canonical* `(artist, album)` — the exact key
/// the Albums view and the cover store use. Rebuilt on every library reload.
///
/// Covers are stored per canonical album (the deduplicated, dominant label),
/// but a track's own `artist`/`album` tags can differ from that label on
/// inconsistently tagged OSTs and compilations. Keying the lookup on the raw
/// per-track tags therefore missed for those tracks, so they fell back to
/// per-file embedded art — and showed the placeholder when the file had none.
/// Resolving through the canonical key keeps every track of an album on that
/// album's cover. See `library::dedup` for how the canonical label is derived.
pub type CoverIndex = Rc<RefCell<HashMap<String, (String, String)>>>;

/// Last resolved cover (and the palette extracted from it), keyed by the
/// canonical `(artist, album)`. Consecutive tracks of one album share their
/// cover, so auto-advance and next/prev hit this cache instead of re-querying
/// the DB / re-reading file tags and re-running palette extraction on every
/// track. A single entry is enough: the access pattern is "many hits on the
/// same key, then move on to the next album".
///
/// `Arc<Mutex<…>>` rather than `Rc<RefCell<…>>` because the palette worker
/// thread writes its result back into the entry.
pub type CoverCache = Arc<Mutex<Option<CoverCacheEntry>>>;

pub struct CoverCacheEntry {
    key: (String, String),
    cover: Option<Arc<Vec<u8>>>,
    /// `None` while extraction is still pending; `Some(result)` once the
    /// worker finished, where `result` is whatever `dominant_color::palette`
    /// returned (it can itself be `None` for undecodable images).
    palette: Option<Option<Vec<(u8, u8, u8)>>>,
}

/// Process-wide cache instance. A single shared slot (instead of one per
/// window) lets the cover picker invalidate it without threading the handle
/// through every view; the window rebuild on language change keeps it, which
/// is fine — the underlying DB data did not change.
static COVER_CACHE: std::sync::OnceLock<CoverCache> = std::sync::OnceLock::new();

pub fn cover_cache() -> CoverCache {
    Arc::clone(COVER_CACHE.get_or_init(|| Arc::new(Mutex::new(None))))
}

/// Drop the cached entry for `(artist, album)` if present, so the next
/// resolution re-reads the DB instead of serving stale bytes.
fn invalidate_cover_cache(artist: &str, album: &str) {
    let cache = cover_cache();
    let mut guard = cache.lock().unwrap();
    if guard
        .as_ref()
        .is_some_and(|e| e.key.0 == artist && e.key.1 == album)
    {
        *guard = None;
    }
}

/// Reaction to a user cover change: receives the canonical `(artist, album)`.
type CoverChangedFn = Box<dyn Fn(&str, &str)>;

thread_local! {
    /// UI-thread reaction to a user cover change, installed by
    /// [`wire_cover_sync`]. Kept in a thread_local rather than next to the
    /// global cache because it captures `Rc`-based UI state (the
    /// [`PlaybackCtx`]), which must never cross threads.
    static COVER_CHANGED_HANDLER: RefCell<Option<CoverChangedFn>> = const { RefCell::new(None) };
}

/// Entry point for the cover picker: a cover for `(artist, album)` was just
/// stored (or removed). Drops the cached entry and, when that album is the
/// one currently playing, repaints the bar/tint/OS controls immediately.
pub fn notify_cover_changed(artist: &str, album: &str) {
    invalidate_cover_cache(artist, album);
    COVER_CHANGED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler(artist, album);
        }
    });
}

/// Install the live-sync reaction to cover changes. When the changed album is
/// the one playing, the player-bar cover, the dynamic tint and the OS media
/// controls refresh right away — no waiting for the next track. Re-installed
/// on every window build (language change), replacing the previous handler.
pub fn wire_cover_sync(ctx: &PlaybackCtx, mpris: MprisHandle) {
    let c = ctx.clone();
    COVER_CHANGED_HANDLER.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |artist, album| {
            let track = {
                let p = c.player.borrow();
                let Some(track) = p.current_track() else {
                    return;
                };
                let key = canonical_key(&c.cover_index, track);
                if key.0 != artist || key.1 != album {
                    return;
                }
                track.clone()
            };
            update_cover_and_tint(&c, &track);
            if let Some(m) = mpris.borrow_mut().as_mut() {
                let cover = resolve_cover_cached(&c, &track);
                m.refresh_metadata(Some(&track), cover.as_deref().map(|v| v.as_slice()));
            }
        }));
    });
}

/// Everything the playback wiring needs to react to a track change: the
/// player, the bar it repaints, the DB for covers, the now-playing/highlight
/// notifiers and the cover caches. One cloneable bundle instead of the
/// half-dozen positional parameters each wiring function used to take — same
/// idea as `main_window::Views`. All fields are cheap `Rc`/`Arc` clones.
#[derive(Clone)]
pub struct PlaybackCtx {
    pub player: Rc<RefCell<Player>>,
    pub bar: Rc<PlayerBar>,
    pub db: Arc<Mutex<Database>>,
    pub notify_now_playing: Rc<dyn Fn(&Track)>,
    pub highlight: HighlightCb,
    pub cover_index: CoverIndex,
    pub cover_cache: CoverCache,
}

/// Canonical album key for a track; falls back to the track's raw tags when
/// the path is not indexed yet (e.g. a reload has not run for this track).
fn canonical_key(index: &CoverIndex, track: &Track) -> (String, String) {
    index.borrow().get(&track.path).cloned().unwrap_or_else(|| {
        (
            track.artist.clone().unwrap_or_default(),
            track.album.clone().unwrap_or_default(),
        )
    })
}

/// Resolve the cover for a track: user-picked cover in the DB wins, then the
/// file's embedded art. An explicit empty record means the user removed the
/// cover on purpose, so we return `None` and let the bar fall back to the
/// placeholder instead of resurrecting the embedded art.
fn resolve_cover(index: &CoverIndex, db: &Arc<Mutex<Database>>, track: &Track) -> Option<Vec<u8>> {
    let (artist, album) = canonical_key(index, track);
    match db.lock().unwrap().get_cover(&artist, &album) {
        Some(bytes) if bytes.is_empty() => None,
        Some(bytes) => Some(bytes),
        None => art::read_cover_art(&track.path),
    }
}

/// Cache-aware cover resolution. On a miss the resolved bytes are stored
/// under the track's canonical key (palette pending); on a hit no DB or file
/// IO happens at all.
fn resolve_cover_cached(ctx: &PlaybackCtx, track: &Track) -> Option<Arc<Vec<u8>>> {
    let key = canonical_key(&ctx.cover_index, track);
    if let Some(entry) = ctx.cover_cache.lock().unwrap().as_ref() {
        if entry.key == key {
            return entry.cover.clone();
        }
    }
    let cover = resolve_cover(&ctx.cover_index, &ctx.db, track).map(Arc::new);
    *ctx.cover_cache.lock().unwrap() = Some(CoverCacheEntry {
        key,
        cover: cover.clone(),
        palette: None,
    });
    cover
}

/// Extract the palette on a worker thread, store it back into the cache (only
/// if the entry still belongs to `key`) and apply the tint on the GTK thread.
fn spawn_palette_extraction(ctx: &PlaybackCtx, key: (String, String), bytes: Arc<Vec<u8>>) {
    let cache = Arc::clone(&ctx.cover_cache);
    std::thread::spawn(move || {
        let palette = dominant_color::palette(&bytes, 5);
        {
            let mut guard = cache.lock().unwrap();
            if let Some(entry) = guard.as_mut().filter(|e| e.key == key) {
                entry.palette = Some(palette.clone());
            }
        }
        glib::idle_add_once(move || {
            theme::update_dynamic_tint(palette);
        });
    });
}

/// Update the player-bar cover and the dynamic window tint for `track`.
/// Within one album both come straight from [`CoverCache`]; a new album
/// resolves the cover once and extracts the palette once, off-thread.
fn update_cover_and_tint(ctx: &PlaybackCtx, track: &Track) {
    let key = canonical_key(&ctx.cover_index, track);

    let hit = {
        let guard = ctx.cover_cache.lock().unwrap();
        guard
            .as_ref()
            .filter(|e| e.key == key)
            .map(|e| (e.cover.clone(), e.palette.clone()))
    };
    if let Some((cover, palette)) = hit {
        ctx.bar.update_cover(cover.as_deref().map(|v| v.as_slice()));
        match (cover, palette) {
            (None, _) => theme::update_dynamic_tint(None),
            (Some(_), Some(palette)) => theme::update_dynamic_tint(palette),
            // Cover known but extraction still pending (e.g. two quick track
            // changes in the same album): just run it again.
            (Some(bytes), None) => spawn_palette_extraction(ctx, key, bytes),
        }
        return;
    }

    let cover = resolve_cover(&ctx.cover_index, &ctx.db, track).map(Arc::new);
    *ctx.cover_cache.lock().unwrap() = Some(CoverCacheEntry {
        key: key.clone(),
        cover: cover.clone(),
        palette: None,
    });
    ctx.bar.update_cover(cover.as_deref().map(|v| v.as_slice()));
    match cover {
        None => theme::update_dynamic_tint(None),
        Some(bytes) => spawn_palette_extraction(ctx, key, bytes),
    }
}

/// The one "a new track started" sequence, shared by the play callback, the
/// transport buttons and the auto-advance timer so the four paths cannot
/// drift apart: notify Last.fm, highlight the row, repaint the bar and
/// refresh cover + tint.
pub fn on_track_started(ctx: &PlaybackCtx, track: &Track) {
    (ctx.notify_now_playing)(track);
    (ctx.highlight)(Some(track));
    ctx.bar.update_track(Some(track));
    ctx.bar.set_playing(true);
    update_cover_and_tint(ctx, track);
}

/// Counterpart of [`on_track_started`] for the end of the queue: clear the
/// highlight, reset the bar and revert the tint to the theme default.
fn on_playback_stopped(ctx: &PlaybackCtx) {
    (ctx.highlight)(None);
    ctx.bar.update_track(None);
    ctx.bar.update_cover(None);
    theme::update_dynamic_tint(None);
    ctx.bar.set_playing(false);
}

pub fn make_play_callback(ctx: PlaybackCtx) -> impl Fn(Vec<Track>, usize) {
    move |tracks, start_idx| {
        if tracks.is_empty() {
            return;
        }
        let mut p = ctx.player.borrow_mut();
        // usize::MAX signals "play all" — if shuffle is on, start from a random position
        let actual_start = if start_idx == usize::MAX {
            if p.shuffle && tracks.len() > 1 {
                use rand::Rng;
                rand::thread_rng().gen_range(0..tracks.len())
            } else {
                0
            }
        } else {
            start_idx
        };
        p.load_queue(tracks, actual_start);
        // Rebuild shuffle order immediately so the universal shuffle state takes effect
        if p.shuffle {
            p.reshuffle();
        }
        if let Ok(Some(track)) = p.play_current() {
            on_track_started(&ctx, track);
        }
    }
}

pub fn wire_transport_controls(ctx: &PlaybackCtx) {
    {
        let c = ctx.clone();
        ctx.bar.btn_play_pause.connect_clicked(move |_| {
            let mut p = c.player.borrow_mut();
            p.pause_resume();
            c.bar.set_playing(matches!(p.state, PlayerState::Playing));
        });
    }
    {
        let c = ctx.clone();
        ctx.bar.btn_next.connect_clicked(move |_| {
            let mut p = c.player.borrow_mut();
            if let Ok(Some(track)) = p.next() {
                on_track_started(&c, track);
            }
        });
    }
    {
        let c = ctx.clone();
        ctx.bar.btn_prev.connect_clicked(move |_| {
            let mut p = c.player.borrow_mut();
            if let Ok(Some(track)) = p.previous() {
                on_track_started(&c, track);
            }
        });
    }
    {
        let player = Rc::clone(&ctx.player);
        ctx.bar.btn_shuffle.connect_clicked(move |btn| {
            let mut p = player.borrow_mut();
            p.shuffle = !p.shuffle;
            if p.shuffle {
                p.reshuffle();
                btn.add_css_class("accent");
            } else {
                btn.remove_css_class("accent");
            }
        });
    }
    {
        let player = Rc::clone(&ctx.player);
        ctx.bar.btn_loop.connect_clicked(move |btn| {
            let mut p = player.borrow_mut();
            p.repeat_one = !p.repeat_one;
            if p.repeat_one {
                btn.add_css_class("accent");
            } else {
                btn.remove_css_class("accent");
            }
        });
    }
    {
        let player = Rc::clone(&ctx.player);
        let bar = Rc::clone(&ctx.bar);
        let prog_bar = ctx.bar.prog_bar.clone();
        ctx.bar.prog_gesture.connect_pressed(move |_, _, x, _| {
            let p = player.borrow();
            let total = p
                .current_track()
                .and_then(|t| t.duration_secs)
                .map(|d| d as f64)
                .unwrap_or(0.0);
            if total <= 0.0 {
                return;
            }
            let width = prog_bar.width() as f64;
            if width <= 0.0 {
                return;
            }
            let target = (x / width).clamp(0.0, 1.0) * total;
            p.seek(target);
            // While paused the tick loop bails out before refreshing the bar, so
            // redraw it here to reflect the new position immediately.
            bar.update_progress(target, total);
        });
    }
    {
        let player = Rc::clone(&ctx.player);
        let lbl_volume = ctx.bar.lbl_volume.clone();
        ctx.bar.vol_scale.connect_value_changed(move |scale| {
            let v = scale.value();
            player.borrow_mut().set_volume(v as f32);
            lbl_volume.set_text(&format!("{:.0}%", v * 100.0));
        });
    }
}

pub fn start_player_timer(
    ctx: PlaybackCtx,
    scrobble_tracker: Rc<RefCell<ScrobbleTracker>>,
    lastfm: Arc<Mutex<Option<LastFmClient>>>,
    window: glib::WeakRef<adw::ApplicationWindow>,
    mpris: MprisHandle,
) {
    let mut mpris_last: Option<String> = None;
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        // Stop the timer once its window is gone (e.g. rebuilt on language
        // change); this drops the captured Player and frees the audio engine.
        if window.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }

        // Keep the OS media controls in sync every tick (state/position is
        // cheap); only resolve the cover when the track changes — and via the
        // cache, so within one album not even the DB is hit.
        // The mpris slot starts empty and is populated by main_window once
        // the window is realized — on Windows we need the HWND first.
        {
            let mut slot = mpris.borrow_mut();
            if let Some(m) = slot.as_mut() {
                let p = ctx.player.borrow();
                let pos = p.position();
                let track = p.current_track();
                let cur_path = track.map(|t| t.path.clone());
                m.set_playback(&p.state, pos);
                if cur_path != mpris_last {
                    mpris_last = cur_path;
                    let cover = track.and_then(|t| resolve_cover_cached(&ctx, t));
                    m.update_track(track, cover.as_deref().map(|v| v.as_slice()));
                }
            }
        }

        let mut p = ctx.player.borrow_mut();
        if !matches!(p.state, PlayerState::Playing) {
            return glib::ControlFlow::Continue;
        }

        if p.is_finished() {
            let result = if p.repeat_one {
                p.play_current()
            } else {
                p.next()
            };
            if let Ok(Some(track)) = result {
                on_track_started(&ctx, track);
            } else {
                on_playback_stopped(&ctx);
            }
        } else {
            let pos = p.position().as_secs_f64();
            let total = p
                .current_track()
                .and_then(|t| t.duration_secs)
                .map(|d| d as f64)
                .unwrap_or(0.0);
            ctx.bar.update_progress(pos, total);

            if !scrobble_tracker.borrow().scrobbled && total > 30.0 {
                let threshold = f64::min(total * 0.5, 240.0);
                if pos >= threshold {
                    scrobble_tracker.borrow_mut().scrobbled = true;
                    if let Some(track) = p.current_track() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        let artist = track.artist.clone().unwrap_or_default();
                        let title = track.title.clone().unwrap_or_default();
                        let album = track.album.clone().unwrap_or_default();
                        let track_id = track.id;
                        let lf = Arc::clone(&lastfm);
                        let db_sc = Arc::clone(&ctx.db);
                        std::thread::spawn(move || {
                            let sk = lf
                                .lock()
                                .unwrap()
                                .as_ref()
                                .and_then(|c| c.session_key().map(str::to_string));
                            let Some(sk) = sk else { return };
                            let client = LastFmClient::new().with_session(&sk);
                            if client.scrobble(&artist, &title, &album, ts).is_err() {
                                if let Some(id) = track_id {
                                    let _ =
                                        db_sc.lock().unwrap().queue_scrobble(id, &ts.to_string());
                                    log::warn!("scrobbler: encolado '{}' - '{}'", artist, title);
                                }
                            }
                        });
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, artist: &str, album: &str) -> Track {
        Track {
            id: None,
            path: path.to_string(),
            title: Some("T".into()),
            artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            track_num: Some(1),
            duration_secs: Some(180),
            disc_num: None,
            album_artist: None,
            mtime: None,
        }
    }

    fn db() -> Arc<Mutex<Database>> {
        Arc::new(Mutex::new(Database::open(":memory:").unwrap()))
    }

    #[test]
    fn resolves_through_canonical_key_when_raw_tags_differ() {
        // The bug: a cover stored under the album's canonical (artist, album)
        // was missed for tracks whose own tags differed from that label. The
        // index must redirect such a track to its album's cover.
        let db = db();
        db.lock()
            .unwrap()
            .set_cover("Lorien Testard", "OST (Act II)", &[1, 2, 3])
            .unwrap();
        let index: CoverIndex = Rc::new(RefCell::new(HashMap::new()));
        index.borrow_mut().insert(
            "/m/dualliste.mp3".into(),
            ("Lorien Testard".into(), "OST (Act II)".into()),
        );
        // Raw tags (duo performer) deliberately do NOT match the stored key,
        // and the file does not exist so the embedded-art fallback yields None.
        let t = track("/m/dualliste.mp3", "Lorien Testard & Alice", "OST (Act II)");
        assert_eq!(resolve_cover(&index, &db, &t), Some(vec![1, 2, 3]));
    }

    #[test]
    fn falls_back_to_raw_tags_when_path_not_indexed() {
        let db = db();
        db.lock().unwrap().set_cover("A", "B", &[9]).unwrap();
        let index: CoverIndex = Rc::new(RefCell::new(HashMap::new()));
        let t = track("/m/unindexed.mp3", "A", "B");
        assert_eq!(resolve_cover(&index, &db, &t), Some(vec![9]));
    }

    #[test]
    fn empty_cover_marker_returns_none() {
        // An empty BLOB means the user removed the cover on purpose; never
        // resurrect embedded art for it.
        let db = db();
        db.lock().unwrap().set_cover("A", "B", &[]).unwrap();
        let index: CoverIndex = Rc::new(RefCell::new(HashMap::new()));
        index
            .borrow_mut()
            .insert("/m/x.mp3".into(), ("A".into(), "B".into()));
        let t = track("/m/x.mp3", "raw", "raw");
        assert_eq!(resolve_cover(&index, &db, &t), None);
    }
}
