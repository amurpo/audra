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
    gdk, Align, Box as GtkBox, ContentFit, FlowBoxChild, GridView, Label, ListItem, NoSelection,
    Orientation, Overlay, Picture, ScrolledWindow, SignalListItemFactory, Stack,
    StackTransitionType, StringList,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(crate) const CARD_SIZE: i32 = 176;

/// Widget data key: each realized cell stashes its last-bound model position so
/// the async-cover repaint can resolve a recycled cell back to its album
/// without holding the `ListItem`. Mirrors `track_list::ROW_POS_KEY`.
const POS_KEY: &str = "audra-album-pos";

type PlayCb = Rc<RefCell<Option<Box<dyn Fn(Vec<Track>, usize)>>>>;

/// The album-detail page currently pushed on the nav, if any: its album key
/// (`"artist|name"`, also the page tag) and the song list it shows.
type OpenDetail = (String, Rc<TrackList>);

/// The inner widgets of one album card, returned by [`build_album_card`] so both
/// the recycling `GridView` factory and the FlowBox `make_album_card` (artists
/// view) share the exact same visuals.
struct AlbumCard {
    overlay: Overlay,
    stack: Stack,
    picture: Picture,
    name: Label,
    artist: Option<Label>,
}

pub struct AlbumsView {
    pub root: adw::NavigationView,
    grid: GridView,
    /// "Dumb" model: one empty placeholder per displayed album. It only carries
    /// the item count / position; the real data lives in `albums_data`, indexed
    /// through `displayed`. Same trick as `track_list`.
    model: StringList,
    /// Every album, unfiltered — the source of truth for re-filtering and the
    /// backing store the factory resolves positions against.
    albums_data: Rc<RefCell<Vec<Album>>>,
    /// Indices into `albums_data`, in grid order — the filtered view. Storing
    /// indices (not cloned `Album`s) keeps each keystroke cheap: filtering
    /// rebuilds this `Vec<usize>` instead of cloning albums and their tracks.
    displayed: Rc<RefCell<Vec<usize>>>,
    /// Lowercased `"name\nartist"` haystack per album, in `albums_data` order.
    /// Precomputed on load so filtering is one `contains` per album.
    search_keys: Rc<RefCell<Vec<String>>>,
    /// Decoded cover textures keyed by `"artist|name"` — *data, not widgets*, so
    /// they survive widget recycling. The factory reads them on bind; the async
    /// fetch inserts here and repaints the realized cells.
    cover_textures: Rc<RefCell<HashMap<String, gdk::Texture>>>,
    /// The library handle, set on every `load_albums`. The factory's right-click
    /// gesture needs it at click time (it isn't known when the factory is built).
    db: Rc<RefCell<Option<Arc<Mutex<Database>>>>>,
    on_play: PlayCb,
    current_filter: Rc<RefCell<String>>,
    /// The album-detail page currently pushed on `root`, if any: its album key
    /// (`"artist|name"`, also the page tag) and the `TrackList` it shows. Lets
    /// `load_albums` refresh an open detail *in place* on a rescan — updating
    /// the song list without popping the user back to the grid.
    open_detail: Rc<RefCell<Option<OpenDetail>>>,
}

