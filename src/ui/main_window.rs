use gtk4::prelude::*;
use gtk4::{Button, FileDialog, MenuButton, Popover, SearchBar, SearchEntry, ToggleButton, gio};
use libadwaita as adw;
use adw::prelude::*;
use glib::clone;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::rc::Rc;

use crate::library::{self, art, db::Database, scanner};
use crate::library::watcher::{WatcherEvent, start_folder_watcher};
use crate::player::{Player, PlayerState};
use crate::scrobbler::LastFmClient;
use crate::ui::albums_view::AlbumsView;
use crate::ui::artists_view::ArtistsView;
use crate::ui::library_view::LibraryView;
use crate::ui::player_bar::PlayerBar;

const APP_CSS: &str = "
picture.cover-art {
    border-radius: 8px;
}
picture.artist-image {
    border-radius: 9999px;
}
.cover-placeholder {
    border-radius: 8px;
    background-color: alpha(currentColor, 0.05);
    padding: 18px;
}
flowboxchild.mosaic-child {
    padding: 0;
    transition: opacity 120ms;
}
flowboxchild.mosaic-child:hover {
    opacity: 0.82;
}
.album-overlay-box {
    padding: 28px 8px 7px 8px;
    background: linear-gradient(rgba(0,0,0,0), rgba(0,0,0,0.72));
}
.album-overlay-title {
    font-weight: bold;
    font-size: 0.85em;
    color: white;
}
.album-overlay-artist {
    font-size: 0.78em;
    color: rgba(255,255,255,0.72);
}
.lastfm-ok {
    color: #33d17a;
}
.lastfm-err {
    color: #e01b24;
}
.cover-thumb {
    border-radius: 6px;
    background-color: alpha(currentColor, 0.05);
}
flowboxchild.artist-card {
    border-radius: 12px;
    transition: background-color 150ms;
    padding: 4px;
}
flowboxchild.artist-card:hover {
    background-color: alpha(currentColor, 0.07);
}
.scan-loading-overlay {
    background-color: alpha(@window_bg_color, 0.92);
}
.scan-loading-card {
    border-radius: 18px;
    padding: 36px 52px;
}
.bar-cover-placeholder {
    border-radius: 6px;
    background-color: alpha(currentColor, 0.06);
}
.bar-cover-note {
    font-size: 26px;
}
.album-cover-note {
    font-size: 52px;
}
label.now-playing-title {
    color: @accent_color;
    font-weight: bold;
}
";

struct ScrobbleTracker {
    scrobbled: bool,
}

