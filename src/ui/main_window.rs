use adw::prelude::*;
use glib::clone;
use gtk4::prelude::*;
use gtk4::{gio, Button, FileDialog, MenuButton, Popover, SearchBar, SearchEntry, ToggleButton};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::library::{self, db::Database, scanner};
use crate::player::Player;
use crate::scrobbler::LastFmClient;
use crate::ui::albums_view::AlbumsView;
use crate::ui::artists_view::ArtistsView;
use crate::ui::lastfm_dialog::show_lastfm_dialog;
use crate::ui::library_view::LibraryView;
use crate::ui::playback::{
    make_play_callback, start_player_timer, wire_transport_controls, ScrobbleTracker,
};
use crate::ui::player_bar::PlayerBar;
use crate::ui::reset::show_reset_dialog;
use crate::ui::theme::{setup_css, update_font};

pub(crate) fn reload_all_views(
    db: &Arc<Mutex<Database>>,
    lib_view: &Rc<RefCell<LibraryView>>,
    albums_view: &Rc<AlbumsView>,
    artists_view: &Rc<ArtistsView>,
) {
    let all = db.lock().unwrap().all_tracks().unwrap_or_default();
    let albums = library::group_into_albums(&all);
    let artists = library::group_into_artists(&albums);
    lib_view.borrow_mut().load_tracks(all);
    albums_view.load_albums(albums.clone(), Arc::clone(db));
    artists_view.load_artists(artists, albums);
}