impl AlbumsView {
    pub fn new(now_playing: Rc<NowPlaying>) -> Self {
        let model = StringList::new(&[]);
        // NoSelection: clicking a cover navigates straight to its detail page,
        // so there's no useful "selected album" state — and no selection chrome
        // painted over the artwork. `activate` still fires with single click.
        let selection = NoSelection::new(Some(model.clone()));
        let factory = SignalListItemFactory::new();

        let albums_data: Rc<RefCell<Vec<Album>>> = Rc::new(RefCell::new(Vec::new()));
        let displayed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let search_keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let cover_textures: Rc<RefCell<HashMap<String, gdk::Texture>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let db: Rc<RefCell<Option<Arc<Mutex<Database>>>>> = Rc::new(RefCell::new(None));
        let on_play: PlayCb = Rc::new(RefCell::new(None));
        let current_filter = Rc::new(RefCell::new(String::new()));
        let open_detail: Rc<RefCell<Option<OpenDetail>>> = Rc::new(RefCell::new(None));

        // --- setup: build one empty, reusable card + its right-click gesture ---
        {
            let displayed_s = Rc::clone(&displayed);
            let albums_s = Rc::clone(&albums_data);
            let db_s = Rc::clone(&db);
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let card = build_album_card(true);
                card.overlay.add_css_class("album-cover");
                // Fixed 176×176 cover, centered in its cell, as the item's direct
                // child — no AspectFrame/filler wrapper. The row comes out at the
                // cover's natural height (176) because the GridView is the
                // ScrolledWindow's direct scrollable child; halign/valign just
                // keep the cover from being pulled wide inside a wider column.
                card.overlay.set_halign(Align::Center);
                card.overlay.set_valign(Align::Center);
                item.set_child(Some(&card.overlay));

                // Right-click → cover menu. The cell is recycled across albums,
                // so the menu can't capture the album here; it resolves the one
                // bound *right now* from `displayed[position]` at click time.
                let gesture = gtk4::GestureClick::new();
                gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
                let item_weak = item.downgrade();
                let overlay_weak = card.overlay.downgrade();
                let displayed_g = Rc::clone(&displayed_s);
                let albums_g = Rc::clone(&albums_s);
                let db_g = Rc::clone(&db_s);
                gesture.connect_pressed(move |g, _, x, y| {
                    let (Some(item), Some(overlay)) = (item_weak.upgrade(), overlay_weak.upgrade())
                    else {
                        return;
                    };
                    let pos = item.position();
                    if pos == gtk4::INVALID_LIST_POSITION {
                        return;
                    }
                    let Some(db) = db_g.borrow().clone() else {
                        return;
                    };
                    let Some(&idx) = displayed_g.borrow().get(pos as usize) else {
                        return;
                    };
                    let (artist, album, track_path) = {
                        let data = albums_g.borrow();
                        let Some(a) = data.get(idx) else { return };
                        (
                            a.artist.clone(),
                            a.name.clone(),
                            a.tracks.first().map(|t| t.path.clone()).unwrap_or_default(),
                        )
                    };
                    let Some((stack, picture, _, _)) = card_parts(overlay.upcast_ref()) else {
                        return;
                    };
                    g.set_state(gtk4::EventSequenceState::Claimed);
                    crate::ui::cover_picker::show_album_cover_menu(
                        &overlay, x, y, db, artist, album, track_path, stack, picture,
                    );
                });
                card.overlay.add_controller(gesture);
            });
        }

        // --- bind: fill the recycled card from `albums_data[displayed[pos]]` ---
        {
            let displayed_b = Rc::clone(&displayed);
            let albums_b = Rc::clone(&albums_data);
            let textures_b = Rc::clone(&cover_textures);
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let pos = item.position() as usize;
                // The item's child IS the card overlay directly (see
                // `connect_setup` — no wrapper).
                let Some(overlay) = item.child() else { return };
                let Some((stack, picture, name_lbl, artist_lbl)) = card_parts(&overlay) else {
                    return;
                };

                let idx = match displayed_b.borrow().get(pos) {
                    Some(&i) => i,
                    None => return,
                };
                let data = albums_b.borrow();
                let Some(album) = data.get(idx) else { return };

                name_lbl.set_text(&album.name);
                if let Some(al) = &artist_lbl {
                    al.set_text(&album.artist);
                }

                // Stash the bound position on the overlay (the item's direct
                // child) for the async-cover repaint to resolve this cell.
                unsafe { overlay.set_data(POS_KEY, pos) };

                // Apply a known texture, or reset to the placeholder: a recycled
                // cell may still show another album's cover, so the `None` arm
                // must clear it rather than leaving the stale art behind.
                match textures_b.borrow().get(&album_key(album)) {
                    Some(texture) => {
                        picture.set_paintable(Some(texture));
                        stack.set_visible_child_name("art");
                    }
                    None => {
                        picture.set_paintable(None::<&gdk::Texture>);
                        stack.set_visible_child_name("placeholder");
                    }
                }
            });
        }

        let grid = GridView::new(Some(selection), Some(factory));
        // `audra-grid`: shared base class carrying the `.view` background reset
        // (see style.css). `audra-album-grid`: this grid's own spacing + hover.
        grid.add_css_class("audra-grid");
        grid.add_css_class("audra-album-grid");
        // Same packing range as the old FlowBox (2–12 columns).
        grid.set_min_columns(2);
        grid.set_max_columns(12);
        grid.set_single_click_activate(true);

        // GridView implements GtkScrollable, so — exactly like the ListView in
        // track_list — it MUST be the direct child of the ScrolledWindow. The
        // scroll machinery then virtualizes it and each row stays at the cover's
        // natural height (176). Wrapping it in the Clamp first (Clamp inside the
        // scroll) is what broke every earlier attempt: the scroll wraps the
        // Clamp in a Viewport, the GridView is no longer the scrollable child,
        // and every row stretches to the viewport height — the "tiras largas".
        // So: grid → ScrolledWindow, and the width Clamp goes on the OUTSIDE
        // (same nesting as track_list, which works).
        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&grid));

        // Same Clamp parameters as TrackList — keeps grids and lists aligned to
        // the same useful width so the "Play all" button stays put when
        // navigating between Songs / Albums / Artists detail pages.
        let clamp = content_clamp();
        clamp.set_vexpand(true);
        clamp.set_child(Some(&scroll));

        let nav = adw::NavigationView::new();
        let root_page = adw::NavigationPage::new(&clamp, &gettext("Albums"));
        root_page.set_tag(Some("albums-root"));
        nav.add(&root_page);

        // --- activate: single click opens the album detail page ---
        {
            let nav_c = nav.clone();
            let displayed_a = Rc::clone(&displayed);
            let albums_a = Rc::clone(&albums_data);
            let on_play_c = Rc::clone(&on_play);
            let now_playing_c = Rc::clone(&now_playing);
            let open_detail_a = Rc::clone(&open_detail);
            grid.connect_activate(move |_, pos| {
                // A fast double-click activates twice; only push while this grid
                // is still the visible page so the second activation can't stack
                // another copy of the detail page.
                if nav_c.visible_page().and_then(|p| p.tag()).as_deref() != Some("albums-root") {
                    return;
                }
                let idx = match displayed_a.borrow().get(pos as usize) {
                    Some(&i) => i,
                    None => return,
                };
                let album = albums_a.borrow().get(idx).cloned();
                if let Some(album) = album {
                    let (page, list) = make_album_detail_page(
                        &album,
                        Rc::clone(&on_play_c),
                        Rc::clone(&now_playing_c),
                    );
                    // Tag the page with the album key and remember its song list
                    // so a later rescan can refresh it in place (see load_albums).
                    let key = album_key(&album);
                    page.set_tag(Some(&key));
                    *open_detail_a.borrow_mut() = Some((key, list));
                    nav_c.push(&page);
                }
            });
        }

        Self {
            root: nav,
            grid,
            model,
            albums_data,
            displayed,
            search_keys,
            cover_textures,
            db,
            on_play,
            current_filter,
            open_detail,
        }
    }

    pub fn set_on_play(&self, callback: impl Fn(Vec<Track>, usize) + 'static) {
        *self.on_play.borrow_mut() = Some(Box::new(callback));
    }

    pub fn load_albums(&self, albums: Vec<Album>, db: Arc<Mutex<Database>>) {
        *self.db.borrow_mut() = Some(Arc::clone(&db));
        self.cover_textures.borrow_mut().clear();

        // Fetch covers for *every* album (not just the filtered view): the
        // result is cached by key, so an album filtered out now still shows its
        // art the moment the filter clears. Hand the fetcher all track paths so
        // it can scan past artless leading tracks to the one embedding the art.
        let need_fetch: Vec<(String, String, Vec<String>)> = albums
            .iter()
            .map(|a| {
                (
                    a.artist.clone(),
                    a.name.clone(),
                    a.tracks.iter().map(|t| t.path.clone()).collect(),
                )
            })
            .collect();

        *self.search_keys.borrow_mut() = albums
            .iter()
            .map(|a| format!("{}\n{}", a.name.to_lowercase(), a.artist.to_lowercase()))
            .collect();
        *self.albums_data.borrow_mut() = albums;

        // Rebuild the displayed set (honouring any active filter) and the model.
        let active = self.current_filter.borrow().clone();
        self.apply_filter(&active);

        // Keep an open album detail in sync without leaving it: if the user is
        // viewing a song list, reload it from the fresh data instead of letting
        // the rescan only touch the grid behind it. If the album is gone (e.g.
        // its files were deleted), fall back to the grid.
        self.refresh_open_detail();

        if !need_fetch.is_empty() {
            self.start_cover_fetch(need_fetch, db);
        }
    }

    /// Reload the pushed album-detail page's song list from the current
    /// `albums_data`, leaving the navigation stack untouched so the user stays
    /// on the album they were viewing. A no-op when the grid is the visible
    /// page; pops to the grid only if the open album no longer exists.
    fn refresh_open_detail(&self) {
        // Only act when a detail page is actually on top, matched by its tag
        // (the album key) to the page we remembered when pushing it.
        let Some(tag) = self.root.visible_page().and_then(|p| p.tag()) else {
            return;
        };
        if tag.as_str() == "albums-root" {
            return;
        }
        let open = self.open_detail.borrow();
        let Some((key, list)) = open.as_ref() else { return };
        if key.as_str() != tag.as_str() {
            return;
        }
        let found = self
            .albums_data
            .borrow()
            .iter()
            .find(|a| &album_key(a) == key)
            .map(|a| a.tracks.clone());
        match found {
            Some(tracks) => list.load(tracks),
            None => {
                // The album vanished from the library; drop the borrow before
                // popping so the nav change can't alias `open_detail`.
                drop(open);
                self.root.pop_to_tag("albums-root");
            }
        }
    }

    pub fn filter(&self, query: &str) {
        *self.current_filter.borrow_mut() = query.to_string();
        self.apply_filter(query);
        // If an album-detail song list is the page on top, filter its tracks
        // too — otherwise searching while inside a detail page does nothing,
        // since `apply_filter` only touches the grid hidden behind it.
        let on_detail = self
            .root
            .visible_page()
            .and_then(|p| p.tag())
            .is_some_and(|t| t.as_str() != "albums-root");
        if on_detail {
            if let Some((_, list)) = self.open_detail.borrow().as_ref() {
                list.filter(query);
            }
        }
    }

    /// Recompute `displayed` from `albums_data`/`search_keys` and splice the
    /// model so the grid shows exactly the matching albums. Mirrors
    /// `track_list::apply_filter`.
    fn apply_filter(&self, query: &str) {
        let displayed: Vec<usize> = if query.is_empty() {
            (0..self.albums_data.borrow().len()).collect()
        } else {
            let q = query.to_lowercase();
            self.search_keys
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, key)| key.contains(&q))
                .map(|(i, _)| i)
                .collect()
        };
        let n = self.model.n_items();
        let additions: Vec<&str> = displayed.iter().map(|_| "").collect();
        *self.displayed.borrow_mut() = displayed;
        self.model.splice(0, n, &additions);
    }

    /// Drive the shared two-pass image pipeline for album covers.
    /// Fast lane: DB cache + embedded ID3 art. Slow lane: Last.fm.
    fn start_cover_fetch(
        &self,
        albums: Vec<(String, String, Vec<String>)>,
        db: Arc<Mutex<Database>>,
    ) {
        let db_fast = Arc::clone(&db);
        let db_slow = Arc::clone(&db);
        let textures = Rc::clone(&self.cover_textures);
        let displayed = Rc::clone(&self.displayed);
        let albums_data = Rc::clone(&self.albums_data);
        let grid_weak = self.grid.downgrade();

        image_loader::run(
            albums,
            ImagePipelineConfig {
                target_size: CARD_SIZE,
                // No blanket delay: the only rate-limited host (MusicBrainz) is
                // now gated per-host inside metadata, so albums served by other
                // sources — or by an unthrottled CAA download — no longer pay a
                // global 1.1 s tax between items.
                slow_delay_ms: 0,
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
                textures.borrow_mut().insert(key, texture);
                // Repaint the realized cells in place: a cover that lands after
                // its cell is already on screen won't get a fresh `bind`, so we
                // paint it here (cells scrolled into view later read the texture
                // from the map on bind). Same idea as the now-playing repaint.
                if let Some(grid) = grid_weak.upgrade() {
                    repaint_covers(&grid, &displayed, &albums_data, &textures);
                }
            },
        );
    }
}

