use crate::i18n::gettext;
use crate::library::db::Database;
use crate::library::{Album, Track};
use crate::ui::image_utils::{pixels_to_texture, scale_to_pixels};
use adw::prelude::*;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, ContentFit, FlowBox, FlowBoxChild, Label, ListBox, ListBoxRow,
    Orientation, Overlay, Picture, ScrolledWindow, SelectionMode, Stack, StackTransitionType,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const CARD_SIZE: i32 = 200;

type CoverMap = Rc<RefCell<HashMap<String, (Stack, Picture)>>>;
type ScaledCover = (String, Vec<u8>, i32, bool);
type PlayCb = Rc<RefCell<Option<Box<dyn Fn(Vec<Track>, usize)>>>>;

pub struct AlbumsView {
    pub root: adw::NavigationView,
    flow: FlowBox,
    albums_data: Rc<RefCell<Vec<Album>>>,
    covers: CoverMap,
    on_play: PlayCb,
    current_filter: Rc<RefCell<String>>,
}

impl AlbumsView {
    pub fn new(current_path: Rc<RefCell<Option<String>>>) -> Self {
        let flow = FlowBox::new();
        flow.set_selection_mode(SelectionMode::Single);
        flow.set_homogeneous(true);
        flow.set_column_spacing(8);
        flow.set_row_spacing(8);
        flow.set_margin_top(12);
        flow.set_margin_bottom(12);
        flow.set_margin_start(12);
        flow.set_margin_end(12);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(12);
        flow.set_activate_on_single_click(true);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&flow));

        let nav = adw::NavigationView::new();
        let root_page = adw::NavigationPage::new(&scroll, &gettext("Albums"));
        root_page.set_tag(Some("albums-root"));
        nav.add(&root_page);

        let albums_data: Rc<RefCell<Vec<Album>>> = Rc::new(RefCell::new(Vec::new()));
        let covers: CoverMap = Rc::new(RefCell::new(HashMap::new()));
        let on_play: PlayCb = Rc::new(RefCell::new(None));

        {
            let nav_c = nav.clone();
            let albums_c = Rc::clone(&albums_data);
            let on_play_c = Rc::clone(&on_play);
            let current_path_c = Rc::clone(&current_path);
            flow.connect_child_activated(move |_, child| {
                let idx = child.index() as usize;
                let album = albums_c.borrow().get(idx).cloned();
                if let Some(album) = album {
                    let page = make_album_detail_page(
                        &album,
                        Rc::clone(&on_play_c),
                        Rc::clone(&current_path_c),
                    );
                    nav_c.push(&page);
                }
            });
        }

        Self {
            root: nav,
            flow,
            albums_data,
            covers,
            on_play,
            current_filter: Rc::new(RefCell::new(String::new())),
        }
    }

    pub fn set_on_play(&self, callback: impl Fn(Vec<Track>, usize) + 'static) {
        *self.on_play.borrow_mut() = Some(Box::new(callback));
    }

    pub fn load_albums(&self, albums: Vec<Album>, db: Arc<Mutex<Database>>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.covers.borrow_mut().clear();

        let mut need_fetch: Vec<(String, String, String)> = Vec::new();

        for album in &albums {
            let key = format!("{}|{}", album.artist, album.name);
            let (card, stack, picture) = make_album_card(album, true);
            self.covers
                .borrow_mut()
                .insert(key.clone(), (stack, picture));
            self.flow.append(&card);

            let track_path = album
                .tracks
                .first()
                .map(|t| t.path.clone())
                .unwrap_or_default();
            need_fetch.push((album.artist.clone(), album.name.clone(), track_path));
        }

        *self.albums_data.borrow_mut() = albums;

        let active = self.current_filter.borrow().clone();
        if !active.is_empty() {
            self.filter(&active);
        }

        if !need_fetch.is_empty() {
            self.start_cover_fetch(need_fetch, db);
        }
    }

    pub fn filter(&self, query: &str) {
        *self.current_filter.borrow_mut() = query.to_string();
        if query.is_empty() {
            self.flow.set_filter_func(|_| true);
        } else {
            let q = query.to_lowercase();
            let albums = Rc::clone(&self.albums_data);
            self.flow.set_filter_func(move |child| {
                let idx = child.index() as usize;
                if let Some(album) = albums.borrow().get(idx) {
                    album.name.to_lowercase().contains(&q)
                        || album.artist.to_lowercase().contains(&q)
                } else {
                    false
                }
            });
        }
    }

    fn start_cover_fetch(&self, albums: Vec<(String, String, String)>, db: Arc<Mutex<Database>>) {
        use std::sync::atomic::{AtomicBool, Ordering};

        let queue: Arc<Mutex<Vec<ScaledCover>>> = Arc::new(Mutex::new(Vec::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let queue_tx = Arc::clone(&queue);
        let finished_tx = Arc::clone(&finished);

        std::thread::spawn(move || {
            let mut need_lastfm: Vec<(String, String)> = Vec::new();

            for (artist, album_name, track_path) in &albums {
                let key = format!("{}|{}", artist, album_name);

                if let Some(bytes) = db.lock().unwrap().get_cover(artist, album_name) {
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        queue_tx
                            .lock()
                            .unwrap()
                            .push((key, scaled.0, scaled.1, scaled.2));
                        continue;
                    }
                }

                if let Some(bytes) = crate::library::art::read_cover_art(track_path) {
                    let _ = db.lock().unwrap().set_cover(artist, album_name, &bytes);
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        queue_tx
                            .lock()
                            .unwrap()
                            .push((key, scaled.0, scaled.1, scaled.2));
                        continue;
                    }
                }

                need_lastfm.push((artist.clone(), album_name.clone()));
            }

            for (artist, album_name) in need_lastfm {
                std::thread::sleep(std::time::Duration::from_millis(1100));
                if let Some(bytes) =
                    crate::library::metadata::fetch_album_cover(&artist, &album_name)
                {
                    let _ = db.lock().unwrap().set_cover(&artist, &album_name, &bytes);
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        let key = format!("{}|{}", artist, album_name);
                        queue_tx
                            .lock()
                            .unwrap()
                            .push((key, scaled.0, scaled.1, scaled.2));
                    }
                }
            }

            finished_tx.store(true, Ordering::Relaxed);
        });

        let covers = Rc::clone(&self.covers);
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let mut q = queue.lock().unwrap();
            for (key, pixels, rowstride, has_alpha) in q.drain(..) {
                if let Some((stack, picture)) = covers.borrow().get(&key) {
                    let texture = pixels_to_texture(pixels, rowstride, has_alpha, CARD_SIZE);
                    picture.set_paintable(Some(&texture));
                    stack.set_visible_child_name("art");
                }
            }
            drop(q);
            if finished.load(std::sync::atomic::Ordering::Relaxed) {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

pub fn make_album_detail_page(
    album: &Album,
    on_play: PlayCb,
    current_path: Rc<RefCell<Option<String>>>,
) -> adw::NavigationPage {
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    header.set_show_start_title_buttons(false);

    let btn_play_all = Button::builder()
        .label(gettext("Play all"))
        .css_classes(["suggested-action", "pill"])
        .build();
    header.pack_end(&btn_play_all);

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_margin_top(8);
    list.set_margin_bottom(12);
    list.set_margin_start(12);
    list.set_margin_end(12);

    let tracks = Rc::new(album.tracks.clone());

    for (i, track) in tracks.iter().enumerate() {
        list.append(&make_track_row(i, track));
    }

    {
        let tracks_c = Rc::clone(&tracks);
        let on_play_c = Rc::clone(&on_play);
        btn_play_all.connect_clicked(move |_| {
            if let Some(cb) = on_play_c.borrow().as_ref() {
                cb((*tracks_c).clone(), usize::MAX);
            }
        });
    }

    {
        let tracks_c = Rc::clone(&tracks);
        list.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            if let Some(cb) = on_play.borrow().as_ref() {
                cb((*tracks_c).clone(), idx);
            }
        });
    }

    // Timer que actualiza el highlight de la pista en reproducción
    {
        let list_weak = list.downgrade();
        let tracks_ref = Rc::clone(&tracks);
        let mut last_path: Option<String> = None;
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let Some(list) = list_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let current = current_path.borrow().clone();
            if current == last_path {
                return glib::ControlFlow::Continue;
            }
            last_path = current.clone();
            let mut i = 0i32;
            while let Some(row) = list.row_at_index(i) {
                let is_playing = current
                    .as_deref()
                    .and_then(|p| tracks_ref.get(i as usize).map(|t| t.path == p))
                    .unwrap_or(false);
                update_track_row_highlight(&row, i as usize, is_playing);
                i += 1;
            }
            glib::ControlFlow::Continue
        });
    }

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));

    adw::NavigationPage::new(&toolbar, &album.name)
}

