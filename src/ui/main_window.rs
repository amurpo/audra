use gtk4::prelude::*;
use gtk4::{Button, FileDialog, SearchBar, SearchEntry, ToggleButton, gio};
use libadwaita as adw;
use adw::prelude::*;
use glib::clone;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::rc::Rc;

use crate::library::{self, db::Database};
use crate::library::{art, scanner};
use crate::player::{Player, PlayerState};
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
";

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

    // --- Header bar ---
    let header = adw::HeaderBar::new();

    let btn_add = Button::from_icon_name("folder-music-symbolic");
    btn_add.set_tooltip_text(Some("Agregar carpeta de música"));
    btn_add.add_css_class("flat");
    header.pack_start(&btn_add);

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

    // --- Player y vistas ---
    let player: Rc<RefCell<Player>> = Rc::new(RefCell::new(
        Player::new().expect("Error iniciando el motor de audio"),
    ));

    let lib_view = Rc::new(RefCell::new(LibraryView::new()));
    let albums_view = Rc::new(AlbumsView::new());
    let artists_view = Rc::new(ArtistsView::new());
    let bar = Rc::new(PlayerBar::new());

    // Cargar datos iniciales
    let tracks = db.lock().unwrap().all_tracks().unwrap_or_default();
    {
        lib_view.borrow_mut().load_tracks(tracks.clone());
        let albums = library::group_into_albums(&tracks);
        let artists = library::group_into_artists(&albums);
        let lastfm_key = std::env::var("LASTFM_API_KEY").ok();
        albums_view.load_albums(albums);
        artists_view.load_artists(artists, lastfm_key);
    }

    // --- Páginas del ViewStack ---
    {
        let page = view_stack.add_titled(
            &albums_view.root,
            Some("albums"),
            "Álbumes",
        );
        page.set_icon_name(Some("media-optical-symbolic"));
    }
    {
        let page = view_stack.add_titled(
            &artists_view.root,
            Some("artists"),
            "Artistas",
        );
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

    // --- Búsqueda en tiempo real (aplica a la vista de Canciones) ---
    {
        let lib_view = Rc::clone(&lib_view);
        search_entry.connect_search_changed(move |entry| {
            lib_view.borrow_mut().filter(&entry.text());
        });
    }

    // --- Agregar carpeta de música ---
    btn_add.connect_clicked(clone!(
        #[strong] window,
        #[strong] db,
        #[strong] lib_view,
        #[strong] albums_view,
        #[strong] artists_view,
        move |_| {
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
                                let lastfm_key = std::env::var("LASTFM_API_KEY").ok();
                                albums_view.load_albums(albums);
                                artists_view.load_artists(artists, lastfm_key);
                            }
                        }
                    }
                ),
            );
        }
    ));

    // --- Click en álbum: reproducir todo el álbum ---
    {
        let albums_view_ref = Rc::clone(&albums_view);
        let player = Rc::clone(&player);
        let bar_ref = Rc::clone(&bar);
        albums_view.flow.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            let tracks = albums_view_ref.get_album_tracks(idx);
            if !tracks.is_empty() {
                let mut p = player.borrow_mut();
                p.load_queue(tracks, 0);
                if let Ok(Some(track)) = p.play_current() {
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
        lib_view.borrow().list_view.connect_activate(move |_, idx| {
            let lv = lib_view_ref.borrow();
            let tracks = lv.all_tracks();
            drop(lv);
            let mut p = player.borrow_mut();
            p.load_queue(tracks, idx as usize);
            if let Ok(Some(track)) = p.play_current() {
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
        bar.btn_next.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.next() {
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
        bar.btn_prev.connect_clicked(move |_| {
            let mut p = player.borrow_mut();
            if let Ok(Some(track)) = p.previous() {
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

    // --- Timer: auto-avance + progreso ---
    {
        let player = Rc::clone(&player);
        let bar = Rc::clone(&bar);
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
                }
            }
            glib::ControlFlow::Continue
        });
    }

    window.present();
}
