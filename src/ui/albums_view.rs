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
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(crate) const CARD_SIZE: i32 = 176;

/// How many cards are appended to a FlowBox per main-loop turn. The first batch
/// lands synchronously (so the grid is never blank); the rest are spread one
/// batch per idle tick. See [`append_in_batches`].
pub(crate) const APPEND_BATCH: usize = 64;

type CoverMap = Rc<RefCell<HashMap<String, (Stack, Picture)>>>;
type PlayCb = Rc<RefCell<Option<Box<dyn Fn(Vec<Track>, usize)>>>>;

pub struct AlbumsView {
    pub root: adw::NavigationView,
    flow: FlowBox,
    albums_data: Rc<RefCell<Vec<Album>>>,
    /// Lowercased `"name\nartist"` haystack per album, in `albums_data` order.
    /// Precomputed on load so filtering is one `contains` per child.
    search_keys: Rc<RefCell<Vec<String>>>,
    covers: CoverMap,
    on_play: PlayCb,
    current_filter: Rc<RefCell<String>>,
    /// Bumped on every `load_albums` so a chunked append still in flight from a
    /// previous load stops instead of dropping stale cards into the new grid.
    load_gen: Rc<Cell<u64>>,
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
                // A fast double-click activates twice; only push while this
                // grid is still the visible page, so the second activation
                // can't stack another copy of the detail page.
                if nav_c.visible_page().and_then(|p| p.tag()).as_deref() != Some("albums-root") {
                    return;
                }
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
            search_keys: Rc::new(RefCell::new(Vec::new())),
            covers,
            on_play,
            current_filter: Rc::new(RefCell::new(String::new())),
            load_gen: Rc::new(Cell::new(0)),
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

        // Supersede any chunked append still running from a previous load so it
        // can't append stale cards into the grid we just cleared.
        let gen = self.load_gen.get().wrapping_add(1);
        self.load_gen.set(gen);

        let mut need_fetch: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut cards: Vec<FlowBoxChild> = Vec::with_capacity(albums.len());

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

            // Populate the cover map for *every* album up front (cheap, no
            // realize) so the async cover fetch always finds its target even
            // for cards that have not been appended yet.
            self.covers
                .borrow_mut()
                .insert(key.clone(), (stack, picture));
            cards.push(card);