fn update_track_row_highlight(row: &ListBoxRow, idx: usize, is_playing: bool) {
    let Some(hbox) = row.child().and_downcast::<GtkBox>() else {
        return;
    };
    let Some(num_lbl) = hbox.first_child().and_downcast::<Label>() else {
        return;
    };
    let title_lbl = num_lbl.next_sibling().and_downcast::<Label>();

    if is_playing {
        num_lbl.set_text("▶");
        num_lbl.add_css_class("now-playing-title");
        if let Some(t) = &title_lbl {
            t.add_css_class("now-playing-title");
        }
    } else {
        num_lbl.set_text(&(idx + 1).to_string());
        num_lbl.remove_css_class("now-playing-title");
        if let Some(t) = &title_lbl {
            t.remove_css_class("now-playing-title");
        }
    }
}

fn make_track_row(idx: usize, track: &Track) -> ListBoxRow {
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let num_label = Label::new(Some(&(idx + 1).to_string()));
    num_label.add_css_class("dim-label");
    num_label.set_width_chars(3);
    num_label.set_xalign(1.0);
    hbox.append(&num_label);

    let title_label = Label::new(Some(&track.display_title()));
    title_label.set_hexpand(true);
    title_label.set_xalign(0.0);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    hbox.append(&title_label);

    let dur_label = Label::new(Some(&track.duration_str()));
    dur_label.add_css_class("dim-label");
    dur_label.add_css_class("caption");
    hbox.append(&dur_label);

    let row = ListBoxRow::new();
    row.set_child(Some(&hbox));
    row
}