pub(crate) fn start_scan(
    folder_path: String,
    db: Arc<Mutex<Database>>,
    lib_view: Rc<RefCell<LibraryView>>,
    albums_view: Rc<AlbumsView>,
    artists_view: Rc<ArtistsView>,
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
                    let norm_folder: std::path::PathBuf =
                        std::path::Path::new(&folder_path).components().collect();
                    let _ = db_g.set_setting("music_folder", &norm_folder.to_string_lossy());
                    let found: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
                    let removed = db_g
                        .remove_missing_from_folder(&folder_path, &found)
                        .unwrap_or(0);
                    if removed > 0 {
                        log::info!("sync: eliminados {} registros obsoletos", removed);
                    }
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

pub fn build_window(app: &adw::Application, db: Arc<Mutex<Database>>) {
    let use_system_font = db.lock().unwrap().get_setting("use_system_font").as_deref() == Some("1");
    let lang_setting = db
        .lock()
        .unwrap()
        .get_setting("language")
        .unwrap_or_default();
    let saved_vol: f64 = db
        .lock()
        .unwrap()
        .get_setting("volume")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.5);
    setup_css(!use_system_font);

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

    let menu_btn = MenuButton::new();
    menu_btn.set_icon_name("folder-music-symbolic");
    menu_btn.set_tooltip_text(Some(&gettext("Library")));
    menu_btn.add_css_class("flat");

    let popover = Popover::new();
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

    let item_refresh = Button::new();
    item_refresh.set_icon_name("view-refresh-symbolic");
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

    let pop_sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep3.set_margin_top(14);
    pop_sep3.set_margin_bottom(3);

    let lang_row = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    lang_row.set_margin_top(4);
    lang_row.set_margin_bottom(4);
    lang_row.set_margin_start(8);
    lang_row.set_margin_end(8);
    let lang_label = gtk4::Label::new(Some(&gettext("Language")));
    lang_label.set_xalign(0.0);
    lang_label.add_css_class("caption");
    lang_label.add_css_class("dim-label");

    let lang_btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    lang_btn_box.add_css_class("linked");

    let btn_lang_auto = ToggleButton::with_label("Auto");
    let btn_lang_en = ToggleButton::with_label("English");
    let btn_lang_es = ToggleButton::with_label("Español");
    btn_lang_en.set_group(Some(&btn_lang_auto));
    btn_lang_es.set_group(Some(&btn_lang_auto));

    // Set initial state before connecting signals to avoid spurious rebuilds
    match lang_setting.as_str() {
        "en" => btn_lang_en.set_active(true),
        "es" => btn_lang_es.set_active(true),
        _ => btn_lang_auto.set_active(true),
    }

    lang_btn_box.append(&btn_lang_auto);
    lang_btn_box.append(&btn_lang_en);
    lang_btn_box.append(&btn_lang_es);
    lang_row.append(&lang_label);
    lang_row.append(&lang_btn_box);

    let item_reset = Button::new();
    item_reset.add_css_class("flat");
    item_reset.set_halign(gtk4::Align::Fill);
    item_reset.set_margin_top(3);
    let reset_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let reset_icon = gtk4::Image::from_icon_name("user-trash-symbolic");
    reset_icon.add_css_class("menu-destructive");
    let reset_lbl = gtk4::Label::new(Some(&gettext("Reset library…")));
    reset_lbl.add_css_class("menu-destructive");
    reset_box.append(&reset_icon);
    reset_box.append(&reset_lbl);
    item_reset.set_child(Some(&reset_box));

    pop_box.append(&scan_row);
    pop_box.append(&pop_sep);
    pop_box.append(&item_lastfm);
    pop_box.append(&pop_sep2);
    pop_box.append(&font_row);
    pop_box.append(&lang_row);
    pop_box.append(&pop_sep3);
    pop_box.append(&item_reset);
    popover.set_child(Some(&pop_box));
    menu_btn.set_popover(Some(&popover));
    header.pack_start(&menu_btn);

    let btn_search = ToggleButton::new();
    btn_search.set_icon_name("system-search-symbolic");
    btn_search.set_tooltip_text(Some(&gettext("Search")));
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
        let page = view_stack.add_titled(&albums_view.root, Some("albums"), &gettext("Albums"));
        page.set_icon_name(Some("media-optical-symbolic"));
    }
    {
        let page = view_stack.add_titled(&artists_view.root, Some("artists"), &gettext("Artists"));
        page.set_icon_name(Some("system-users-symbolic"));
    }
    {
        let page =
            view_stack.add_titled(&lib_view.borrow().root, Some("tracks"), &gettext("Songs"));
        page.set_icon_name(Some("view-list-symbolic"));
    }
    view_stack.set_visible_child_name("albums");

    let view_switcher = adw::ViewSwitcher::new();
    view_switcher.set_stack(Some(&view_stack));
    view_switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
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
        db,
        #[strong]
        lib_view,
        #[strong]
        albums_view,
        #[strong]
        artists_view,
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
                    db,
                    #[strong]
                    lib_view,
                    #[strong]
                    albums_view,
                    #[strong]
                    artists_view,
                    #[weak]
                    scan_loading_box,
                    #[weak]
                    scan_spinner,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                start_scan(
                                    path.to_string_lossy().to_string(),
                                    Arc::clone(&db),
                                    Rc::clone(&lib_view),
                                    Rc::clone(&albums_view),
                                    Rc::clone(&artists_view),
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
        db,
        #[strong]
        lib_view,
        #[strong]
        albums_view,
        #[strong]
        artists_view,
        #[weak]
        popover,
        #[weak]
        scan_loading_box,
        #[weak]
        scan_spinner,
        move |_| {
            popover.popdown();
            if let Some(folder) = db.lock().unwrap().get_setting("music_folder") {
                start_scan(
                    folder,
                    Arc::clone(&db),
                    Rc::clone(&lib_view),
                    Rc::clone(&albums_view),
                    Rc::clone(&artists_view),
                    scan_loading_box,
                    scan_spinner,
                );
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

    btn_lang_auto.connect_toggled(clone!(
        #[strong]
        db,
        #[strong]
        app,
        #[weak]
        window,
        move |btn| {
            if !btn.is_active() {
                return;
            }
            let _ = db.lock().unwrap().set_setting("language", "");
            crate::i18n::init(None);
            window.close();
            build_window(&app, Arc::clone(&db));
        }
    ));
    btn_lang_en.connect_toggled(clone!(
        #[strong]
        db,
        #[strong]
        app,
        #[weak]
        window,
        move |btn| {
            if !btn.is_active() {
                return;
            }
            let _ = db.lock().unwrap().set_setting("language", "en");
            crate::i18n::init(Some("en"));
            window.close();
            build_window(&app, Arc::clone(&db));
        }
    ));
    btn_lang_es.connect_toggled(clone!(
        #[strong]
        db,
        #[strong]
        app,
        #[weak]
        window,
        move |btn| {
            if !btn.is_active() {
                return;
            }
            let _ = db.lock().unwrap().set_setting("language", "es");
            crate::i18n::init(Some("es"));
            window.close();
            build_window(&app, Arc::clone(&db));
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
        db,
        #[strong]
        lib_view,
        #[strong]
        albums_view,
        #[strong]
        artists_view,
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
                Arc::clone(&db),
                Rc::clone(&lib_view),
                Rc::clone(&albums_view),
                Rc::clone(&artists_view),
                scan_loading_box.clone(),
                scan_spinner.clone(),
            );
        }
    ));

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
    lib_view.borrow().set_on_play_all(make_play_callback(
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

    start_player_timer(
        Rc::clone(&player),
        Rc::clone(&bar),
        Rc::clone(&scrobble_tracker),
        Arc::clone(&lastfm),
        Arc::clone(&db),
        Rc::clone(&notify_now_playing),
        Rc::clone(&highlight_track),
        window.downgrade(),
    );

    window.present();
}
