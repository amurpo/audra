use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, FlowBox, FlowBoxChild, Label, Orientation,
    ScrolledWindow, Align, SelectionMode,
};
use libadwaita as adw;
use adw::prelude::*;
use glib;
use crate::ui::image_utils::{scale_to_pixels, pixels_to_texture};
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::i18n::{gettext, ngettext};
use crate::library::{Artist, Album, Track};
use crate::library::db::Database;
use crate::ui::albums_view::{make_album_card, make_album_detail_page};

const CARD_SIZE: i32 = 200;
const AVATAR_SIZE: i32 = 120;

type PlayCallback = Box<dyn Fn(Vec<Track>, usize)>;
type AvatarMap = Rc<RefCell<HashMap<String, adw::Avatar>>>;
type ScaledPhoto = (String, Vec<u8>, i32, bool);

pub struct ArtistsView {
    pub root: adw::NavigationView,
    flow: FlowBox,
    artists_list: Rc<RefCell<Vec<Artist>>>,
    all_albums: Rc<RefCell<Vec<Album>>>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
    avatars: AvatarMap,
    current_filter: Rc<RefCell<String>>,
}

impl ArtistsView {
    pub fn new(db: Arc<Mutex<Database>>, current_path: Rc<RefCell<Option<String>>>) -> Self {
        let nav = adw::NavigationView::new();

        let flow = FlowBox::new();
        flow.set_selection_mode(SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_column_spacing(12);
        flow.set_row_spacing(12);
        flow.set_margin_top(16);
        flow.set_margin_bottom(16);
        flow.set_margin_start(16);
        flow.set_margin_end(16);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(8);
        flow.set_activate_on_single_click(true);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&flow));

        let root_page = adw::NavigationPage::new(&scroll, &gettext("Artists"));
        root_page.set_tag(Some("artists-root"));
        nav.add(&root_page);

        let artists_list: Rc<RefCell<Vec<Artist>>> = Rc::new(RefCell::new(Vec::new()));
        let all_albums: Rc<RefCell<Vec<Album>>> = Rc::new(RefCell::new(Vec::new()));
        let on_play: Rc<RefCell<Option<PlayCallback>>> = Rc::new(RefCell::new(None));
        let avatars: AvatarMap = Rc::new(RefCell::new(HashMap::new()));

        {
            let nav_c = nav.clone();
            let artists_c = Rc::clone(&artists_list);
            let albums_c = Rc::clone(&all_albums);
            let on_play_c = Rc::clone(&on_play);
            let db_c = Arc::clone(&db);
            let current_path_c = Rc::clone(&current_path);

            flow.connect_child_activated(move |_, child| {
                let idx = child.index() as usize;
                let artist_name = artists_c.borrow().get(idx).map(|a| a.name.clone());
                if let Some(name) = artist_name {
                    let mut artist_albums: Vec<Album> = albums_c
                        .borrow()
                        .iter()
                        .filter(|a| a.artist == name)
                        .cloned()
                        .collect();

                    {
                        let db_g = db_c.lock().unwrap();
                        for album in &mut artist_albums {
                            if album.cover.is_none() {
                                album.cover = db_g.get_cover(&album.artist, &album.name);
                            }
                        }
                    }

                    let page = make_artist_detail_page(
                        nav_c.clone(),
                        &name,
                        artist_albums,
                        Rc::clone(&on_play_c),
                        Rc::clone(&current_path_c),
                    );
                    nav_c.push(&page);
                }
            });
        }