pub fn make_album_card(album: &Album, show_artist: bool) -> (FlowBoxChild, Stack, Picture) {
    let overlay = Overlay::new();
    overlay.set_size_request(CARD_SIZE, CARD_SIZE);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_hexpand(false);
    overlay.set_vexpand(false);

    let cover_stack = Stack::new();
    cover_stack.set_halign(Align::Fill);
    cover_stack.set_valign(Align::Fill);
    cover_stack.set_overflow(gtk4::Overflow::Hidden);
    cover_stack.set_transition_type(StackTransitionType::Crossfade);
    cover_stack.set_transition_duration(150);

    let cover_picture = Picture::new();
    cover_picture.set_content_fit(ContentFit::Cover);
    cover_picture.set_can_shrink(true);
    cover_picture.set_halign(Align::Fill);
    cover_picture.set_valign(Align::Fill);
    cover_stack.add_named(&cover_picture, Some("art"));

    let placeholder = GtkBox::new(Orientation::Vertical, 0);
    placeholder.set_halign(Align::Fill);
    placeholder.set_valign(Align::Fill);
    placeholder.set_hexpand(true);
    placeholder.set_vexpand(true);
    let note_lbl = Label::new(Some("♪"));
    note_lbl.add_css_class("album-cover-note");
    note_lbl.add_css_class("dim-label");
    note_lbl.set_halign(Align::Center);
    note_lbl.set_valign(Align::Center);
    note_lbl.set_vexpand(true);
    placeholder.append(&note_lbl);
    cover_stack.add_named(&placeholder, Some("placeholder"));

    cover_stack.set_visible_child_name("placeholder");

    overlay.set_child(Some(&cover_stack));

    let info = GtkBox::new(Orientation::Vertical, 1);
    info.set_valign(Align::End);
    info.set_halign(Align::Fill);
    info.add_css_class("album-overlay-box");

    let lbl_name = Label::new(Some(&album.name));
    lbl_name.add_css_class("album-overlay-title");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_xalign(0.0);

    info.append(&lbl_name);
    if show_artist {
        let lbl_artist = Label::new(Some(&album.artist));
        lbl_artist.add_css_class("album-overlay-artist");
        lbl_artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        lbl_artist.set_xalign(0.0);
        info.append(&lbl_artist);
    }
    overlay.add_overlay(&info);

    let child = FlowBoxChild::new();
    child.add_css_class("mosaic-child");
    child.set_child(Some(&overlay));
    child.set_halign(Align::Center);
    child.set_valign(Align::Center);

    (child, cover_stack, cover_picture)
}
