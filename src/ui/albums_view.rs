use crate::i18n::gettext;
use crate::library::db::Database;
use crate::library::{Album, Track};
use crate::ui::image_loader::{self, FetchOutcome, ImagePipelineConfig};
use crate::ui::now_playing::NowPlaying;
use crate::ui::track_list::{TrackList, TrackListConfig};
use crate::ui::widgets::{content_clamp, page_title_row};
use adw::prelude::*;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, ContentFit, FlowBox, FlowBoxChild, Label, Orientation, Overlay, Picture,
    ScrolledWindow, SelectionMode, Stack, StackTransitionType,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(crate) const CARD_SIZE: i32 = 176;

type CoverMap = Rc<RefCell<HashMap<String, (Stack, Picture)>>>;
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
    pub fn new(now_playing: Rc<NowPlaying>) -> Self {
        let flow = FlowBox::new();
        flow.set_selection_mode(SelectionMode::Single);
        flow.set_homogeneous(true);
        // Tight horizontal packing, more breathing room vertically — covers
        // are visually tall objects (gradient bar + title), so a bit of
        // extra row gap reads better than uniform spacing.
        flow.set_column_spacing(4);
        flow.set_row_spacing(14);
        flow.set_margin_top(8);
        flow.set_margin_bottom(12);
        flow.set_margin_start(4);
        flow.set_margin_end(4);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(12);
        flow.set_activate_on_single_click(true);

        // Same Clamp parameters as TrackList — keeps grids and lists aligned
        // to the same useful width so the "Play all" button stays put when
        // navigating between Songs / Albums / Artists detail pages.
        let clamp = content_clamp();
        clamp.set_child(Some(&flow));

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&clamp));

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
            let now_playing_c = Rc::clone(&now_playing);
            flow.connect_child_activated(move |_, child| {
                let idx = child.index() as usize;
                let album = albums_c.borrow().get(idx).cloned();
                if let Some(album) = album {
                    let page = make_album_detail_page(
                        &album,
                        Rc::clone(&on_play_c),
                        Rc::clone(&now_playing_c),
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

        let mut need_fetch: Vec<(String, String, Vec<String>)> = Vec::new();

        for album in &albums {
            let key = format!("{}|{}", album.artist, album.name);
            let (card, stack, picture) = make_album_card(album, true);

            let track_path = album
                .tracks
                .first()
                .map(|t| t.path.clone())
                .unwrap_or_default();

            crate::ui::cover_picker::install_album_cover_gesture(
                &card,
                Arc::clone(&db),
                album.artist.clone(),
                album.name.clone(),
                track_path,
                stack.clone(),
                picture.clone(),
            );

            self.covers
                .borrow_mut()
                .insert(key.clone(), (stack, picture));
            self.flow.append(&card);

            // Hand the cover fetcher *every* track path so it can scan past
            // artless leading tracks to the one that embeds the album art.
            let track_paths: Vec<String> = album.tracks.iter().map(|t| t.path.clone()).collect();
            need_fetch.push((album.artist.clone(), album.name.clone(), track_paths));
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

    /// Drive the shared two-pass image pipeline for album covers.
    /// Fast lane: DB cache + embedded ID3 art. Slow lane: Last.fm.
    fn start_cover_fetch(
        &self,
        albums: Vec<(String, String, Vec<String>)>,
        db: Arc<Mutex<Database>>,
    ) {
        let covers = Rc::clone(&self.covers);
        let db_fast = Arc::clone(&db);
        let db_slow = Arc::clone(&db);

        image_loader::run(
            albums,
            ImagePipelineConfig {
                target_size: CARD_SIZE,
                slow_delay_ms: 1100,
            },
            move |item: &(String, String, Vec<String>)| {
                let (artist, album_name, track_paths) = item;
                // Stored cover wins. Empty bytes = user removed it on purpose:
                // skip outright so the slow lane does not refetch it.
                if let Some(bytes) = db_fast.lock().unwrap().get_cover(artist, album_name) {
                    if bytes.is_empty() {
                        return FetchOutcome::Skip;
                    }
                    return FetchOutcome::Got(bytes);
                }
                // Embedded ID3/Vorbis art. Inconsistently-tagged rips often
                // leave their first tracks artless, so scan the album's tracks
                // until one carries cover art instead of giving up on the
                // first — otherwise the whole album (and every song played
                // from it) stays coverless even though siblings embed the art.
                for path in track_paths {
                    if let Some(bytes) = crate::library::art::read_cover_art(path) {
                        let _ = db_fast
                            .lock()
                            .unwrap()
                            .set_cover(artist, album_name, &bytes);
                        return FetchOutcome::Got(bytes);
                    }
                }
                FetchOutcome::Miss
            },
            Some(Box::new(move |item: &(String, String, Vec<String>)| {
                let (artist, album_name, _) = item;
                let res = crate::library::metadata::fetch_album_cover(artist, album_name);
                if let Some(ref bytes) = res {
                    let _ = db_slow.lock().unwrap().set_cover(artist, album_name, bytes);
                }
                res
            })),
            move |item: &(String, String, Vec<String>), texture| {
                let key = format!("{}|{}", item.0, item.1);
                if let Some((stack, picture)) = covers.borrow().get(&key) {
                    picture.set_paintable(Some(&texture));
                    stack.set_visible_child_name("art");
                }
            },
        );
    }
}

/// Build a detail page for one album. Identical chrome to the global "Songs"
/// view: the `[N songs] + Play all` action row and the surrounding `Clamp`
/// live inside `TrackList`, so both surfaces share the exact same layout.
/// This page only adds the navigation header (back button + album title).
pub fn make_album_detail_page(
    album: &Album,
    on_play: PlayCb,
    now_playing: Rc<NowPlaying>,
) -> adw::NavigationPage {
    let track_list = TrackList::new(TrackListConfig::album_detail(), now_playing);
    track_list.load(album.tracks.clone());

    {
        let on_play_c = Rc::clone(&on_play);
        track_list.set_on_activate(move |tracks, idx| {
            if let Some(cb) = on_play_c.borrow().as_ref() {
                cb(tracks, idx);
            }
        });
    }

    {
        let on_play_c = Rc::clone(&on_play);
        track_list.set_on_play_all(move |tracks| {
            if let Some(cb) = on_play_c.borrow().as_ref() {
                cb(tracks, usize::MAX);
            }
        });
    }

    // No HeaderBar — the back arrow sits inline next to the title so this
    // page has the same vertical layout as the Songs view. The
    // NavigationPage still carries the title for accessibility / breadcrumbs.
    let title_row = page_title_row(&album.name, true);
    let title_clamp = content_clamp();
    title_clamp.set_child(Some(&title_row));

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.append(&title_clamp);
    content.append(&track_list.root);

    adw::NavigationPage::new(&content, &album.name)
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