        Self { root: nav, flow, artists_list, all_albums, on_play, avatars, current_filter: Rc::new(RefCell::new(String::new())) }
    }

    pub fn set_on_play(&self, callback: impl Fn(Vec<Track>, usize) + 'static) {
        *self.on_play.borrow_mut() = Some(std::boxed::Box::new(callback));
    }

    pub fn filter(&self, query: &str) {
        *self.current_filter.borrow_mut() = query.to_string();
        if query.is_empty() {
            self.flow.set_filter_func(|_| true);
        } else {
            let q = query.to_lowercase();
            let artists = Rc::clone(&self.artists_list);
            self.flow.set_filter_func(move |child| {
                let idx = child.index() as usize;
                if let Some(artist) = artists.borrow().get(idx) {
                    artist.name.to_lowercase().contains(&q)
                } else {
                    false
                }
            });
        }
    }

    pub fn load_artists(&self, artists: Vec<Artist>, albums: Vec<Album>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.avatars.borrow_mut().clear();

        let mut names_to_fetch: Vec<String> = Vec::new();

        for artist in &artists {
            let (card, avatar) = make_artist_card(artist);
            self.avatars.borrow_mut().insert(artist.name.clone(), avatar);
            self.flow.append(&card);
            names_to_fetch.push(artist.name.clone());
        }

        *self.artists_list.borrow_mut() = artists;
        *self.all_albums.borrow_mut() = albums;

        let active = self.current_filter.borrow().clone();
        if !active.is_empty() {
            self.filter(&active);
        }

        if !names_to_fetch.is_empty() {
            self.start_photo_fetch(names_to_fetch);
        }
    }

    fn start_photo_fetch(&self, artists: Vec<String>) {
        use std::sync::atomic::{AtomicBool, Ordering};

        let queue: Arc<Mutex<Vec<ScaledPhoto>>> = Arc::new(Mutex::new(Vec::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let queue_tx = Arc::clone(&queue);
        let finished_tx = Arc::clone(&finished);

        std::thread::spawn(move || {
            for artist in &artists {
                if let Some(bytes) = crate::library::metadata::fetch_artist_photo(artist) {
                    if let Some(scaled) = scale_to_pixels(&bytes, AVATAR_SIZE) {
                        queue_tx.lock().unwrap().push((artist.clone(), scaled.0, scaled.1, scaled.2));
                    }
                }
            }
            finished_tx.store(true, Ordering::Relaxed);
        });

        let avatars = Rc::clone(&self.avatars);
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let mut q = queue.lock().unwrap();
            for (name, pixels, rowstride, has_alpha) in q.drain(..) {
                if let Some(avatar) = avatars.borrow().get(&name) {
                    let texture = pixels_to_texture(pixels, rowstride, has_alpha, AVATAR_SIZE);
                    avatar.set_custom_image(Some(&texture));
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

fn make_artist_detail_page(
    nav: adw::NavigationView,
    artist_name: &str,
    albums: Vec<Album>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
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

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_column_spacing(3);
    flow.set_row_spacing(3);
    flow.set_margin_top(8);
    flow.set_margin_bottom(8);
    flow.set_margin_start(8);
    flow.set_margin_end(8);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(12);
    flow.set_activate_on_single_click(true);

    for album in &albums {
        flow.append(&make_artist_album_card(album));
    }

    let albums_rc = Rc::new(albums);

    {
        let albums_c = Rc::clone(&albums_rc);
        let on_play_c = Rc::clone(&on_play);
        btn_play_all.connect_clicked(move |_| {
            let all_tracks: Vec<Track> = albums_c
                .iter()
                .flat_map(|a| a.tracks.iter().cloned())
                .collect();
            if let Some(cb) = on_play_c.borrow().as_ref() {
                cb(all_tracks, usize::MAX);
            }
        });
    }

    {
        let albums_c = Rc::clone(&albums_rc);
        let on_play_c = Rc::clone(&on_play);
        let nav_c = nav.clone();
        let current_path_c = Rc::clone(&current_path);
        flow.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            if let Some(album) = albums_c.get(idx) {
                let page = make_album_detail_page(
                    album,
                    Rc::clone(&on_play_c),
                    Rc::clone(&current_path_c),
                );
                nav_c.push(&page);
            }
        });
    }

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&flow));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));

    adw::NavigationPage::new(&toolbar, artist_name)
}

fn make_artist_album_card(album: &Album) -> FlowBoxChild {
    let (child, stack, picture) = make_album_card(album, false);
    if let Some(ref data) = album.cover {
        if let Some((pixels, rowstride, has_alpha)) =
            scale_to_pixels(data.as_slice(), CARD_SIZE)
        {
            let texture = pixels_to_texture(pixels, rowstride, has_alpha, CARD_SIZE);
            picture.set_paintable(Some(&texture));
            stack.set_visible_child_name("art");
        }
    }
    child
}

fn make_artist_card(artist: &Artist) -> (FlowBoxChild, adw::Avatar) {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_halign(Align::Center);
    vbox.set_hexpand(false);
    vbox.set_margin_top(8);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let avatar = adw::Avatar::new(AVATAR_SIZE, Some(&artist.name), true);
    avatar.set_halign(Align::Center);
    avatar.set_valign(Align::Center);

    let lbl_name = Label::new(Some(&artist.name));
    lbl_name.add_css_class("body");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_max_width_chars(18);
    lbl_name.set_halign(Align::Center);
    lbl_name.set_justify(gtk4::Justification::Center);

    let info_str = format!(
        "{} {} · {} {}",
        artist.album_count,
        ngettext("album", "albums", artist.album_count as u32),
        artist.track_count,
        ngettext("song", "songs", artist.track_count as u32),
    );
    let lbl_info = Label::new(Some(&info_str));
    lbl_info.add_css_class("dim-label");
    lbl_info.add_css_class("caption");
    lbl_info.set_halign(Align::Center);
    lbl_info.set_justify(gtk4::Justification::Center);

    vbox.append(&avatar);
    vbox.append(&lbl_name);
    vbox.append(&lbl_info);

    let child = FlowBoxChild::new();
    child.add_css_class("artist-card");
    child.set_halign(Align::Center);
    child.set_valign(Align::Center);
    child.set_child(Some(&vbox));
    (child, avatar)
}
