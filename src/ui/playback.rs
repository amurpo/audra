use gtk4::prelude::*;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::rc::Rc;

use crate::library::{art, Track};
use crate::library::db::Database;
use crate::player::{Player, PlayerState};
use crate::scrobbler::LastFmClient;
use crate::ui::player_bar::PlayerBar;

pub struct ScrobbleTracker {
    pub scrobbled: bool,
}

impl Default for ScrobbleTracker {
    fn default() -> Self {
        Self { scrobbled: false }
    }
}

pub fn make_play_callback(
    player: Rc<RefCell<Player>>,
    bar: Rc<PlayerBar>,
    notify_now_playing: Rc<dyn Fn(&Track)>,
    highlight_track: Rc<dyn Fn(Option<&Track>)>,
) -> impl Fn(Vec<Track>, usize) {
    move |tracks, start_idx| {
        if tracks.is_empty() {
            return;
        }
        let mut p = player.borrow_mut();
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
            notify_now_playing(track);
            highlight_track(Some(track));
            bar.update_track(Some(track));
            bar.set_playing(true);
            let cover = art::read_cover_art(&track.path);
            bar.update_cover(cover.as_deref());
        }
    }
}

pub fn wire_transport_controls(
    bar: &Rc<PlayerBar>,
    player: &Rc<RefCell<Player>>,
    notify_now_playing: Rc<dyn Fn(&Track)>,
    highlight_track: Rc<dyn Fn(Option<&Track>)>,
) {
    {
        let player = Rc::clone(player);
        let bar_ref = Rc::clone(bar);
        bar.btn_play_pause.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            p.pause_resume();
            bar_ref.set_playing(matches!(p.state, PlayerState::Playing));
        });
    }
    {
        let player = Rc::clone(player);
        let bar_ref = Rc::clone(bar);
        let nnp = Rc::clone(&notify_now_playing);
        let ht = Rc::clone(&highlight_track);
        bar.btn_next.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.next() {
                nnp(track);
                ht(Some(track));
                bar_ref.update_track(Some(track));
                bar_ref.set_playing(true);
                bar_ref.update_cover(art::read_cover_art(&track.path).as_deref());
            }
        });
    }
    {
        let player = Rc::clone(player);
        let bar_ref = Rc::clone(bar);
        let nnp = Rc::clone(&notify_now_playing);
        let ht = Rc::clone(&highlight_track);
        bar.btn_prev.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.previous() {
                nnp(track);
                ht(Some(track));
                bar_ref.update_track(Some(track));
                bar_ref.set_playing(true);
                bar_ref.update_cover(art::read_cover_art(&track.path).as_deref());
            }
        });
    }
    {
        let player = Rc::clone(player);
        bar.btn_shuffle.connect_clicked(move |btn| {
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
        let player = Rc::clone(player);
        bar.btn_loop.connect_clicked(move |btn| {
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
        let player = Rc::clone(player);
        let prog_bar = bar.prog_bar.clone();
        bar.prog_gesture.connect_pressed(move |_, _, x, _| {
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
            p.seek((x / width).clamp(0.0, 1.0) * total);
        });
    }
    {
        let player = Rc::clone(player);
        let lbl_volume = bar.lbl_volume.clone();
        bar.vol_scale.connect_value_changed(move |scale| {
            let v = scale.value();
            player.borrow_mut().set_volume(v as f32);
            lbl_volume.set_text(&format!("{:.0}%", v * 100.0));
        });
    }
}

pub fn start_player_timer(
    player: Rc<RefCell<Player>>,
    bar: Rc<PlayerBar>,
    scrobble_tracker: Rc<RefCell<ScrobbleTracker>>,
    lastfm: Arc<Mutex<Option<LastFmClient>>>,
    db: Arc<Mutex<Database>>,
    notify_now_playing: Rc<dyn Fn(&Track)>,
    highlight_track: Rc<dyn Fn(Option<&Track>)>,
) {
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let mut p = player.borrow_mut();
        if !matches!(p.state, PlayerState::Playing) {
            return glib::ControlFlow::Continue;
        }

        if p.is_finished() {
            let result = if p.repeat_one { p.play_current() } else { p.next() };
            if let Ok(Some(track)) = result {
                notify_now_playing(track);
                highlight_track(Some(track));
                let cover = art::read_cover_art(&track.path);
                bar.update_track(Some(track));
                bar.update_cover(cover.as_deref());
                bar.set_playing(true);
            } else {
                highlight_track(None);
                bar.update_track(None);
                bar.set_playing(false);
            }
        } else {
            let pos = p.position().as_secs_f64();
            let total = p
                .current_track()
                .and_then(|t| t.duration_secs)
                .map(|d| d as f64)
                .unwrap_or(0.0);
            bar.update_progress(pos, total);

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
                        let db_sc = Arc::clone(&db);
                        std::thread::spawn(move || {
                            let guard = lf.lock().unwrap();
                            if let Some(client) = guard.as_ref() {
                                if client.scrobble(&artist, &title, &album, ts).is_err() {
                                    if let Some(id) = track_id {
                                        let _ = db_sc
                                            .lock()
                                            .unwrap()
                                            .queue_scrobble(id, &ts.to_string());
                                        log::warn!(
                                            "scrobbler: encolado '{}' - '{}'",
                                            artist,
                                            title
                                        );
                                    }
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
