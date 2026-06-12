use crate::i18n::{gettext, ngettext};
use crate::library::db::Database;
use crate::library::{Album, Artist, Track};
use crate::ui::albums_view::{make_album_card, make_album_detail_page, CARD_SIZE};
use crate::ui::image_apply::{apply_image, ImageTarget};
use crate::ui::image_loader::{self, FetchOutcome, ImagePipelineConfig};
use crate::ui::now_playing::NowPlaying;
use crate::ui::widgets::{content_clamp, page_title_row, play_all_button};
use adw::prelude::*;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, FlowBox, FlowBoxChild, Label, Orientation, ScrolledWindow, SelectionMode,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub const AVATAR_SIZE: i32 = 152;

type PlayCallback = Box<dyn Fn(Vec<Track>, usize)>;
type AvatarMap = Rc<RefCell<HashMap<String, adw::Avatar>>>;

pub struct ArtistsView {
    pub root: adw::NavigationView,
    flow: FlowBox,
    artists_list: Rc<RefCell<Vec<Artist>>>,
    /// Lowercased artist name per entry in `artists_list`, in the same order.
    /// Precomputed on load so filtering is one `contains` per child.
    search_keys: Rc<RefCell<Vec<String>>>,
    all_albums: Rc<RefCell<Vec<Album>>>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
    avatars: AvatarMap,
    current_filter: Rc<RefCell<String>>,
}

impl ArtistsView {
    pub fn new(db: Arc<Mutex<Database>>, now_playing: Rc<NowPlaying>) -> Self {
        let nav = adw::NavigationView::new();

        let flow = FlowBox::new();
        flow.set_selection_mode(SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_column_spacing(2);
        flow.set_row_spacing(8);
        flow.set_margin_top(8);
        flow.set_margin_bottom(16);
        flow.set_margin_start(4);
        flow.set_margin_end(4);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(8);
        flow.set_activate_on_single_click(true);

        // Same Clamp parameters as TrackList and AlbumsView so all surfaces
        // share the same useful width.
        let clamp = content_clamp();
        clamp.set_child(Some(&flow));

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&clamp));

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
            let now_playing_c = Rc::clone(&now_playing);

            flow.connect_child_activated(move |_, child| {
                // Double-click guard: see the equivalent check in AlbumsView.
                if nav_c.visible_page().and_then(|p| p.tag()).as_deref() != Some("artists-root") {
                    return;
                }
                let idx = child.index() as usize;
                let artist_name = artists_c.borrow().get(idx).map(|a| a.name.clone());
                if let Some(name) = artist_name {
                    // Two kinds of hits:
                    //   * Direct: album.artist == name → keep the whole album.
                    //   * Compilation: album.artist != name but some tracks
                    //     do match (Various Artists / OSTs) → keep only the
                    //     artist's tracks under that album, with the original
                    //     album name preserved so the cover still resolves.
                    // Both comparisons are case-insensitive so tags like
                    // "Comes With The Fall" vs "Comes With the Fall" resolve
                    // to the same canonical artist entry.
                    let name_lower = name.to_lowercase();
                    let mut artist_albums: Vec<Album> = albums_c
                        .borrow()
                        .iter()
                        .filter_map(|a| {
                            if a.artist.to_lowercase() == name_lower {
                                return Some(a.clone());
                            }
                            let tracks: Vec<crate::library::Track> = a
                                .tracks
                                .iter()
                                .filter(|t| t.display_artist().to_lowercase() == name_lower)
                                .cloned()
                                .collect();
                            if tracks.is_empty() {
                                None
                            } else {
                                Some(Album {
                                    name: a.name.clone(),
                                    artist: a.artist.clone(),
                                    tracks,
                                    cover: a.cover.clone(),
                                })
                            }
                        })
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
                        Rc::clone(&now_playing_c),
                        Arc::clone(&db_c),
                    );
                    nav_c.push(&page);
                }
            });
        }

        Self {
            root: nav,
            flow,
            artists_list,
            search_keys: Rc::new(RefCell::new(Vec::new())),
            all_albums,
            on_play,
            avatars,
            current_filter: Rc::new(RefCell::new(String::new())),
        }
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
            let keys = Rc::clone(&self.search_keys);
            self.flow.set_filter_func(move |child| {
                let idx = child.index() as usize;
                keys.borrow()
                    .get(idx)
                    .is_some_and(|key| key.contains(&q))
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
            crate::ui::cover_picker::install_artist_photo_gesture(
                &card,
                artist.name.clone(),
                avatar.clone(),
            );
            self.avatars
                .borrow_mut()
                .insert(artist.name.clone(), avatar);
            self.flow.append(&card);
            names_to_fetch.push(artist.name.clone());
        }

        *self.search_keys.borrow_mut() =
            artists.iter().map(|a| a.name.to_lowercase()).collect();
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

    /// Drive the shared two-pass image pipeline for artist photos.
    /// No local source for artist photos (yet), so the fast lane always
    /// misses and everything resolves through Last.fm.
    fn start_photo_fetch(&self, artists: Vec<String>) {
        let avatars = Rc::clone(&self.avatars);
        image_loader::run(
            artists,
            ImagePipelineConfig {
                target_size: AVATAR_SIZE,
                slow_delay_ms: 0,
            },
            |_artist: &String| FetchOutcome::Miss,
            Some(Box::new(|artist: &String| {
                crate::library::metadata::fetch_artist_photo(artist)
            })),
            move |artist: &String, texture| {
                if let Some(avatar) = avatars.borrow().get(artist) {
                    avatar.set_custom_image(Some(&texture));
                }
            },
        );
    }
}

