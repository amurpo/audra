use adw::prelude::*;
use glib::clone;
use gtk4::prelude::*;
use gtk4::{gio, Button, FileDialog, MenuButton, Popover, SearchBar, SearchEntry, ToggleButton};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::library::{self, db::Database, scanner};
use crate::player::{
    replaygain::{self, ReplayGainMode},
    Player,
};
use crate::scrobbler::LastFmClient;
use crate::ui::albums_view::AlbumsView;
use crate::ui::artists_view::ArtistsView;
use crate::ui::lastfm_dialog::show_lastfm_dialog;
use crate::ui::library_view::LibraryView;
use crate::ui::now_playing::NowPlaying;
use crate::ui::playback::{
    cover_cache, make_play_callback, start_player_timer, wire_cover_sync, wire_mpris,
    wire_transport_controls, CoverIndex, PlaybackCtx, ScrobbleTracker,
};
use crate::ui::player_bar::PlayerBar;
use crate::ui::reset::show_reset_dialog;
use crate::ui::theme::{set_tint_mode, setup_css, update_font, TintMode};

/// The library views plus the DB handle and the shared cover index — the
/// bundle of state the scan/reload paths always pass around together. Grouping
/// them keeps `reload_all_views`, `start_scan` and `show_reset_dialog` to a
/// short, readable parameter list instead of a half-dozen positional handles.
/// All fields are cheap `Rc`/`Arc` clones, so passing the struct by value is
/// just reference-count bumps.
#[derive(Clone)]
pub(crate) struct Views {
    pub db: Arc<Mutex<Database>>,
    pub lib: Rc<RefCell<LibraryView>>,
    pub albums: Rc<AlbumsView>,
    pub artists: Rc<ArtistsView>,
    pub cover_index: CoverIndex,
}

pub(crate) fn reload_all_views(views: &Views) {
    let (all, music_folder) = {
        let g = views.db.lock().unwrap();
        (
            g.all_tracks().unwrap_or_default(),
            g.get_setting("music_folder"),
        )
    };
    let albums = library::group_into_albums(&all, music_folder.as_deref());
    let artists = library::group_into_artists(&albums);
    // Index every track path to its album's canonical (artist, album) so the
    // player resolves covers under the same key the Albums view and the cover
    // store use — see `CoverIndex`.
    {
        let mut idx = views.cover_index.borrow_mut();
        idx.clear();
        for a in &albums {
            for t in &a.tracks {
                idx.insert(t.path.clone(), (a.artist.clone(), a.name.clone()));
            }
        }
    }
    views.lib.borrow_mut().load_tracks(all);
    views
        .albums
        .load_albums(albums.clone(), Arc::clone(&views.db));
    views.artists.load_artists(artists, albums);
}

pub(crate) fn start_scan(
    folder_path: String,
    views: Views,
    loading_box: gtk4::Box,
    spinner: gtk4::Spinner,
) {
    loading_box.set_visible(true);
    spinner.start();

    // Scan AND all DB writes happen on the worker thread so the UI never
    // freezes on a large library. The UI thread only refreshes the views
    // once the worker signals it is done.
    let scan_path = folder_path;
    let db_worker = Arc::clone(&views.db);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Incremental rescan: files whose stored mtime matches are not
        // re-read; only new/changed files pay the tag-parsing cost.
        let known_mtimes = db_worker.lock().unwrap().path_mtimes().unwrap_or_default();
        let result = scanner::scan_folder(&scan_path, &known_mtimes);
        {
            let db_g = db_worker.lock().unwrap();
            let _ = db_g.upsert_tracks(&result.tracks);
            let norm_folder: std::path::PathBuf =
                std::path::Path::new(&scan_path).components().collect();
            let _ = db_g.set_setting("music_folder", &norm_folder.to_string_lossy());
            let removed = db_g
                .remove_missing_from_folder(&scan_path, &result.found_paths)
                .unwrap_or(0);
            if removed > 0 {
                log::info!("sync: eliminados {} registros obsoletos", removed);
            }
        }
        let _ = tx.send(());
    });

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        use std::sync::mpsc::TryRecvError;
        match rx.try_recv() {
            Ok(()) => {
                reload_all_views(&views);
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

/// Present a modal error dialog and quit the application when it closes.
/// Used for failures during startup where the app cannot run at all (DB
/// inaccessible, audio engine missing).
pub fn show_fatal_error(app: &adw::Application, title: &str, detail: &str) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Audra")
        .default_width(420)
        .default_height(160)
        .build();
    window.present();

    let dialog = adw::AlertDialog::new(Some(title), Some(detail));
    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    let app_c = app.clone();
    dialog.connect_response(None, move |_, _| {
        app_c.quit();
    });
    dialog.present(Some(&window));
}

const APP_ICON_SVG: &[u8] =
    include_bytes!("../../data/icons/hicolor/scalable/apps/io.github.amurpo.audra.svg");

fn register_app_icon() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let theme = gtk4::IconTheme::for_display(&display);
    let icon_dir = std::env::temp_dir()
        .join("audra-icons")
        .join("hicolor")
        .join("scalable")
        .join("apps");
    if std::fs::create_dir_all(&icon_dir).is_ok() {
        let icon_path = icon_dir.join("io.github.amurpo.audra.svg");
        if std::fs::write(&icon_path, APP_ICON_SVG).is_ok() {
            theme.add_search_path(std::env::temp_dir().join("audra-icons"));
        }
    }
}