/// Stable cover-cache key for an album: `"artist|name"`. Also used as the
/// detail page's navigation tag so a rescan can find the open page again.
pub(crate) fn album_key(album: &Album) -> String {
    format!("{}|{}", album.artist, album.name)
}

/// Build one empty album card (no text, placeholder cover). Both the `GridView`
/// factory's `setup` and `make_album_card` (FlowBox) use this so the two
/// surfaces stay pixel-identical.
fn build_album_card(show_artist: bool) -> AlbumCard {
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

    let lbl_name = Label::new(None);
    lbl_name.add_css_class("album-overlay-title");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_xalign(0.0);
    info.append(&lbl_name);

    let lbl_artist = if show_artist {
        let l = Label::new(None);
        l.add_css_class("album-overlay-artist");
        l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        l.set_xalign(0.0);
        info.append(&l);
        Some(l)
    } else {
        None
    };

    overlay.add_overlay(&info);

    AlbumCard {
        overlay,
        stack: cover_stack,
        picture: cover_picture,
        name: lbl_name,
        artist: lbl_artist,
    }
}

/// Re-derive a card's inner widgets from its (recycled) overlay. The structure
/// is fixed by [`build_album_card`]: the overlay's main child is the cover
/// `Stack` (`first_child`), whose first page is the `Picture`; the info `Box`
/// holding the two labels is the overlay child (`stack.next_sibling`). Mirrors
/// the tree-walking approach in `track_list::track_row_of`.
fn card_parts(overlay: &gtk4::Widget) -> Option<(Stack, Picture, Label, Option<Label>)> {
    let stack = overlay.first_child().and_downcast::<Stack>()?;
    let picture = stack.first_child().and_downcast::<Picture>()?;
    let info = stack.next_sibling().and_downcast::<GtkBox>()?;
    let name = info.first_child().and_downcast::<Label>()?;
    let artist = name.next_sibling().and_downcast::<Label>();
    Some((stack, picture, name, artist))
}

