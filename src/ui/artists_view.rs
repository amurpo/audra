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
    gdk, Align, Box as GtkBox, FlowBox, FlowBoxChild, GridView, Label, ListItem, NoSelection,
    Orientation, ScrolledWindow, SelectionMode, SignalListItemFactory, StringList,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub const AVATAR_SIZE: i32 = 152;

/// Widget data key: each realized cell stashes its last-bound model position so
/// the async-photo repaint can resolve a recycled cell back to its artist
/// without holding the `ListItem`. Mirrors `albums_view`'s `POS_KEY`.
const POS_KEY: &str = "audra-artist-pos";

type PlayCallback = Box<dyn Fn(Vec<Track>, usize)>;
type PlayCb = Rc<RefCell<Option<PlayCallback>>>;

pub struct ArtistsView {
    pub root: adw::NavigationView,
    grid: GridView,
    /// "Dumb" model: one empty placeholder per displayed artist. It only carries
    /// the item count / position; the real data lives in `artists_data`, indexed
    /// through `displayed`. Same trick as `albums_view`.
    model: StringList,
    /// Every artist, unfiltered — the source of truth for re-filtering and the
    /// backing store the factory resolves positions against.
    artists_data: Rc<RefCell<Vec<Artist>>>,
    /// Indices into `artists_data`, in grid order — the filtered view. Storing
    /// indices (not cloned `Artist`s) keeps each keystroke cheap.
    displayed: Rc<RefCell<Vec<usize>>>,
    /// Lowercased artist name per `artists_data` entry, in the same order.
    /// Precomputed on load so filtering is one `contains` per artist.
    search_keys: Rc<RefCell<Vec<String>>>,
    /// Decoded artist photos keyed by artist name — *data, not widgets*, so they
    /// survive widget recycling. The factory reads them on bind; the async fetch
    /// inserts here and repaints the realized cells.
    photo_textures: Rc<RefCell<HashMap<String, gdk::Texture>>>,
    all_albums: Rc<RefCell<Vec<Album>>>,
    on_play: PlayCb,
    current_filter: Rc<RefCell<String>>,
}