pub fn build_window(app: &adw::Application, db: Arc<Mutex<Database>>) {
    // On Windows, register the bundled share/icons directory so GTK can
    // resolve "io.github.amurpo.audra" from hicolor. Without this, GTK falls
    // back to its own default icon and overwrites the embedded .ico.
    #[cfg(windows)]
    if let Some(display) = gtk4::gdk::Display::default() {
        let theme = gtk4::IconTheme::for_display(&display);
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                theme.add_search_path(dir.join("share").join("icons"));
            }
        }
    }

    // Ensure the app icon is always resolvable for the About dialog.
    // The SVG is embedded in the binary; we write it once to a temp dir and
    // register that dir with the icon theme. This is a no-op on systems where
    // the icon is already installed (the theme finds it there first).
    register_app_icon();
    // Read every persisted setting the window needs in one lock, one pass.
    let (use_system_font, replaygain_setting, lang_setting, dyn_color_setting, saved_vol) = {
        let g = db.lock().unwrap();
        (
            g.get_setting("use_system_font").as_deref() == Some("1"),
            g.get_setting("replaygain").unwrap_or_default(),
            g.get_setting("language").unwrap_or_default(),
            g.get_setting("dynamic_color").unwrap_or_default(),
            g.get_setting("volume")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.5),
        )
    };
    let replaygain_init_mode = replaygain::mode_from_setting(&replaygain_setting);
    let dyn_color_init = TintMode::from_setting(&dyn_color_setting);
    set_tint_mode(dyn_color_init);
    setup_css(!use_system_font);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Audra")
        .default_width(1024)
        .default_height(680)
        .icon_name("io.github.amurpo.audra")
        .build();

    #[cfg(target_os = "windows")]
    window.set_decorated(false);

    // --- Player ---
    // Created before the settings popover because the ReplayGain and Language
    // rows capture it. No audio device (CI/headless, missing ALSA, etc.) is
    // recoverable enough to keep the rest of the app off the panic path: show
    // a modal and exit cleanly instead of aborting with a stack trace.
    let player: Rc<RefCell<Player>> = match Player::new() {
        Ok(p) => Rc::new(RefCell::new(p)),
        Err(e) => {
            show_fatal_error(
                app,
                &gettext("Audio output unavailable"),
                &format!(
                    "{}\n\n{}",
                    gettext("Audra could not initialise the audio engine."),
                    e
                ),
            );
            return;
        }
    };
    player.borrow_mut().replaygain_mode = replaygain_init_mode;

    // Switching language tears the window down and rebuilds it so every
    // gettext string is re-evaluated; playback is stopped first because the
    // rebuilt window creates a fresh Player.
    let apply_language: Rc<dyn Fn(Option<&'static str>)> = Rc::new({
        let player = Rc::clone(&player);
        let db = Arc::clone(&db);
        let app = app.clone();
        let window = window.downgrade();
        move |lang: Option<&'static str>| {
            player.borrow_mut().stop();
            let _ = db
                .lock()
                .unwrap()
                .set_setting("language", lang.unwrap_or(""));
            crate::i18n::init(lang);
            if let Some(w) = window.upgrade() {
                w.close();
            }
            build_window(&app, Arc::clone(&db));
        }
    });

    // --- Last.fm ---
    let lastfm: Arc<Mutex<Option<LastFmClient>>> = Arc::new(Mutex::new(None));
    {
        if LastFmClient::is_configured() {
            let db_g = db.lock().unwrap();
            if let Some(sk) = db_g
                .get_setting("lastfm_session_key")
                .filter(|s| !s.is_empty())
            {
                *lastfm.lock().unwrap() = Some(LastFmClient::new().with_session(&sk));
            }
        }
    }
    {
        let lf = Arc::clone(&lastfm);
        let db_flush = Arc::clone(&db);
        std::thread::spawn(move || {
            let sk = lf
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|c| c.session_key().map(str::to_string));
            if let Some(sk) = sk {
                LastFmClient::new().with_session(&sk).flush_queue(&db_flush);
            }
        });
    }

    // --- Header bar ---
    let header = adw::HeaderBar::new();
    header.add_css_class("audra-header-bar");

    let menu_btn = MenuButton::new();
    let menu_icon = crate::ui::icons::image(crate::ui::icons::Icon::FolderMusic, 20);
    menu_btn.set_child(Some(&menu_icon));
    menu_btn.set_tooltip_text(Some(&gettext("Library")));
    menu_btn.add_css_class("flat");

    let popover = Popover::new();
    popover.add_css_class("audra-shaded");
    let pop_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    pop_box.set_margin_top(4);
    pop_box.set_margin_bottom(4);
    pop_box.set_margin_start(4);
    pop_box.set_margin_end(4);
    // Fixed width so the popover does not resize when labels change length
    // across languages.
    pop_box.set_size_request(264, -1);

    let scan_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);

    let item_scan = Button::with_label(&gettext("Scan collection"));
    item_scan.add_css_class("flat");
    item_scan.set_hexpand(true);
    item_scan.set_halign(gtk4::Align::Fill);

    let item_refresh =
        crate::ui::icons::flat_icon_button(crate::ui::icons::Icon::Refresh, 20, None);
    item_refresh.add_css_class("flat");
    item_refresh.set_tooltip_text(Some(&gettext("Refresh collection")));

    scan_row.append(&item_scan);
    scan_row.append(&item_refresh);

    let pop_sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep.set_margin_top(4);
    pop_sep.set_margin_bottom(4);

    let item_lastfm = Button::with_label(&gettext("Last.fm Account"));
    item_lastfm.add_css_class("flat");
    item_lastfm.set_halign(gtk4::Align::Fill);

    let pop_sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep2.set_margin_top(4);
    pop_sep2.set_margin_bottom(4);

    let font_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    font_row.set_margin_top(2);
    font_row.set_margin_bottom(2);
    font_row.set_margin_start(8);
    font_row.set_margin_end(8);
    let font_label = gtk4::Label::new(Some(&gettext("System font")));
    font_label.set_hexpand(true);
    font_label.set_xalign(0.0);
    let font_switch = gtk4::Switch::new();
    font_switch.set_active(use_system_font);
    font_switch.set_valign(gtk4::Align::Center);
    font_row.append(&font_label);
    font_row.append(&font_switch);

    let rg_row = crate::ui::widgets::segmented_setting_row(
        &gettext("ReplayGain"),
        &[
            (gettext("Off"), None),
            (gettext("Track"), Some(ReplayGainMode::Track)),
            (gettext("Album"), Some(ReplayGainMode::Album)),
        ],
        replaygain_init_mode,
        {
            let db = Arc::clone(&db);
            let player = Rc::clone(&player);
            move |mode| {
                player.borrow_mut().replaygain_mode = mode;
                let _ = db
                    .lock()
                    .unwrap()
                    .set_setting("replaygain", replaygain::mode_as_setting(mode));
            }
        },
    );

    let dc_row = crate::ui::widgets::segmented_setting_row(
        &gettext("Dynamic color"),
        &[
            (gettext("Off"), TintMode::Off),
            (gettext("Partial"), TintMode::Partial),
            (gettext("Full"), TintMode::Full),
        ],
        dyn_color_init,
        {
            let db = Arc::clone(&db);
            move |mode: TintMode| {
                let _ = db
                    .lock()
                    .unwrap()
                    .set_setting("dynamic_color", mode.as_setting());
                set_tint_mode(mode);
            }
        },
    );

    let pop_sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep3.set_margin_top(14);
    pop_sep3.set_margin_bottom(3);

    let lang_row = crate::ui::widgets::segmented_setting_row(
        &gettext("Language"),
        &[
            ("Auto".to_string(), None),
            ("English".to_string(), Some("en")),
            ("Español".to_string(), Some("es")),
        ],
        match lang_setting.as_str() {
            "en" => Some("en"),
            "es" => Some("es"),
            _ => None,
        },
        {
            let apply_language = Rc::clone(&apply_language);
            move |lang| apply_language(lang)
        },
    );

    let item_reset = Button::new();
    item_reset.add_css_class("flat");
    item_reset.set_halign(gtk4::Align::Fill);
    item_reset.set_margin_top(3);
    let reset_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let reset_icon = crate::ui::icons::image(crate::ui::icons::Icon::DeleteBin, 16);
    reset_icon.add_css_class("menu-destructive");
    {
        let reset_icon = reset_icon.clone();
        reset_icon.connect_realize(move |img| {
            crate::ui::icons::set_image_icon(
                img,
                crate::ui::icons::Icon::DeleteBin,
                16,
                &crate::ui::icons::error_color(img),
            );
        });
    }
    let reset_lbl = gtk4::Label::new(Some(&gettext("Reset library…")));
    reset_lbl.add_css_class("menu-destructive");
    reset_box.append(&reset_icon);
    reset_box.append(&reset_lbl);
    item_reset.set_child(Some(&reset_box));

    let pop_sep4 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep4.set_margin_top(4);
    pop_sep4.set_margin_bottom(4);

    let item_about = Button::with_label(&gettext("About Audra"));
    item_about.add_css_class("flat");
    item_about.set_halign(gtk4::Align::Fill);

    pop_box.append(&scan_row);
    pop_box.append(&pop_sep);
    pop_box.append(&item_lastfm);
    pop_box.append(&pop_sep2);
    pop_box.append(&font_row);
    pop_box.append(&rg_row);
    pop_box.append(&dc_row);
    pop_box.append(&lang_row);
    pop_box.append(&pop_sep3);
    pop_box.append(&item_reset);
    pop_box.append(&pop_sep4);
    pop_box.append(&item_about);
    popover.set_child(Some(&pop_box));
    menu_btn.set_popover(Some(&popover));
    header.pack_start(&menu_btn);

    let btn_search = ToggleButton::new();
    let search_icon = crate::ui::icons::image(crate::ui::icons::Icon::Search, 20);
    btn_search.set_child(Some(&search_icon));
    btn_search.set_tooltip_text(Some(&gettext("Search")));
    btn_search.add_css_class("flat");
    header.pack_end(&btn_search);

    // --- Vistas y estado compartido ---
    // Single source of truth for "what's currently playing". All track lists
    // subscribe to this bus and update their `.playing` row indicator in sync.
    let now_playing = NowPlaying::new();

    let lib_view = Rc::new(RefCell::new(LibraryView::new(Rc::clone(&now_playing))));
    let albums_view = Rc::new(AlbumsView::new(Rc::clone(&now_playing)));
    let artists_view = Rc::new(ArtistsView::new(Arc::clone(&db), Rc::clone(&now_playing)));
    let bar = Rc::new(PlayerBar::new(Rc::clone(&now_playing)));

    // A track row's play/pause icon toggles playback through the exact same
    // button MPRIS and the keyboard use, so there's one play/pause code path.
    {
        let bar_c = Rc::clone(&bar);
        now_playing.set_toggle_handler(move || bar_c.btn_play_pause.emit_clicked());
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
                let sk = lf
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|c| c.session_key().map(str::to_string));
                if let Some(sk) = sk {
                    LastFmClient::new()
                        .with_session(&sk)
                        .update_now_playing(&artist, &title, &album);
                }
            });
        })
    };
    let highlight_track: crate::ui::playback::HighlightCb = {
        let np = Rc::clone(&now_playing);
        Rc::new(move |track: Option<&crate::library::Track>| {
            np.set(track.map(|t| t.path.clone()));
        })
    };

    // path → canonical (artist, album), shared with the player so cover
    // lookups at play time hit the same key the Albums view uses. Rebuilt by
    // every `reload_all_views` (initial load and rescans).
    let cover_index: CoverIndex = Rc::new(RefCell::new(HashMap::new()));

    // Bundle the views + DB + cover index so the scan/reload paths take one
    // handle instead of a long parameter list. Individual handles stay in
    // scope for the rest of the wiring below.
    let views = Views {
        db: Arc::clone(&db),
        lib: Rc::clone(&lib_view),
        albums: Rc::clone(&albums_view),
        artists: Rc::clone(&artists_view),
        cover_index: Rc::clone(&cover_index),
    };

    // --- Carga inicial ---
    reload_all_views(&views);

    // --- ViewStack ---
    let view_stack = adw::ViewStack::new();
    view_stack.add_titled(&albums_view.root, Some("albums"), &gettext("Albums"));
    view_stack.add_titled(&artists_view.root, Some("artists"), &gettext("Artists"));
    view_stack.add_titled(&lib_view.borrow().root, Some("tracks"), &gettext("Songs"));
    view_stack.set_visible_child_name("albums");

    let view_switcher = crate::ui::widgets::view_switcher_bar(
        &view_stack,
        &[
            crate::ui::widgets::ViewTab {
                stack_name: "albums",
                icon: crate::ui::icons::Icon::Album,
                label: gettext("Albums"),
            },
            crate::ui::widgets::ViewTab {
                stack_name: "artists",
                icon: crate::ui::icons::Icon::Group,
                label: gettext("Artists"),
            },
            crate::ui::widgets::ViewTab {
                stack_name: "tracks",
                icon: crate::ui::icons::Icon::ListUnordered,
                label: gettext("Songs"),
            },
        ],
    );
    header.set_title_widget(Some(&view_switcher));

    // --- Barra de búsqueda ---
    let search_entry = SearchEntry::new();
    search_entry.set_placeholder_text(Some(&gettext("Search by title, artist or album…")));
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
    // A small outer breathing margin around the main content area so nothing
    // (Play-all button, artist avatars, grid cards) hugs the window edge. The
    // header bar and the player bar stay flush with the edges by design.
    view_stack.set_margin_start(12);
    view_stack.set_margin_end(12);
    view_stack.set_margin_top(6);
    view_stack.set_margin_bottom(6);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_css_class("audra-toolbar");
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
    let scan_title_lbl = gtk4::Label::new(Some(&gettext("Scanning collection…")));
    scan_title_lbl.add_css_class("title-2");
    let scan_sub_lbl = gtk4::Label::new(Some(&gettext("This may take a moment")));
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
        #[strong]
        window,
        #[strong]
        views,
        #[weak]
        popover,
        #[weak]
        scan_loading_box,
        #[weak]
        scan_spinner,
        move |_| {
            popover.popdown();
            let dialog = FileDialog::new();
            dialog.select_folder(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[strong]
                    views,
                    #[weak]
                    scan_loading_box,
                    #[weak]
                    scan_spinner,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                start_scan(
                                    path.to_string_lossy().to_string(),
                                    views.clone(),
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

    item_refresh.connect_clicked(clone!(
        #[strong]
        views,
        #[weak]
        popover,
        #[weak]
        scan_loading_box,
        #[weak]
        scan_spinner,
        move |_| {
            popover.popdown();
            if let Some(folder) = views.db.lock().unwrap().get_setting("music_folder") {
                start_scan(folder, views.clone(), scan_loading_box, scan_spinner);
            }
        }
    ));

    font_switch.connect_state_set(clone!(
        #[strong]
        db,
        move |_, state| {
            let _ = db
                .lock()
                .unwrap()
                .set_setting("use_system_font", if state { "1" } else { "0" });
            update_font(!state);
            glib::Propagation::Proceed
        }
    ));

    item_lastfm.connect_clicked(clone!(
        #[strong]
        window,
        #[strong]
        db,
        #[strong]
        lastfm,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            show_lastfm_dialog(&window, Arc::clone(&db), Arc::clone(&lastfm));
        }
    ));

    item_reset.connect_clicked(clone!(
        #[strong]
        window,
        #[strong]
        views,
        #[strong]
        scan_loading_box,
        #[strong]
        scan_spinner,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            show_reset_dialog(
                &window,
                views.clone(),
                scan_loading_box.clone(),
                scan_spinner.clone(),
            );
        }
    ));

    item_about.connect_clicked(clone!(
        #[strong]
        window,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            let about = adw::AboutDialog::builder()
                .application_name("Audra")
                .application_icon("io.github.amurpo.audra")
                .developer_name("Daniel Avila")
                .version(env!("CARGO_PKG_VERSION"))
                .comments(gettext("Native music player with Last.fm scrobbling"))
                .copyright("© Daniel Avila")
                .license_type(gtk4::License::Gpl30)
                .website("https://github.com/amurpo/audra")
                .issue_url("https://github.com/amurpo/audra/issues")
                .translator_credits(gettext("translator-credits"))
                .build();
            about.add_credit_section(
                Some("Remix Icon"),
                &["https://remixicon.com — Remix Icon License v1.0"],
            );
            about.add_css_class("audra-shaded");
            about.present(Some(&window));
        }
    ));

    // One shared playback context and ONE play callback instance behind an
    // `Rc`, handed to every view — instead of building four identical
    // closures with six captured handles each.
    let ctx = PlaybackCtx {
        player: Rc::clone(&player),
        bar: Rc::clone(&bar),
        db: Arc::clone(&db),
        notify_now_playing: Rc::clone(&notify_now_playing),
        highlight: Rc::clone(&highlight_track),
        cover_index: Rc::clone(&cover_index),
        cover_cache: cover_cache(),
    };
    let play_cb: Rc<dyn Fn(Vec<crate::library::Track>, usize)> =
        Rc::new(make_play_callback(ctx.clone()));

    albums_view.set_on_play({
        let cb = Rc::clone(&play_cb);
        move |tracks, idx| cb(tracks, idx)
    });
    artists_view.set_on_play({
        let cb = Rc::clone(&play_cb);
        move |tracks, idx| cb(tracks, idx)
    });
    lib_view.borrow().set_on_play_all({
        let cb = Rc::clone(&play_cb);
        move |tracks, idx| cb(tracks, idx)
    });
    lib_view.borrow().set_on_activate({
        let cb = Rc::clone(&play_cb);
        move |tracks, idx| cb(tracks, idx)
    });

    wire_transport_controls(&ctx);

    // Apply saved volume (explicitly set player + label before triggering the scale signal)
    player.borrow_mut().set_volume(saved_vol as f32);
    bar.lbl_volume
        .set_text(&format!("{:.0}%", saved_vol * 100.0));
    bar.vol_scale.set_value(saved_vol);

    // Persist volume changes to DB
    bar.vol_scale.connect_value_changed(clone!(
        #[strong]
        db,
        move |scale| {
            let _ = db
                .lock()
                .unwrap()
                .set_setting("volume", &scale.value().to_string());
        }
    ));

    let (mpris_tx, mpris_rx) = std::sync::mpsc::channel();
    let mpris_cell: crate::ui::playback::MprisHandle =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    // Live cover sync: a cover change from the picker repaints the bar, tint
    // and OS controls immediately when it hits the album that's playing.
    wire_cover_sync(&ctx, Rc::clone(&mpris_cell));

    // Windows SMTC: wire the command-drain timer unconditionally so it is
    // ready before the first event arrives. Sender<T>: Clone lets the
    // connect_map handler below supply a fresh tx for each attempt without
    // rebinding the receiver.
    #[cfg(windows)]
    {
        let mpris_cell = Rc::clone(&mpris_cell);
        let bar_c = Rc::clone(&bar);
        let player_c = Rc::clone(&player);

        wire_mpris(
            mpris_rx,
            Rc::clone(&player_c),
            Rc::clone(&bar_c),
            window.downgrade(),
        );

        // connect_map fires when the window becomes visible on screen.
        // Defer the souvlaki call by 300 ms: GetForWindow() can silently
        // fail even with a valid HWND if the Win32 message pump has not
        // yet finished processing its initial queue of window messages.
        // If the window is hidden and reshown, connect_map fires again
        // and retries (the is_some() guard prevents double-init).
        window.connect_map(move |window| {
            if mpris_cell.borrow().is_some() {
                return;
            }
            let win_weak = window.downgrade();
            let cell = Rc::clone(&mpris_cell);
            let tx = mpris_tx.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                if cell.borrow().is_some() {
                    return;
                }
                let Some(win) = win_weak.upgrade() else {
                    return;
                };
                if let Some(m) = crate::player::mpris::Mpris::new(&win, tx) {
                    *cell.borrow_mut() = Some(m);
                } else {
                    log::warn!("mpris/smtc: media controls unavailable");
                }
            });
        });
    }

    window.present();

    // Linux/other: D-Bus MPRIS does not need an HWND; one idle tick is
    // enough to let the surface map before calling Mpris::new.
    #[cfg(not(windows))]
    {
        let mpris_cell = Rc::clone(&mpris_cell);
        let bar_c = Rc::clone(&bar);
        let player_c = Rc::clone(&player);
        let window_weak = window.downgrade();
        glib::idle_add_local_once(move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            if let Some(m) = crate::player::mpris::Mpris::new(&window, mpris_tx) {
                *mpris_cell.borrow_mut() = Some(m);
                wire_mpris(mpris_rx, player_c, bar_c, window.downgrade());
            } else {
                log::warn!("mpris/smtc: media controls unavailable on this platform");
            }
        });
    }

    start_player_timer(
        ctx,
        Rc::clone(&scrobble_tracker),
        Arc::clone(&lastfm),
        window.downgrade(),
        mpris_cell,
    );
}