impl Default for ScrobbleTracker {
    fn default() -> Self {
        Self { scrobbled: false }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(APP_CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn reload_all_views(
    db: &Arc<Mutex<Database>>,
    lib_view: &Rc<RefCell<LibraryView>>,
    albums_view: &Rc<AlbumsView>,
    artists_view: &Rc<ArtistsView>,
) {
    let all = db.lock().unwrap().all_tracks().unwrap_or_default();
    lib_view.borrow_mut().load_tracks(all.clone());
    let albums = library::group_into_albums(&all);
    let artists = library::group_into_artists(&albums);
    albums_view.load_albums(albums.clone(), Arc::clone(db));
    artists_view.load_artists(artists, albums);
}

fn make_play_callback(
    player: Rc<RefCell<Player>>,
    bar: Rc<PlayerBar>,
    notify_now_playing: Rc<dyn Fn(&crate::library::Track)>,
    highlight_track: Rc<dyn Fn(Option<&crate::library::Track>)>,
) -> impl Fn(Vec<crate::library::Track>, usize) {
    move |tracks, start_idx| {
        if tracks.is_empty() {
            return;
        }
        let mut p = player.borrow_mut();
        p.load_queue(tracks, start_idx);
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

fn start_scan(
    folder_path: String,
    db: Arc<Mutex<Database>>,
    lib_view: Rc<RefCell<LibraryView>>,
    albums_view: Rc<AlbumsView>,
    artists_view: Rc<ArtistsView>,
    watcher_events: Arc<Mutex<Vec<WatcherEvent>>>,
    watcher_handle: Rc<RefCell<Option<notify::RecommendedWatcher>>>,
    watcher_active: bool,
    loading_box: gtk4::Box,
    spinner: gtk4::Spinner,
) {
    loading_box.set_visible(true);
    spinner.start();

    let scan_path = folder_path.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(scanner::scan_folder(&scan_path));
    });

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        use std::sync::mpsc::TryRecvError;
        match rx.try_recv() {
            Ok(tracks) => {
                {
                    let db_g = db.lock().unwrap();
                    for t in &tracks {
                        let _ = db_g.upsert_track(t);
                    }
                    let _ = db_g.set_setting("music_folder", &folder_path);
                    let found: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
                    let removed = db_g
                        .remove_missing_from_folder(&folder_path, &found)
                        .unwrap_or(0);
                    if removed > 0 {
                        log::info!("sync: eliminados {} registros obsoletos", removed);
                    }
                }
                if watcher_active {
                    *watcher_handle.borrow_mut() =
                        start_folder_watcher(&folder_path, Arc::clone(&watcher_events));
                }
                reload_all_views(&db, &lib_view, &albums_view, &artists_view);
                loading_box.set_visible(false);
                spinner.stop();
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => {
                loading_box.set_visible(false);
                spinner.stop();
                glib::ControlFlow::Break
            }
        }
    });
}

fn start_watcher_event_loop(
    watcher_events: Arc<Mutex<Vec<WatcherEvent>>>,
    db: Arc<Mutex<Database>>,
    lib_view: Rc<RefCell<LibraryView>>,
    albums_view: Rc<AlbumsView>,
    artists_view: Rc<ArtistsView>,
) {
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let mut evts = watcher_events.lock().unwrap();
        if evts.is_empty() {
            return glib::ControlFlow::Continue;
        }
        let batch: Vec<WatcherEvent> = evts.drain(..).collect();
        drop(evts);

        let db_g = db.lock().unwrap();
        let mut changed = false;
        for evt in batch {
            match evt {
                WatcherEvent::Created(path) => {
                    if let Some(track) = scanner::scan_file(&path) {
                        let _ = db_g.upsert_track(&track);
                        changed = true;
                    }
                }
                WatcherEvent::Removed(path) => {
                    let _ = db_g.remove_track_by_path(&path);
                    changed = true;
                }
            }
        }
        drop(db_g);

        if changed {
            reload_all_views(&db, &lib_view, &albums_view, &artists_view);
        }
        glib::ControlFlow::Continue
    });
}