impl ArtistsView {
    pub fn new(db: Arc<Mutex<Database>>, now_playing: Rc<NowPlaying>) -> Self {
        let model = StringList::new(&[]);
        // NoSelection: clicking a card navigates straight to the artist detail,
        // so there's no useful "selected artist" state — and no selection chrome
        // painted over the photo. `activate` still fires with single click.
        let selection = NoSelection::new(Some(model.clone()));
        let factory = SignalListItemFactory::new();

        let artists_data: Rc<RefCell<Vec<Artist>>> = Rc::new(RefCell::new(Vec::new()));
        let displayed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let search_keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let photo_textures: Rc<RefCell<HashMap<String, gdk::Texture>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let all_albums: Rc<RefCell<Vec<Album>>> = Rc::new(RefCell::new(Vec::new()));
        let on_play: PlayCb = Rc::new(RefCell::new(None));
        let current_filter = Rc::new(RefCell::new(String::new()));

        // --- setup: build one empty, reusable artist card + its right-click menu ---
        {
            let displayed_s = Rc::clone(&displayed);
            let artists_s = Rc::clone(&artists_data);
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let (vbox, _avatar, _name, _info) = build_artist_card();
                item.set_child(Some(&vbox));

                // Right-click → photo menu. The cell is recycled across artists,
                // so the menu can't capture the artist here; it resolves the one
                // bound *right now* from `displayed[position]` at click time.
                let gesture = gtk4::GestureClick::new();
                gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
                let item_weak = item.downgrade();
                let vbox_weak = vbox.downgrade();
                let displayed_g = Rc::clone(&displayed_s);
                let artists_g = Rc::clone(&artists_s);
                gesture.connect_pressed(move |g, _, x, y| {
                    let (Some(item), Some(vbox)) = (item_weak.upgrade(), vbox_weak.upgrade())
                    else {
                        return;
                    };
                    let pos = item.position();
                    if pos == gtk4::INVALID_LIST_POSITION {
                        return;
                    }
                    let Some(&idx) = displayed_g.borrow().get(pos as usize) else {
                        return;
                    };
                    let name = {
                        let data = artists_g.borrow();
                        let Some(a) = data.get(idx) else { return };
                        a.name.clone()
                    };
                    let Some((avatar, _, _)) = artist_card_parts(vbox.upcast_ref()) else {
                        return;
                    };
                    g.set_state(gtk4::EventSequenceState::Claimed);
                    crate::ui::cover_picker::show_artist_photo_menu(&vbox, x, y, name, avatar);
                });
                vbox.add_controller(gesture);
            });
        }

        // --- bind: fill the recycled card from `artists_data[displayed[pos]]` ---
        {
            let displayed_b = Rc::clone(&displayed);
            let artists_b = Rc::clone(&artists_data);
            let textures_b = Rc::clone(&photo_textures);
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let pos = item.position() as usize;
                // The item's child IS the card vbox directly (see `connect_setup`).
                let Some(vbox) = item.child() else { return };
                let Some((avatar, name_lbl, info_lbl)) = artist_card_parts(&vbox) else {
                    return;
                };

                let idx = match displayed_b.borrow().get(pos) {
                    Some(&i) => i,
                    None => return,
                };
                let data = artists_b.borrow();
                let Some(artist) = data.get(idx) else { return };

                // Feed the name to the Avatar too: while the photo is missing or
                // still loading it renders this artist's generated initials.
                avatar.set_text(Some(&artist.name));
                name_lbl.set_text(&artist.name);
                info_lbl.set_text(&artist_info(artist));

                // Stash the bound position on the vbox (the item's direct child)
                // for the async-photo repaint to resolve this cell.
                unsafe { vbox.set_data(POS_KEY, pos) };

                // Apply a cached photo, or reset to initials: a recycled cell may
                // still show another artist's photo, so the `None` arm must clear
                // it rather than leaving the stale image behind.
                match textures_b.borrow().get(&artist.name) {
                    Some(texture) => avatar.set_custom_image(Some(texture)),
                    None => avatar.set_custom_image(None::<&gdk::Texture>),
                }
            });
        }

        let grid = GridView::new(Some(selection), Some(factory));
        grid.add_css_class("audra-artist-grid");
        // Same packing range as the old FlowBox (2–8 columns).
        grid.set_min_columns(2);
        grid.set_max_columns(8);
        grid.set_single_click_activate(true);

        // GridView implements GtkScrollable, so — exactly like the Albums grid and
        // the ListView in track_list — it MUST be the direct child of the
        // ScrolledWindow. The scroll machinery then virtualizes it and each row
        // stays at the card's natural height. Wrapping it in the Clamp first
        // (Clamp inside the scroll) is what broke every earlier album attempt: the
        // scroll wraps the Clamp in a Viewport, the GridView stops being the
        // scrollable child, and every row stretches to the viewport height. So:
        // grid → ScrolledWindow, and the width Clamp goes on the OUTSIDE.
        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&grid));

        // Same Clamp parameters as TrackList / AlbumsView so all surfaces share
        // the same useful width.
        let clamp = content_clamp();
        clamp.set_vexpand(true);
        clamp.set_child(Some(&scroll));

        let nav = adw::NavigationView::new();
        let root_page = adw::NavigationPage::new(&clamp, &gettext("Artists"));
        root_page.set_tag(Some("artists-root"));
        nav.add(&root_page);

        // --- activate: single click opens the artist detail page ---
        {
            let nav_c = nav.clone();
            let displayed_a = Rc::clone(&displayed);
            let artists_a = Rc::clone(&artists_data);
            let albums_a = Rc::clone(&all_albums);
            let on_play_a = Rc::clone(&on_play);
            let now_playing_a = Rc::clone(&now_playing);
            let db_a = Arc::clone(&db);
            grid.connect_activate(move |_, pos| {
                // A fast double-click activates twice; only push while this grid
                // is still the visible page so the second activation can't stack
                // another copy of the detail page.
                if nav_c.visible_page().and_then(|p| p.tag()).as_deref() != Some("artists-root") {
                    return;
                }
                let idx = match displayed_a.borrow().get(pos as usize) {
                    Some(&i) => i,
                    None => return,
                };
                let name = match artists_a.borrow().get(idx) {
                    Some(a) => a.name.clone(),
                    None => return,
                };

                // Two kinds of hits:
                //   * Direct: album.artist == name → keep the whole album.
                //   * Compilation: album.artist != name but some tracks do match
                //     (Various Artists / OSTs) → keep only the artist's tracks
                //     under that album, with the original album name preserved so
                //     the cover still resolves.
                // Both comparisons are case-insensitive so tags like "Comes With
                // The Fall" vs "Comes With the Fall" resolve to the same artist.
                let name_lower = name.to_lowercase();
                let mut artist_albums: Vec<Album> = albums_a
                    .borrow()
                    .iter()
                    .filter_map(|a| {
                        if a.artist.to_lowercase() == name_lower {
                            return Some(a.clone());
                        }
                        let tracks: Vec<Track> = a
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
                    let db_g = db_a.lock().unwrap();
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
                    Rc::clone(&on_play_a),
                    Rc::clone(&now_playing_a),
                    Arc::clone(&db_a),
                );
                nav_c.push(&page);
            });
        }

        Self {
            root: nav,
            grid,
            model,
            artists_data,
            displayed,
            search_keys,
            photo_textures,
            all_albums,
            on_play,
            current_filter,
        }
    }

    pub fn set_on_play(&self, callback: impl Fn(Vec<Track>, usize) + 'static) {
        *self.on_play.borrow_mut() = Some(std::boxed::Box::new(callback));
    }

    pub fn filter(&self, query: &str) {
        *self.current_filter.borrow_mut() = query.to_string();
        self.apply_filter(query);
    }

    /// Recompute `displayed` from `artists_data`/`search_keys` and splice the
    /// model so the grid shows exactly the matching artists. Mirrors
    /// `albums_view::apply_filter`.
    fn apply_filter(&self, query: &str) {
        let displayed: Vec<usize> = if query.is_empty() {
            (0..self.artists_data.borrow().len()).collect()
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

    pub fn load_artists(&self, artists: Vec<Artist>, albums: Vec<Album>) {
        self.photo_textures.borrow_mut().clear();

        let names_to_fetch: Vec<String> = artists.iter().map(|a| a.name.clone()).collect();
        *self.search_keys.borrow_mut() = artists.iter().map(|a| a.name.to_lowercase()).collect();
        *self.artists_data.borrow_mut() = artists;
        *self.all_albums.borrow_mut() = albums;

        // Rebuild the displayed set (honouring any active filter) and the model.
        let active = self.current_filter.borrow().clone();
        self.apply_filter(&active);

        if !names_to_fetch.is_empty() {
            self.start_photo_fetch(names_to_fetch);
        }
    }

    /// Drive the shared two-pass image pipeline for artist photos.
    /// No local source for artist photos (yet), so the fast lane always misses
    /// and everything resolves through Last.fm. Results are cached by name and
    /// repainted into the realized cells, the same way the Albums grid does.
    fn start_photo_fetch(&self, artists: Vec<String>) {
        let textures = Rc::clone(&self.photo_textures);
        let displayed = Rc::clone(&self.displayed);
        let artists_data = Rc::clone(&self.artists_data);
        let grid_weak = self.grid.downgrade();

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
                textures.borrow_mut().insert(artist.clone(), texture);
                // Repaint the realized cells in place: a photo that lands after
                // its cell is already on screen won't get a fresh `bind`, so we
                // paint it here (cells scrolled into view later read the texture
                // from the map on bind).
                if let Some(grid) = grid_weak.upgrade() {
                    repaint_photos(&grid, &displayed, &artists_data, &textures);
                }
            },
        );
    }
}