            // Hand the cover fetcher *every* track path so it can scan past
            // artless leading tracks to the one that embeds the album art.
            let track_paths: Vec<String> = album.tracks.iter().map(|t| t.path.clone()).collect();
            need_fetch.push((album.artist.clone(), album.name.clone(), track_paths));
        }

        // Append in small batches across the main loop instead of all at once.
        // FlowBox does not virtualize: it lays out and realizes every child it
        // holds, so appending thousands in one frame causes a multi-hundred-ms
        // hitch the first time the library opens, growing linearly with the
        // collection. Spreading the appends keeps the UI responsive on large
        // libraries; the first batch lands synchronously so the grid is never
        // blank, and small libraries finish entirely within that first batch.
        let flow = self.flow.clone();
        append_in_batches(cards, Rc::clone(&self.load_gen), gen, move |card| {
            flow.append(&card)
        });

        *self.search_keys.borrow_mut() = albums
            .iter()
            .map(|a| format!("{}\n{}", a.name.to_lowercase(), a.artist.to_lowercase()))
            .collect();
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
            let keys = Rc::clone(&self.search_keys);
            self.flow.set_filter_func(move |child| {
                let idx = child.index() as usize;
                keys.borrow()
                    .get(idx)
                    .is_some_and(|key| key.contains(&q))
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

/// Append `items` through `append`: the first [`APPEND_BATCH`] synchronously
/// (so the caller's view is never blank on the first frame), the rest spread
/// across the main loop one batch per idle tick.
///
/// `load_gen`/`gen` let a later load supersede a drain still in flight: each
/// `load_albums` bumps the shared `load_gen` and passes its own `gen`. When the
/// two stop matching, the pending drain breaks instead of appending stale cards
/// into a grid that was already cleared and refilled by the newer load.
pub(crate) fn append_in_batches<T: 'static>(
    items: Vec<T>,
    load_gen: Rc<Cell<u64>>,
    gen: u64,
    mut append: impl FnMut(T) + 'static,
) {
    let mut items = items.into_iter();
    // First batch synchronously. A library that fits in one batch is fully
    // appended here and schedules no idle source at all.
    for _ in 0..APPEND_BATCH {
        match items.next() {
            Some(item) => append(item),
            None => return,
        }
    }
    let pending = Rc::new(RefCell::new(items));
    glib::idle_add_local(move || {
        if load_gen.get() != gen {
            return glib::ControlFlow::Break; // superseded by a newer load
        }
        let mut items = pending.borrow_mut();
        for _ in 0..APPEND_BATCH {
            match items.next() {
                Some(item) => append(item),
                None => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `idle_add_local` always attaches its source to (and acquires) the global
    // default `MainContext`, so these tests must drive that one context — and
    // must not run concurrently, or their `acquire` calls race. This lock
    // serializes them; each run drains the context clean on entry and exit.
    static MAIN_LOOP_LOCK: Mutex<()> = Mutex::new(());

    fn with_main_loop<F: FnOnce(&glib::MainContext)>(body: F) {
        let _serial = MAIN_LOOP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().expect("own the default main context");
        while ctx.iteration(false) {} // start from a clean slate
        body(&ctx);
        while ctx.iteration(false) {} // leave it clean for the next test
    }

    #[test]
    fn append_in_batches_applies_first_batch_synchronously() {
        with_main_loop(|_ctx| {
            let out: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&out);
            // More than one batch, so a drain is scheduled but has not run yet.
            append_in_batches(
                (0..APPEND_BATCH * 3).collect::<Vec<_>>(),
                Rc::new(Cell::new(1)),
                1,
                move |x| sink.borrow_mut().push(x),
            );
            // Exactly the first batch landed before the loop was ever pumped.
            assert_eq!(out.borrow().len(), APPEND_BATCH);
        });
    }

    #[test]
    fn append_in_batches_drains_every_item_in_order() {
        with_main_loop(|ctx| {
            let total = APPEND_BATCH * 3 + 7; // not a multiple of the batch size
            let out: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&out);
            append_in_batches(
                (0..total).collect::<Vec<_>>(),
                Rc::new(Cell::new(1)),
                1,
                move |x| sink.borrow_mut().push(x),
            );
            // Pump the real main loop until the idle drain is exhausted.
            while out.borrow().len() < total && ctx.iteration(false) {}
            assert_eq!(*out.borrow(), (0..total).collect::<Vec<_>>());
        });
    }

    #[test]
    fn append_in_batches_stops_when_superseded() {
        with_main_loop(|ctx| {
            let out: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&out);
            let gen = Rc::new(Cell::new(1));
            append_in_batches(
                (0..APPEND_BATCH * 10).collect::<Vec<_>>(),
                Rc::clone(&gen),
                1,
                move |x| sink.borrow_mut().push(x),
            );
            assert_eq!(out.borrow().len(), APPEND_BATCH, "only the sync batch so far");
            // A newer load bumps the generation: the in-flight drain must stop
            // instead of appending the remaining stale items.
            gen.set(2);
            while ctx.iteration(false) {}
            assert_eq!(
                out.borrow().len(),
                APPEND_BATCH,
                "no items appended past the first batch after supersede"
            );
        });
    }

    #[test]
    fn append_in_batches_small_input_schedules_no_idle() {
        with_main_loop(|ctx| {
            let out: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&out);
            append_in_batches(
                (0..10).collect::<Vec<_>>(),
                Rc::new(Cell::new(1)),
                1,
                move |x| sink.borrow_mut().push(x),
            );
            assert_eq!(out.borrow().len(), 10, "all applied synchronously");
            // Nothing was scheduled: pumping the loop appends no further items.
            while ctx.iteration(false) {}
            assert_eq!(out.borrow().len(), 10, "no idle drain was scheduled");
        });
    }
}