fn make_artist_detail_page(
    nav: adw::NavigationView,
    artist_name: &str,
    albums: Vec<Album>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
    now_playing: Rc<NowPlaying>,
    db: Arc<Mutex<Database>>,
) -> adw::NavigationPage {
    // No HeaderBar — the back arrow sits inline next to the artist name,
    // matching Songs / album-detail layouts exactly. NavigationPage still
    // keeps the title for accessibility / breadcrumbs.
    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);
    // Mirror the main Albums grid: tighter columns, taller rows.
    flow.set_column_spacing(4);
    flow.set_row_spacing(14);
    flow.set_margin_top(4);
    flow.set_margin_bottom(12);
    flow.set_margin_start(4);
    flow.set_margin_end(4);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(12);
    flow.set_activate_on_single_click(true);

    for album in &albums {
        flow.append(&make_artist_album_card(album, Arc::clone(&db)));
    }

    let albums_rc = Rc::new(albums);

    // Action row: `[N albums]  spacer  [▶ Play all]`. Same visual recipe as
    // TrackList's action row (heading + dim-label on the left, suggested
    // action button on the right) so navigating between Songs / Album detail
    // / Artist detail does not move the button.
    let action_bar = GtkBox::new(Orientation::Horizontal, 8);
    action_bar.set_margin_top(8);
    action_bar.set_margin_bottom(6);
    action_bar.set_margin_start(4);
    action_bar.set_margin_end(4);

    let lbl_count = Label::new(Some(&format!(
        "{} {}",
        albums_rc.len(),
        ngettext("album", "albums", albums_rc.len() as u32)
    )));
    lbl_count.add_css_class("heading");
    lbl_count.add_css_class("dim-label");
    lbl_count.set_xalign(0.0);
    lbl_count.set_valign(Align::Center);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let btn_play_all = play_all_button(&gettext("Play all"));

    action_bar.append(&lbl_count);
    action_bar.append(&spacer);
    action_bar.append(&btn_play_all);

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
        let now_playing_c = Rc::clone(&now_playing);
        flow.connect_child_activated(move |_, child| {
            // Double-click guard: see the equivalent check in AlbumsView.
            if nav_c.visible_page().and_then(|p| p.tag()).as_deref() != Some("artist-detail") {
                return;
            }
            let idx = child.index() as usize;
            if let Some(album) = albums_c.get(idx) {
                let page =
                    make_album_detail_page(album, Rc::clone(&on_play_c), Rc::clone(&now_playing_c));
                nav_c.push(&page);
            }
        });
    }

    // One Clamp for the action row, another for the grid. Both come from the
    // shared helper so the right edge of the "Play all" button lines up with
    // the right edge of the grid below.
    let action_clamp = content_clamp();
    action_clamp.set_child(Some(&action_bar));

    let grid_clamp = content_clamp();
    grid_clamp.set_child(Some(&flow));

    // Section header with back arrow inline — same helper, same look as
    // album-detail.
    let title_row = page_title_row(artist_name, true);
    let title_clamp = content_clamp();
    title_clamp.set_child(Some(&title_row));

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.append(&title_clamp);
    content.append(&action_clamp);

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&grid_clamp));
    content.append(&scroll);

    let page = adw::NavigationPage::new(&content, artist_name);
    // Tagged so the album grid above can tell whether this page is still the
    // visible one before pushing an album detail. Unique in the stack: the
    // artists-root guard ensures at most one artist detail at a time.
    page.set_tag(Some("artist-detail"));
    page
}

fn make_artist_album_card(album: &Album, db: Arc<Mutex<Database>>) -> FlowBoxChild {
    let (child, stack, picture) = make_album_card(album, false);
    if let Some(ref data) = album.cover {
        apply_image(
            ImageTarget::AlbumCover {
                picture: picture.clone(),
                stack: stack.clone(),
            },
            Some(data.as_slice()),
            CARD_SIZE,
        );
    }

    // Right-click on the album card opens the cover picker, the same way the
    // main Albums grid does. Reuses `install_album_cover_gesture` so the menu,
    // search box and persistence are identical across both entry points.
    let track_path = album
        .tracks
        .first()
        .map(|t| t.path.clone())
        .unwrap_or_default();
    crate::ui::cover_picker::install_album_cover_gesture(
        &child,
        db,
        album.artist.clone(),
        album.name.clone(),
        track_path,
        stack,
        picture,
    );

    child
}

fn make_artist_card(artist: &Artist) -> (FlowBoxChild, adw::Avatar) {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_halign(Align::Center);
    vbox.set_hexpand(false);
    vbox.set_margin_top(8);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(6);
    vbox.set_margin_end(6);

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