/// The "N albums · M songs" caption under an artist's name.
fn artist_info(artist: &Artist) -> String {
    format!(
        "{} {} · {} {}",
        artist.album_count,
        ngettext("album", "albums", artist.album_count as u32),
        artist.track_count,
        ngettext("song", "songs", artist.track_count as u32),
    )
}

/// Build one empty artist card (round avatar + name + info), returning the inner
/// widgets so the recycling `GridView` factory can fill them on bind. The `vbox`
/// stays sized to its content (`halign: Center`, `hexpand: false`) so the hover
/// wash on `.artist-card` hugs the card instead of the whole column. The card's
/// inner spacing lives in CSS `padding` on `.artist-card` (not widget margins),
/// so the hover background covers it — matching the old FlowBox, whose card
/// highlight wrapped the vbox margins. Inter-card gaps live on the grid `> child`.
fn build_artist_card() -> (GtkBox, adw::Avatar, Label, Label) {
    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.add_css_class("artist-card");
    vbox.set_halign(Align::Center);
    vbox.set_hexpand(false);

    let avatar = adw::Avatar::new(AVATAR_SIZE, None, true);
    avatar.set_halign(Align::Center);
    avatar.set_valign(Align::Center);

    let lbl_name = Label::new(None);
    lbl_name.add_css_class("body");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_max_width_chars(18);
    lbl_name.set_halign(Align::Center);
    lbl_name.set_justify(gtk4::Justification::Center);

    let lbl_info = Label::new(None);
    lbl_info.add_css_class("dim-label");
    lbl_info.add_css_class("caption");
    lbl_info.set_halign(Align::Center);
    lbl_info.set_justify(gtk4::Justification::Center);

    vbox.append(&avatar);
    vbox.append(&lbl_name);
    vbox.append(&lbl_info);
    (vbox, avatar, lbl_name, lbl_info)
}