/// Repaint the cover of every realized cell whose album already has a texture.
/// Driven by the async fetch as results land. Walks the realized cells and
/// resolves each one's album through the position stashed by `connect_bind`.
fn repaint_covers(
    grid: &GridView,
    displayed: &RefCell<Vec<usize>>,
    albums: &RefCell<Vec<Album>>,
    textures: &RefCell<HashMap<String, gdk::Texture>>,
) {
    let disp = displayed.borrow();
    let data = albums.borrow();
    let tex = textures.borrow();
    let mut next = grid.first_child();
    while let Some(widget) = next {
        next = widget.next_sibling();
        // Each realized cell: item widget → card overlay (holds POS_KEY).
        // Mirrors the `connect_bind` structure (overlay is the direct child).
        let Some(overlay) = widget.first_child() else {
            continue;
        };
        let Some(pos_ptr) = (unsafe { overlay.data::<usize>(POS_KEY) }) else {
            continue;
        };
        let pos = unsafe { *pos_ptr.as_ref() };
        let Some(&idx) = disp.get(pos) else { continue };
        let Some(album) = data.get(idx) else { continue };
        let Some(texture) = tex.get(&album_key(album)) else {
            continue;
        };
        let Some((stack, picture, _, _)) = card_parts(&overlay) else {
            continue;
        };
        picture.set_paintable(Some(texture));
        stack.set_visible_child_name("art");
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
) -> (adw::NavigationPage, Rc<TrackList>) {
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

    let page = adw::NavigationPage::new(&content, &album.name);
    (page, track_list)
}

/// Build a FlowBox album card with data already set. Still used by the artists
/// view (its album grids stay on FlowBox); the main Albums grid uses the
/// recycling `GridView` factory instead.
pub fn make_album_card(album: &Album, show_artist: bool) -> (FlowBoxChild, Stack, Picture) {
    let card = build_album_card(show_artist);
    card.name.set_text(&album.name);
    if let Some(artist_lbl) = &card.artist {
        artist_lbl.set_text(&album.artist);
    }

    let child = FlowBoxChild::new();
    child.add_css_class("mosaic-child");
    child.set_child(Some(&card.overlay));
    child.set_halign(Align::Center);
    child.set_valign(Align::Center);

    (child, card.stack, card.picture)
}
