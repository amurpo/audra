use gtk4::prelude::*;
use gtk4::{Button, FileDialog, MenuButton, Popover, SearchBar, SearchEntry, ToggleButton, gio};
use libadwaita as adw;
use adw::prelude::*;
use glib::clone;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::rc::Rc;

use crate::library::{self, db::Database};
use crate::library::{art, scanner};
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
    cursor: pointer;
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
    cursor: pointer;
    transition: background-color 150ms;
    padding: 4px;
}
flowboxchild.artist-card:hover {
    background-color: alpha(currentColor, 0.07);
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

fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

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
        .default_height(500)
        .resizable(false)
        .build();

    let header = adw::HeaderBar::new();

    // Stack: alterna entre formulario y vista "conectado"
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(300);

    // ── Página 1: formulario ──────────────────────────────────────────────
    let form_box = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    form_box.set_margin_top(16);
    form_box.set_margin_bottom(20);
    form_box.set_margin_start(16);
    form_box.set_margin_end(16);

    let cred_group = adw::PreferencesGroup::new();
    cred_group.set_title("Cuenta de Last.fm");

    let user_row = adw::EntryRow::new();
    user_row.set_title("Usuario");

    let pass_row = adw::PasswordEntryRow::new();
    pass_row.set_title("Contraseña");

    cred_group.add(&user_row);
    cred_group.add(&pass_row);

    let status_label = gtk4::Label::new(None);
    status_label.set_wrap(true);
    status_label.set_xalign(0.0);
    status_label.set_use_markup(true);

    let form_btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    form_btn_row.set_halign(gtk4::Align::End);

    let btn_connect = Button::with_label("Conectar");
    btn_connect.add_css_class("suggested-action");

    form_btn_row.append(&btn_connect);

    form_box.append(&cred_group);
    form_box.append(&status_label);
    form_box.append(&form_btn_row);

    // ── Página 2: estado conectado ────────────────────────────────────────
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

    stack.add_named(&form_box, Some("form"));
    stack.add_named(&ok_box, Some("connected"));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));

    win.set_content(Some(&toolbar));

    // Pre-cargar y decidir qué página mostrar
    {
        let db_g = db.lock().unwrap();
        let username_val = db_g.get_setting("lastfm_username").unwrap_or_default();
        let connected = db_g
            .get_setting("lastfm_session_key")
            .map(|k| !k.is_empty())
            .unwrap_or(false);

        user_row.set_text(&username_val);

        if connected && !username_val.is_empty() {
            ok_status.set_description(Some(&username_val));
            stack.set_visible_child_name("connected");
        } else {
            stack.set_visible_child_name("form");
        }
    }

    // Botón Conectar
    btn_connect.connect_clicked(clone!(
        #[weak] user_row,
        #[weak] pass_row,
        #[weak] status_label,
        #[weak] stack,
        #[weak] ok_status,
        #[strong] db,
        #[strong] lastfm,
        move |btn| {
            let username = user_row.text().to_string();
            let password = pass_row.text().to_string();

            if !LastFmClient::is_configured() {
                status_label.set_markup(
                    "<span foreground='#e01b24'>La app no tiene API Key compilada. Compila con LASTFM_API_KEY y LASTFM_API_SECRET.</span>",
                );
                return;
            }

            if username.is_empty() || password.is_empty() {
                status_label.set_markup(
                    "<span foreground='#e01b24'>Completa usuario y contraseña.</span>",
                );
                return;
            }

            btn.set_sensitive(false);
            status_label.set_text("Conectando…");

            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let db2 = Arc::clone(&db);
            let lastfm2 = Arc::clone(&lastfm);

            std::thread::spawn(move || {
                let client = LastFmClient::new();
                match client.authenticate_with_password(&username, &password) {
                    Ok(sk) => {
                        {
                            let db_g = db2.lock().unwrap();
                            let _ = db_g.set_setting("lastfm_session_key", &sk);
                            let _ = db_g.set_setting("lastfm_username", &username);
                        }
                        let new_client = LastFmClient::new().with_session(&sk);
                        *lastfm2.lock().unwrap() = Some(new_client);
                        let _ = tx.send(Ok(username));
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
            });

            let btn_c = btn.clone();
            let status_c = status_label.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                use std::sync::mpsc::TryRecvError;
                match rx.try_recv() {
                    Ok(Ok(user)) => {
                        ok_status.set_description(Some(&user));
                        stack.set_visible_child_name("connected");
                        btn_c.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        status_c.set_markup(&format!(
                            "<span foreground='#e01b24'>Error: {}</span>",
                            escape_markup(&e)
                        ));
                        btn_c.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        btn_c.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        }
    ));

    // Botón Cambiar cuenta: vuelve al formulario sin borrar la sesión
    btn_change.connect_clicked(clone!(
        #[weak] stack,
        #[weak] pass_row,
        move |_| {
            pass_row.set_text("");
            stack.set_visible_child_name("form");
        }
    ));

    // Botón Desconectar: elimina sesión y vuelve al formulario
    btn_forget.connect_clicked(clone!(
        #[weak] stack,
        #[weak] pass_row,
        #[weak] status_label,
        #[strong] db,
        #[strong] lastfm,
        move |_| {
            {
                let db_g = db.lock().unwrap();
                let _ = db_g.delete_setting("lastfm_session_key");
                let _ = db_g.delete_setting("lastfm_username");
            }
            *lastfm.lock().unwrap() = None;
            pass_row.set_text("");
            status_label.set_text("Cuenta desvinculada.");
            stack.set_visible_child_name("form");
        }
    ));

    win.present();
}

pub fn build_window(app: &adw::Application, db: Arc<Mutex<Database>>) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(APP_CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Audra")
        .default_width(1024)
        .default_height(680)
        .icon_name("com.audra.player")
        .build();

    // --- Estado Last.fm: cargado al inicio ---
    let lastfm: Arc<Mutex<Option<LastFmClient>>> = Arc::new(Mutex::new(None));
    {
        if LastFmClient::is_configured() {
            let db_g = db.lock().unwrap();
            if let Some(sk) = db_g.get_setting("lastfm_session_key").filter(|s| !s.is_empty()) {
                *lastfm.lock().unwrap() = Some(LastFmClient::new().with_session(&sk));
            }
        }
    }
    // Flush de scrobbles pendientes al arrancar
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

    // Menú de biblioteca (reemplaza el botón único anterior)
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

    let item_lastfm = Button::with_label("Cuenta de Last.fm");
    item_lastfm.add_css_class("flat");
    item_lastfm.set_halign(gtk4::Align::Fill);

    pop_box.append(&item_scan);
    pop_box.append(&item_lastfm);
    popover.set_child(Some(&pop_box));
    menu_btn.set_popover(Some(&popover));
    header.pack_start(&menu_btn);

    let btn_search = ToggleButton::new();
    btn_search.set_icon_name("system-search-symbolic");
    btn_search.set_tooltip_text(Some("Buscar"));
    btn_search.add_css_class("flat");
    header.pack_end(&btn_search);

    // ViewSwitcher en el centro del header
    let view_stack = adw::ViewStack::new();

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

    // --- Player y vistas ---
    let player: Rc<RefCell<Player>> = Rc::new(RefCell::new(
        Player::new().expect("Error iniciando el motor de audio"),
    ));

    let lib_view = Rc::new(RefCell::new(LibraryView::new()));
    let albums_view = Rc::new(AlbumsView::new());
    let artists_view = Rc::new(ArtistsView::new(Arc::clone(&db)));
    let bar = Rc::new(PlayerBar::new());

    // Tracker de scrobbling (sólo hilo principal)
    let scrobble_tracker = Rc::new(RefCell::new(ScrobbleTracker::default()));

    // Helper: notifica "now playing" a Last.fm y resetea el tracker
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

    // Cargar datos iniciales
    let tracks = db.lock().unwrap().all_tracks().unwrap_or_default();
    {
        lib_view.borrow_mut().load_tracks(tracks.clone());
        let albums = library::group_into_albums(&tracks);
        let artists = library::group_into_artists(&albums);
        albums_view.load_albums(albums.clone(), Arc::clone(&db));
        artists_view.load_artists(artists, albums);
    }

    // --- Páginas del ViewStack ---
    {
        let page = view_stack.add_titled(&albums_view.root, Some("albums"), "Álbumes");
        page.set_icon_name(Some("media-optical-symbolic"));
    }
    {
        let page = view_stack.add_titled(&artists_view.root, Some("artists"), "Artistas");
        page.set_icon_name(Some("system-users-symbolic"));
    }
    {
        let page = view_stack.add_titled(
            &lib_view.borrow().root,
            Some("tracks"),
            "Canciones",
        );
        page.set_icon_name(Some("view-list-symbolic"));
    }

    view_stack.set_visible_child_name("albums");

    // --- Layout general ---
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.add_top_bar(&search_bar);
    toolbar_view.set_content(Some(&view_stack));
    toolbar_view.add_bottom_bar(&bar.root);

    window.set_content(Some(&toolbar_view));

    // --- Búsqueda en tiempo real ---
    {
        let lib_view = Rc::clone(&lib_view);
        let albums_view = Rc::clone(&albums_view);
        let artists_view = Rc::clone(&artists_view);
        search_entry.connect_search_changed(move |entry| {
            let query = entry.text();
            lib_view.borrow_mut().filter(&query);
            albums_view.filter(&query);
            artists_view.filter(&query);
        });
    }

    // --- Escanear colección ---
    item_scan.connect_clicked(clone!(
        #[strong] window,
        #[strong] db,
        #[strong] lib_view,
        #[strong] albums_view,
        #[strong] artists_view,
        #[weak] popover,
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
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let tracks = scanner::scan_folder(&path_str);
                                {
                                    let db = db.lock().unwrap();
                                    for t in &tracks {
                                        let _ = db.upsert_track(t);
                                    }
                                }
                                let all = db.lock().unwrap().all_tracks().unwrap_or_default();
                                lib_view.borrow_mut().load_tracks(all.clone());

                                let albums = library::group_into_albums(&all);
                                let artists = library::group_into_artists(&albums);
                                albums_view.load_albums(albums.clone(), Arc::clone(&db));
                                artists_view.load_artists(artists, albums);
                            }
                        }
                    }
                ),
            );
        }
    ));

    // --- Cuenta de Last.fm ---
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

    // --- Click en álbum: reproducir todo el álbum ---
    {
        let albums_view_ref = Rc::clone(&albums_view);
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        let nnp = Rc::clone(&notify_now_playing);
        albums_view.flow.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            let tracks = albums_view_ref.get_album_tracks(idx);
            if !tracks.is_empty() {
                let mut p = player.borrow_mut();
                p.load_queue(tracks, 0);
                if let Ok(Some(track)) = p.play_current() {
                    nnp(track);
                    bar_ref.update_track(Some(track));
                    bar_ref.set_playing(true);
                    let cover = art::read_cover_art(&track.path);
                    bar_ref.update_cover(cover.as_deref());
                }
            }
        });
    }

    // --- Click en artista / álbum del artista: reproducir ---
    {
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        let nnp = Rc::clone(&notify_now_playing);
        artists_view.set_on_play(move |tracks| {
            if !tracks.is_empty() {
                let mut p = player.borrow_mut();
                p.load_queue(tracks, 0);
                if let Ok(Some(track)) = p.play_current() {
                    nnp(track);
                    bar_ref.update_track(Some(track));
                    bar_ref.set_playing(true);
                    let cover = art::read_cover_art(&track.path);
                    bar_ref.update_cover(cover.as_deref());
                }
            }
        });
    }

    // --- Doble click en track: reproducir ---
    {
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        let lib_view_ref = Rc::clone(&lib_view);
        let nnp = Rc::clone(&notify_now_playing);
        lib_view.borrow().list_view.connect_activate(move |_, idx| {
            let lv = lib_view_ref.borrow();
            let tracks = lv.all_tracks();
            drop(lv);
            let mut p = player.borrow_mut();
            p.load_queue(tracks, idx as usize);
            if let Ok(Some(track)) = p.play_current() {
                nnp(track);
                bar_ref.update_track(Some(track));
                bar_ref.set_playing(true);
                let cover = art::read_cover_art(&track.path);
                bar_ref.update_cover(cover.as_deref());
            }
        });
    }

    // --- Play/Pause ---
    {
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        bar.btn_play_pause.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            p.pause_resume();
            let playing = matches!(p.state, PlayerState::Playing);
            bar_ref.set_playing(playing);
        });
    }

    // --- Siguiente ---
    {
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        let nnp = Rc::clone(&notify_now_playing);
        bar.btn_next.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.next() {
                nnp(track);
                bar_ref.update_track(Some(track));
                bar_ref.set_playing(true);
                let cover = art::read_cover_art(&track.path);
                bar_ref.update_cover(cover.as_deref());
            }
        });
    }

    // --- Anterior ---
    {
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        let nnp = Rc::clone(&notify_now_playing);
        bar.btn_prev.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.previous() {
                nnp(track);
                bar_ref.update_track(Some(track));
                bar_ref.set_playing(true);
                let cover = art::read_cover_art(&track.path);
                bar_ref.update_cover(cover.as_deref());
            }
        });
    }

    // --- Shuffle ---
    {
        let player = Rc::clone(&player);
        bar.btn_shuffle.connect_clicked(move |btn| {
            let mut p = player.borrow_mut();
            p.shuffle = !p.shuffle;
            if p.shuffle {
                btn.add_css_class("accent");
            } else {
                btn.remove_css_class("accent");
            }
        });
    }

    // --- Loop ---
    {
        let player = Rc::clone(&player);
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

    // --- Volumen ---
    {
        let player = Rc::clone(&player);
        bar.vol_scale.connect_value_changed(move |scale| {
            player.borrow_mut().set_volume(scale.value() as f32);
        });
    }

    // --- Timer: auto-avance + progreso + scrobbling ---
    {
        let player = Rc::clone(&player);
        let bar = Rc::clone(&bar);
        let tracker = Rc::clone(&scrobble_tracker);
        let lastfm = Arc::clone(&lastfm);
        let nnp = Rc::clone(&notify_now_playing);

        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let mut p = player.borrow_mut();
            if matches!(p.state, PlayerState::Playing) {
                if p.is_finished() {
                    let result = if p.repeat_one {
                        p.play_current()
                    } else {
                        p.next()
                    };
                    if let Ok(Some(track)) = result {
                        nnp(track);
                        let cover = art::read_cover_art(&track.path);
                        bar.update_track(Some(track));
                        bar.update_cover(cover.as_deref());
                        bar.set_playing(true);
                    } else {
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

                    // Scrobble al 50% (o 4 minutos, lo que ocurra primero)
                    if !tracker.borrow().scrobbled && total > 30.0 {
                        let threshold = f64::min(total * 0.5, 240.0);
                        if pos >= threshold {
                            tracker.borrow_mut().scrobbled = true;
                            if let Some(track) = p.current_track() {
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                let artist   = track.artist.clone().unwrap_or_default();
                                let title    = track.title.clone().unwrap_or_default();
                                let album    = track.album.clone().unwrap_or_default();
                                let track_id = track.id;
                                let lf       = Arc::clone(&lastfm);
                                let db_sc    = Arc::clone(&db);
                                std::thread::spawn(move || {
                                    let guard = lf.lock().unwrap();
                                    if let Some(client) = guard.as_ref() {
                                        if client.scrobble(&artist, &title, &album, ts).is_err() {
                                            // Sin conexión o proxy caído — encolar para resync
                                            if let Some(id) = track_id {
                                                let _ = db_sc.lock().unwrap()
                                                    .queue_scrobble(id, &ts.to_string());
                                                log::warn!("scrobbler: encolado '{}' - '{}'", artist, title);
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    window.present();
}