/// Re-derive a recycled cell's inner widgets from its vbox. The structure is
/// fixed by [`build_artist_card`]: vbox → [avatar, name label, info label].
/// Mirrors `albums_view::card_parts`.
fn artist_card_parts(vbox: &gtk4::Widget) -> Option<(adw::Avatar, Label, Label)> {
    let avatar = vbox.first_child().and_downcast::<adw::Avatar>()?;
    let name = avatar.next_sibling().and_downcast::<Label>()?;
    let info = name.next_sibling().and_downcast::<Label>()?;
    Some((avatar, name, info))
}

/// Repaint the photo of every realized cell whose artist already has a texture.
/// Driven by the async fetch as results land. Walks the realized cells and
/// resolves each one's artist through the position stashed by `connect_bind`.
/// Mirrors `albums_view::repaint_covers`.
fn repaint_photos(
    grid: &GridView,
    displayed: &RefCell<Vec<usize>>,
    artists: &RefCell<Vec<Artist>>,
    textures: &RefCell<HashMap<String, gdk::Texture>>,
) {
    let disp = displayed.borrow();
    let data = artists.borrow();
    let tex = textures.borrow();
    let mut next = grid.first_child();
    while let Some(widget) = next {
        next = widget.next_sibling();
        // Each realized cell: item widget → card vbox (holds POS_KEY).
        let Some(vbox) = widget.first_child() else {
            continue;
        };
        let Some(pos_ptr) = (unsafe { vbox.data::<usize>(POS_KEY) }) else {
            continue;
        };
        let pos = unsafe { *pos_ptr.as_ref() };
        let Some(&idx) = disp.get(pos) else { continue };
        let Some(artist) = data.get(idx) else {
            continue;
        };
        let Some(texture) = tex.get(&artist.name) else {
            continue;
        };
        let Some((avatar, _, _)) = artist_card_parts(&vbox) else {
            continue;
        };
        avatar.set_custom_image(Some(texture));
    }
}

fn make_artist_detail_page(
    nav: adw::NavigationView,
    artist_name: &str,
    albums: Vec<Album>,
    on_play: PlayCb,
    now_playing: Rc<NowPlaying>,
    db: Arc<Mutex<Database>>,
) -> adw::NavigationPage {
    // No HeaderBar — the back arrow sits inline next to the artist name,
    // matching Songs / album-detail layouts exactly. NavigationPage still
    // keeps the title for accessibility / breadcrumbs.
    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);
    // Match the main Albums grid's airy spacing. This is a left-anchored
    // (`halign: Start`) homogeneous FlowBox, so the gap between covers IS the
    // `column_spacing` directly. The Albums GridView looks airier than its 2px
    // child margin suggests because it stretches its columns to fill the width
    // (~40px effective gap at full width); 40px here reproduces that. Vertical
    // 14px = the GridView's 7px top+bottom child margin.
    flow.set_column_spacing(40);
    flow.set_row_spacing(14);
    flow.set_margin_top(4);
    flow.set_margin_bottom(12);
    flow.set_margin_start(4);
    flow.set_margin_end(4);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(12);
    // Anchor cards top-left like the album/artist GridViews: a homogeneous
    // FlowBox otherwise stretches its few present columns across the full width,
    // so a handful of albums look spread out and centered instead of packed left.
    // This album sub-grid stays a FlowBox (one artist never has thousands of
    // albums), so it still needs the explicit halign the GridViews get for free.
    flow.set_halign(Align::Start);
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