fn wire_transport_controls(
    bar: &Rc<PlayerBar>,
    player: &Rc<RefCell<Player>>,
    notify_now_playing: Rc<dyn Fn(&crate::library::Track)>,
    highlight_track: Rc<dyn Fn(Option<&crate::library::Track>)>,
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

fn start_player_timer(
    player: Rc<RefCell<Player>>,
    bar: Rc<PlayerBar>,
    scrobble_tracker: Rc<RefCell<ScrobbleTracker>>,
    lastfm: Arc<Mutex<Option<LastFmClient>>>,
    db: Arc<Mutex<Database>>,
    notify_now_playing: Rc<dyn Fn(&crate::library::Track)>,
    highlight_track: Rc<dyn Fn(Option<&crate::library::Track>)>,
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

// ── Diálogo Last.fm ───────────────────────────────────────────────────────────

fn show_lastfm_dialog(
    parent: &adw::ApplicationWindow,
    db: Arc<Mutex<Database>>,
    lastfm: Arc<Mutex<Option<LastFmClient>>>,
) {
    let win = adw::Window::builder()
        .title("Cuenta de Last.fm")
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .default_height(440)
        .resizable(false)
        .build();

    let header = adw::HeaderBar::new();
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(300);

    // Página 1: autorizar
    let auth_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    auth_box.set_valign(gtk4::Align::Center);
    auth_box.set_vexpand(true);
    auth_box.set_margin_top(24);
    auth_box.set_margin_bottom(24);
    auth_box.set_margin_start(24);
    auth_box.set_margin_end(24);

    let auth_status = adw::StatusPage::new();
    auth_status.set_icon_name(Some("avatar-default-symbolic"));
    auth_status.set_title("Conectar a Last.fm");
    auth_status.set_description(Some(
        "Autoriza Audra en tu cuenta de Last.fm para registrar tus escuchas.",
    ));
    auth_status.set_vexpand(true);

    let auth_error_label = gtk4::Label::new(None);
    auth_error_label.set_wrap(true);
    auth_error_label.set_use_markup(true);
    auth_error_label.set_halign(gtk4::Align::Center);

    let btn_authorize = Button::with_label("Autorizar en Last.fm");
    btn_authorize.add_css_class("suggested-action");
    btn_authorize.set_halign(gtk4::Align::Center);

    auth_box.append(&auth_status);
    auth_box.append(&auth_error_label);
    auth_box.append(&btn_authorize);

    // Página 2: esperando confirmación
    let wait_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    wait_box.set_valign(gtk4::Align::Center);
    wait_box.set_vexpand(true);
    wait_box.set_margin_top(24);
    wait_box.set_margin_bottom(24);
    wait_box.set_margin_start(24);
    wait_box.set_margin_end(24);

    let wait_status = adw::StatusPage::new();
    wait_status.set_icon_name(Some("network-transmit-receive-symbolic"));
    wait_status.set_title("Esperando autorización");
    wait_status.set_description(Some(
        "Completa la autorización en el navegador y luego haz clic en «Ya autoricé».",
    ));
    wait_status.set_vexpand(true);

    let wait_error_label = gtk4::Label::new(None);
    wait_error_label.set_wrap(true);
    wait_error_label.set_use_markup(true);
    wait_error_label.set_halign(gtk4::Align::Center);

    let wait_btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    wait_btn_row.set_halign(gtk4::Align::Center);

    let btn_confirmed = Button::with_label("Ya autoricé");
    btn_confirmed.add_css_class("suggested-action");
    let btn_cancel_wait = Button::with_label("Cancelar");

    wait_btn_row.append(&btn_confirmed);
    wait_btn_row.append(&btn_cancel_wait);
    wait_box.append(&wait_status);
    wait_box.append(&wait_error_label);
    wait_box.append(&wait_btn_row);

    // Página 3: conectado
    let ok_box = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
    ok_box.set_valign(gtk4::Align::Center);
    ok_box.set_vexpand(true);
    ok_box.set_margin_top(32);
    ok_box.set_margin_bottom(32);
    ok_box.set_margin_start(24);
    ok_box.set_margin_end(24);

    let ok_status = adw::StatusPage::new();
    ok_status.set_icon_name(Some("emblem-ok-symbolic"));
    ok_status.set_title("Conectado a Last.fm");
    ok_status.set_vexpand(true);

    let ok_btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    ok_btn_row.set_halign(gtk4::Align::Center);

    let btn_change = Button::with_label("Cambiar cuenta");
    let btn_forget = Button::with_label("Desconectar");
    btn_forget.add_css_class("destructive-action");

    ok_btn_row.append(&btn_change);
    ok_btn_row.append(&btn_forget);
    ok_box.append(&ok_status);
    ok_box.append(&ok_btn_row);

    stack.add_named(&auth_box, Some("authorize"));
    stack.add_named(&wait_box, Some("waiting"));
    stack.add_named(&ok_box, Some("connected"));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    win.set_content(Some(&toolbar));

    {
        let db_g = db.lock().unwrap();
        let username_val = db_g.get_setting("lastfm_username").unwrap_or_default();
        let connected = db_g
            .get_setting("lastfm_session_key")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        if connected && !username_val.is_empty() {
            ok_status.set_description(Some(&username_val));
            stack.set_visible_child_name("connected");
        } else {
            stack.set_visible_child_name("authorize");
        }
    }

    let pending_token: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    btn_authorize.connect_clicked(clone!(
        #[weak] auth_error_label,
        #[weak] stack,
        #[weak] btn_authorize,
        #[strong] pending_token,
        move |_| {
            if !LastFmClient::is_configured() {
                auth_error_label.set_markup(
                    "<span foreground='#e01b24'>La URL del proxy no está configurada.</span>",
                );
                return;
            }
            btn_authorize.set_sensitive(false);
            auth_error_label.set_text("");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            std::thread::spawn(move || {
                match LastFmClient::get_auth_token() {
                    Ok(r) => { let _ = tx.send(Ok((r.token, r.auth_url))); }
                    Err(e) => { let _ = tx.send(Err(e.to_string())); }
                }
            });

            let pending_c = Rc::clone(&pending_token);
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                use std::sync::mpsc::TryRecvError;
                match rx.try_recv() {
                    Ok(Ok((token, auth_url))) => {
                        *pending_c.borrow_mut() = Some(token);
                        let _ = std::process::Command::new("xdg-open").arg(&auth_url).spawn();
                        stack.set_visible_child_name("waiting");
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        auth_error_label.set_markup(&format!(
                            "<span foreground='#e01b24'>Error: {}</span>",
                            escape_markup(&e)
                        ));
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        }
    ));

    btn_confirmed.connect_clicked(clone!(
        #[weak] wait_error_label,
        #[weak] stack,
        #[weak] ok_status,
        #[weak] btn_confirmed,
        #[strong] pending_token,
        #[strong] db,
        #[strong] lastfm,
        move |_| {
            let token = match pending_token.borrow().clone() {
                Some(t) => t,
                None => {
                    wait_error_label.set_markup(
                        "<span foreground='#e01b24'>No hay token pendiente. Vuelve a autorizar.</span>",
                    );
                    return;
                }
            };
            btn_confirmed.set_sensitive(false);
            wait_error_label.set_text("");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            let db2 = Arc::clone(&db);
            let lastfm2 = Arc::clone(&lastfm);
            std::thread::spawn(move || {
                match LastFmClient::get_session(&token) {
                    Ok(r) => {
                        {
                            let db_g = db2.lock().unwrap();
                            let _ = db_g.set_setting("lastfm_session_key", &r.session_key);
                            let _ = db_g.set_setting("lastfm_username", &r.username);
                        }
                        let new_client = LastFmClient::new().with_session(&r.session_key);
                        *lastfm2.lock().unwrap() = Some(new_client);
                        let _ = tx.send(Ok((r.session_key, r.username)));
                    }
                    Err(e) => { let _ = tx.send(Err(e.to_string())); }
                }
            });

            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                use std::sync::mpsc::TryRecvError;
                match rx.try_recv() {
                    Ok(Ok((_sk, username))) => {
                        ok_status.set_description(Some(&username));
                        stack.set_visible_child_name("connected");
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        wait_error_label.set_markup(&format!(
                            "<span foreground='#e01b24'>Error: {}</span>",
                            escape_markup(&e)
                        ));
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        }
    ));

    btn_cancel_wait.connect_clicked(clone!(
        #[weak] stack,
        #[strong] pending_token,
        move |_| {
            *pending_token.borrow_mut() = None;
            stack.set_visible_child_name("authorize");
        }
    ));

    btn_change.connect_clicked(clone!(
        #[weak] stack,
        move |_| { stack.set_visible_child_name("authorize"); }
    ));

    btn_forget.connect_clicked(clone!(
        #[weak] stack,
        #[strong] db,
        #[strong] lastfm,
        move |_| {
            {
                let db_g = db.lock().unwrap();
                let _ = db_g.delete_setting("lastfm_session_key");
                let _ = db_g.delete_setting("lastfm_username");
            }
            *lastfm.lock().unwrap() = None;
            stack.set_visible_child_name("authorize");
        }
    ));

    win.present();
}

// ── Ventana principal ─────────────────────────────────────────────────────────

pub fn build_window(app: &adw::Application, db: Arc<Mutex<Database>>) {
    setup_css();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Audra")
        .default_width(1024)
        .default_height(680)
        .icon_name("com.audra.player")
        .build();

    #[cfg(target_os = "windows")]
    window.set_decorated(false);

    // --- Last.fm ---
    let lastfm: Arc<Mutex<Option<LastFmClient>>> = Arc::new(Mutex::new(None));
    {
        if LastFmClient::is_configured() {
            let db_g = db.lock().unwrap();
            if let Some(sk) = db_g.get_setting("lastfm_session_key").filter(|s| !s.is_empty()) {
                *lastfm.lock().unwrap() = Some(LastFmClient::new().with_session(&sk));
            }
        }
    }
    {
        let lf = Arc::clone(&lastfm);
        let db_flush = Arc::clone(&db);
        std::thread::spawn(move || {
            let guard = lf.lock().unwrap();
            if let Some(client) = guard.as_ref() {
                client.flush_queue(&db_flush.lock().unwrap());
            }
        });
    }

    // --- Header bar ---
    let header = adw::HeaderBar::new();

    let menu_btn = MenuButton::new();
    menu_btn.set_icon_name("folder-music-symbolic");
    menu_btn.set_tooltip_text(Some("Biblioteca"));
    menu_btn.add_css_class("flat");

    let popover = Popover::new();
    let pop_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    pop_box.set_margin_top(4);
    pop_box.set_margin_bottom(4);
    pop_box.set_margin_start(4);
    pop_box.set_margin_end(4);

    let item_scan = Button::with_label("Escanear colección");
    item_scan.add_css_class("flat");
    item_scan.set_halign(gtk4::Align::Fill);

    let item_watcher = gtk4::CheckButton::with_label("Sincronizar colección automáticamente");
    item_watcher.set_halign(gtk4::Align::Start);
    item_watcher.set_margin_start(8);
    item_watcher.set_margin_end(8);
    item_watcher.set_margin_top(2);
    item_watcher.set_margin_bottom(4);
    let watcher_was_enabled = db
        .lock()
        .unwrap()
        .get_setting("watcher_enabled")
        .map_or(false, |v| v == "1");
    item_watcher.set_active(watcher_was_enabled);

    let pop_sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep.set_margin_top(4);
    pop_sep.set_margin_bottom(4);

    let item_lastfm = Button::with_label("Cuenta de Last.fm");
    item_lastfm.add_css_class("flat");
    item_lastfm.set_halign(gtk4::Align::Fill);

    pop_box.append(&item_scan);
    pop_box.append(&item_watcher);
    pop_box.append(&pop_sep);
    pop_box.append(&item_lastfm);
    popover.set_child(Some(&pop_box));
    menu_btn.set_popover(Some(&popover));
    header.pack_start(&menu_btn);

    let btn_search = ToggleButton::new();
    btn_search.set_icon_name("system-search-symbolic");
    btn_search.set_tooltip_text(Some("Buscar"));
    btn_search.add_css_class("flat");
    header.pack_end(&btn_search);

    // --- Player, vistas y estado compartido ---
    let player: Rc<RefCell<Player>> = Rc::new(RefCell::new(
        Player::new().expect("Error iniciando el motor de audio"),
    ));
    let current_path: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let lib_view = Rc::new(RefCell::new(LibraryView::new(Rc::clone(&current_path))));
    let albums_view = Rc::new(AlbumsView::new(Rc::clone(&current_path)));
    let artists_view = Rc::new(ArtistsView::new(Arc::clone(&db), Rc::clone(&current_path)));
    let bar = Rc::new(PlayerBar::new());

    // --- Watcher de archivos ---
    let watcher_events: Arc<Mutex<Vec<WatcherEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let watcher_handle: Rc<RefCell<Option<notify::RecommendedWatcher>>> =
        Rc::new(RefCell::new(None));
    if watcher_was_enabled {
        if let Some(folder) = db.lock().unwrap().get_setting("music_folder") {
            *watcher_handle.borrow_mut() =
                start_folder_watcher(&folder, Arc::clone(&watcher_events));
        }
    }

    // --- Helpers de scrobble y highlight ---
    let scrobble_tracker = Rc::new(RefCell::new(ScrobbleTracker::default()));
    let notify_now_playing: Rc<dyn Fn(&crate::library::Track)> = {
        let lastfm = Arc::clone(&lastfm);
        let tracker = Rc::clone(&scrobble_tracker);
        Rc::new(move |track: &crate::library::Track| {
            tracker.borrow_mut().scrobbled = false;
            let artist = track.artist.clone().unwrap_or_default();
            let title = track.title.clone().unwrap_or_default();
            let album = track.album.clone().unwrap_or_default();
            let lf = Arc::clone(&lastfm);
            std::thread::spawn(move || {
                let guard = lf.lock().unwrap();
                if let Some(client) = guard.as_ref() {
                    client.update_now_playing(&artist, &title, &album);
                }
            });
        })
    };
    let highlight_track: Rc<dyn Fn(Option<&crate::library::Track>)> = {
        let lib = Rc::clone(&lib_view);
        let cp = Rc::clone(&current_path);
        Rc::new(move |track: Option<&crate::library::Track>| {
            let path = track.map(|t| t.path.clone());
            *cp.borrow_mut() = path.clone();
            lib.borrow().set_playing_path(path.as_deref());
        })
    };

    // --- Carga inicial ---
    reload_all_views(&db, &lib_view, &albums_view, &artists_view);

    // --- ViewStack ---
    let view_stack = adw::ViewStack::new();
    {
        let page = view_stack.add_titled(&albums_view.root, Some("albums"), "Álbumes");
        page.set_icon_name(Some("media-optical-symbolic"));
    }
    {
        let page = view_stack.add_titled(&artists_view.root, Some("artists"), "Artistas");
        page.set_icon_name(Some("system-users-symbolic"));
    }
    {
        let page =
            view_stack.add_titled(&lib_view.borrow().root, Some("tracks"), "Canciones");
        page.set_icon_name(Some("view-list-symbolic"));
    }
    view_stack.set_visible_child_name("albums");

    let view_switcher = adw::ViewSwitcher::new();
    view_switcher.set_stack(Some(&view_stack));
    view_switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
    header.set_title_widget(Some(&view_switcher));

    // --- Barra de búsqueda ---
    let search_entry = SearchEntry::new();
    search_entry.set_placeholder_text(Some("Buscar por título, artista o álbum…"));
    search_entry.set_hexpand(true);
    let search_bar = SearchBar::new();
    search_bar.set_show_close_button(false);
    search_bar.connect_entry(&search_entry);
    search_bar.set_child(Some(&search_entry));
    btn_search
        .bind_property("active", &search_bar, "search-mode-enabled")
        .sync_create()
        .bidirectional()
        .build();
    search_bar.set_key_capture_widget(Some(&window));

    // --- Layout ---
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.add_top_bar(&search_bar);
    toolbar_view.set_content(Some(&view_stack));
    toolbar_view.add_bottom_bar(&bar.root);

    let scan_overlay = gtk4::Overlay::new();
    scan_overlay.set_child(Some(&toolbar_view));

    let scan_loading_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    scan_loading_box.add_css_class("scan-loading-overlay");
    scan_loading_box.set_visible(false);
    let scan_card = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    scan_card.add_css_class("scan-loading-card");
    scan_card.set_halign(gtk4::Align::Center);
    scan_card.set_valign(gtk4::Align::Center);
    let scan_spinner = gtk4::Spinner::new();
    scan_spinner.set_size_request(48, 48);
    let scan_title_lbl = gtk4::Label::new(Some("Escaneando colección…"));
    scan_title_lbl.add_css_class("title-2");
    let scan_sub_lbl = gtk4::Label::new(Some("Esto puede tomar un momento"));
    scan_sub_lbl.add_css_class("dim-label");
    scan_card.append(&scan_spinner);
    scan_card.append(&scan_title_lbl);
    scan_card.append(&scan_sub_lbl);
    scan_loading_box.append(&scan_card);
    scan_overlay.add_overlay(&scan_loading_box);

    window.set_content(Some(&scan_overlay));

    // --- Señales ---

    {
        let lib_view = Rc::clone(&lib_view);
        let albums_view = Rc::clone(&albums_view);
        let artists_view = Rc::clone(&artists_view);
        search_entry.connect_search_changed(move |entry| {
            let q = entry.text();
            lib_view.borrow_mut().filter(&q);
            albums_view.filter(&q);
            artists_view.filter(&q);
        });
    }

    item_scan.connect_clicked(clone!(
        #[strong] window,
        #[strong] db,
        #[strong] lib_view,
        #[strong] albums_view,
        #[strong] artists_view,
        #[strong] watcher_events,
        #[strong] watcher_handle,
        #[weak] item_watcher,
        #[weak] popover,
        #[weak] scan_loading_box,
        #[weak] scan_spinner,
        move |_| {
            popover.popdown();
            let dialog = FileDialog::new();
            dialog.select_folder(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[strong] db,
                    #[strong] lib_view,
                    #[strong] albums_view,
                    #[strong] artists_view,
                    #[strong] watcher_events,
                    #[strong] watcher_handle,
                    #[weak] item_watcher,
                    #[weak] scan_loading_box,
                    #[weak] scan_spinner,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                start_scan(
                                    path.to_string_lossy().to_string(),
                                    Arc::clone(&db),
                                    Rc::clone(&lib_view),
                                    Rc::clone(&albums_view),
                                    Rc::clone(&artists_view),
                                    Arc::clone(&watcher_events),
                                    Rc::clone(&watcher_handle),
                                    item_watcher.is_active(),
                                    scan_loading_box,
                                    scan_spinner,
                                );
                            }
                        }
                    }
                ),
            );
        }
    ));

    item_lastfm.connect_clicked(clone!(
        #[strong] window,
        #[strong] db,
        #[strong] lastfm,
        #[weak] popover,
        move |_| {
            popover.popdown();
            show_lastfm_dialog(&window, Arc::clone(&db), Arc::clone(&lastfm));
        }
    ));

    item_watcher.connect_toggled(clone!(
        #[strong] db,
        #[strong] watcher_events,
        #[strong] watcher_handle,
        move |btn| {
            let enabled = btn.is_active();
            let _ = db
                .lock()
                .unwrap()
                .set_setting("watcher_enabled", if enabled { "1" } else { "0" });
            if enabled {
                if let Some(folder) = db.lock().unwrap().get_setting("music_folder") {
                    *watcher_handle.borrow_mut() =
                        start_folder_watcher(&folder, Arc::clone(&watcher_events));
                }
            } else {
                *watcher_handle.borrow_mut() = None;
            }
        }
    ));

    start_watcher_event_loop(
        Arc::clone(&watcher_events),
        Arc::clone(&db),
        Rc::clone(&lib_view),
        Rc::clone(&albums_view),
        Rc::clone(&artists_view),
    );

    albums_view.set_on_play(make_play_callback(
        Rc::clone(&player),
        Rc::clone(&bar),
        Rc::clone(&notify_now_playing),
        Rc::clone(&highlight_track),
    ));
    artists_view.set_on_play(make_play_callback(
        Rc::clone(&player),
        Rc::clone(&bar),
        Rc::clone(&notify_now_playing),
        Rc::clone(&highlight_track),
    ));
    {
        let lib_view_ref = Rc::clone(&lib_view);
        let play_cb = make_play_callback(
            Rc::clone(&player),
            Rc::clone(&bar),
            Rc::clone(&notify_now_playing),
            Rc::clone(&highlight_track),
        );
        lib_view.borrow().list_view.connect_activate(move |_, idx| {
            let tracks = lib_view_ref.borrow().all_tracks();
            play_cb(tracks, idx as usize);
        });
    }

    wire_transport_controls(
        &bar,
        &player,
        Rc::clone(&notify_now_playing),
        Rc::clone(&highlight_track),
    );

    start_player_timer(
        Rc::clone(&player),
        Rc::clone(&bar),
        Rc::clone(&scrobble_tracker),
        Arc::clone(&lastfm),
        Arc::clone(&db),
        Rc::clone(&notify_now_playing),
        Rc::clone(&highlight_track),
    );

    window.present();
}
