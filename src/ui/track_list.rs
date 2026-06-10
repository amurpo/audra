//! Unified, reusable track list.
//!
//! Replaces the two divergent implementations the app used to ship: the
//! `ListView` in `library_view` and the `ListBox`+`boxed-list` in
//! `albums_view::make_album_detail_page`. Both call sites now use this module,
//! so row visuals, highlight behaviour and width clamping live in one place.
//!
//! Visuals are configurable per call site:
//! - **Songs** (global list): `[ title / artist ]  duration`
//! - **Album detail**:        `#   title           duration`
//!
//! The "now playing" indicator reacts through the [`NowPlaying`] bus — no
//! polling timers, no per-detail `glib::timeout_add_local`.
//!
//! Width: the whole list is wrapped in `adw::Clamp` (max 760 px) so on wide
//! monitors it doesn't look like a GNOME-Music-style spreadsheet.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, EventControllerMotion, Image, Label, ListItem, ListView,
    Orientation, Overlay, ScrolledWindow, SignalListItemFactory, SingleSelection, StringList,
};

use crate::i18n::{gettext, ngettext};
use crate::library::Track;
use crate::ui::icons::{self, Icon};
use crate::ui::now_playing::NowPlaying;
use crate::ui::widgets::{content_clamp, play_all_button};

/// Size (px) of the per-row play/pause icon that stands in for the track
/// number while a row is active or hovered.
const ICON_SIZE: i32 = 16;

/// Visual configuration for a single instance of [`TrackList`].
///
/// Defaults to "Songs" layout. Use the named constructors when meaning matters
/// at the call site.
#[derive(Clone, Copy, Debug)]
pub struct TrackListConfig {
    pub show_track_number: bool,
    pub show_artist: bool,
    pub show_duration: bool,
    /// Render the `[N songs]  spacer  [▶ Play all]` row above the list. Both
    /// the Songs view and album-detail pages set this to true so the two
    /// surfaces look identical.
    pub show_action_row: bool,
    /// When the loaded tracks span more than one disc (a dedup-folded
    /// multi-disc release), show a dim "Disc N" header above each disc's
    /// first row and number tracks per disc. Single-disc albums and the
    /// global Songs list are unaffected.
    pub show_disc_headers: bool,
}

impl TrackListConfig {
    /// Global "Songs" list: no number column, shows artist (rows have no album
    /// context), shows duration.
    pub fn songs() -> Self {
        Self {
            show_track_number: false,
            show_artist: true,
            show_duration: true,
            show_action_row: true,
            show_disc_headers: false,
        }
    }
    /// Album detail: shows the track number (artist is implied by the page).
    pub fn album_detail() -> Self {
        Self {
            show_track_number: true,
            show_artist: false,
            show_duration: true,
            show_action_row: true,
            show_disc_headers: true,
        }
    }
}

type ActivateCb = Rc<RefCell<Option<Rc<dyn Fn(Vec<Track>, usize)>>>>;
type PlayAllCb = Rc<RefCell<Option<Rc<dyn Fn(Vec<Track>)>>>>;

/// Widget data key used to stash each row's last-bound model position, so the
/// "now playing" repaint can resolve a realized row back to its track without
/// holding the recycled [`ListItem`].
const ROW_POS_KEY: &str = "audra-row-pos";

pub struct TrackList {
    /// The widget to embed in the parent container. Already includes
    /// `adw::Clamp` + `ScrolledWindow` + `ListView`.
    pub root: gtk4::Widget,
    model: StringList,
    full_tracks: RefCell<Vec<Track>>,
    displayed: Rc<RefCell<Vec<Track>>>,
    /// True when the displayed tracks span more than one disc — the gate for
    /// the per-disc headers and per-disc numbering. Recomputed on every
    /// `load`/`filter`; shared with the factory closures.
    multi_disc: Rc<Cell<bool>>,
    active_filter: RefCell<String>,
    /// `[N songs]` label inside the action row, or `None` when the action row
    /// is disabled. Updated on every `load`/`filter`.
    lbl_count: Option<Label>,
    on_play_all: PlayAllCb,
    on_activate: ActivateCb,
}

impl TrackList {
    pub fn new(cfg: TrackListConfig, now_playing: Rc<NowPlaying>) -> Rc<Self> {
        let displayed: Rc<RefCell<Vec<Track>>> = Rc::new(RefCell::new(Vec::new()));
        let multi_disc: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        // Declared up here (instead of next to `this`) so the per-row icon click
        // handler built in `connect_setup` can capture it.
        let on_activate: ActivateCb = Rc::new(RefCell::new(None));
        let model = StringList::new(&[]);
        let selection = SingleSelection::new(Some(model.clone()));
        let factory = SignalListItemFactory::new();

        {
            let now_playing_setup = Rc::clone(&now_playing);
            let displayed_setup = Rc::clone(&displayed);
            let on_activate_setup = Rc::clone(&on_activate);
            let multi_disc_setup = Rc::clone(&multi_disc);
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();

                let row = GtkBox::new(Orientation::Horizontal, 12);
                row.add_css_class("audra-track-row");

                // Left slot: the track number and a play/pause icon button
                // occupy one fixed-width cell so both list layouts line up and
                // the title never shifts when the icon appears on hover. An
                // Overlay (number underneath, icon on top) keeps the cell sized
                // by the number alone — a sibling Box would let the icon's
                // `hexpand` leak out and stretch the column across the row.
                let slot = Overlay::new();
                slot.add_css_class("slot");
                slot.set_valign(Align::Center);

                let num = Label::new(None);
                num.add_css_class("num");
                num.add_css_class("audra-mono");
                num.set_xalign(1.0);
                num.set_valign(Align::Center);
                slot.set_child(Some(&num));

                let icon_btn = Button::new();
                icon_btn.add_css_class("flat");
                icon_btn.add_css_class("row-icon-btn");
                icon_btn.set_halign(Align::End);
                icon_btn.set_valign(Align::Center);
                icon_btn.set_can_focus(false);
                icon_btn.set_child(Some(&icons::image(Icon::Play, ICON_SIZE)));
                icon_btn.set_visible(false);
                slot.add_overlay(&icon_btn);

                row.append(&slot);

                let info = GtkBox::new(Orientation::Vertical, 2);
                info.set_hexpand(true);
                info.set_valign(Align::Center);

                let title = Label::new(None);
                title.add_css_class("title");
                title.set_xalign(0.0);
                title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                info.append(&title);

                let artist = Label::new(None);
                artist.add_css_class("artist");
                artist.set_xalign(0.0);
                artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                artist.set_visible(cfg.show_artist);
                info.append(&artist);

                row.append(&info);

                let dur = Label::new(None);
                dur.add_css_class("dur");
                dur.add_css_class("audra-mono");
                dur.set_valign(Align::Center);
                dur.set_xalign(1.0);
                dur.set_width_chars(5);
                dur.set_visible(cfg.show_duration);
                row.append(&dur);

                // Disc header: a dim "Disc N" caption above the row, shown by
                // `connect_bind` only on the first row of each disc of a
                // multi-disc release. It lives *inside* the item (wrapper Box)
                // so the ListView positions keep their 1:1 mapping with the
                // displayed tracks — activation indices and the now-playing
                // repaint are untouched.
                if cfg.show_disc_headers {
                    let header = Label::new(None);
                    header.add_css_class("caption");
                    header.add_css_class("dim-label");
                    header.set_xalign(0.0);
                    header.set_margin_top(10);
                    header.set_margin_bottom(2);
                    header.set_margin_start(12);
                    header.set_visible(false);

                    let wrapper = GtkBox::new(Orientation::Vertical, 0);
                    wrapper.append(&header);
                    wrapper.append(&row);
                    item.set_child(Some(&wrapper));
                } else {
                    item.set_child(Some(&row));
                }

                // Hover swaps the number for a ▶ on non-playing rows. The hover
                // state lives in a CSS class so the recycling `connect_bind` can
                // re-derive it. WeakRefs break the ListItem→row→controller→closure
                // ownership cycle that would otherwise leak every row.
                let motion = EventControllerMotion::new();
                {
                    let row_weak = row.downgrade();
                    let item_weak = item.downgrade();
                    let np = Rc::clone(&now_playing_setup);
                    let disp = Rc::clone(&displayed_setup);
                    let multi = Rc::clone(&multi_disc_setup);
                    motion.connect_enter(move |_, _, _| {
                        let (Some(row), Some(item)) = (row_weak.upgrade(), item_weak.upgrade())
                        else {
                            return;
                        };
                        row.add_css_class("row-hover");
                        let per_disc = cfg.show_disc_headers && multi.get();
                        repaint_slot_from_item(
                            &row,
                            &item,
                            &np,
                            &disp,
                            cfg.show_track_number,
                            per_disc,
                        );
                    });
                }
                {
                    let row_weak = row.downgrade();
                    let item_weak = item.downgrade();
                    let np = Rc::clone(&now_playing_setup);
                    let disp = Rc::clone(&displayed_setup);
                    let multi = Rc::clone(&multi_disc_setup);
                    motion.connect_leave(move |_| {
                        let (Some(row), Some(item)) = (row_weak.upgrade(), item_weak.upgrade())
                        else {
                            return;
                        };
                        row.remove_css_class("row-hover");
                        let per_disc = cfg.show_disc_headers && multi.get();
                        repaint_slot_from_item(
                            &row,
                            &item,
                            &np,
                            &disp,
                            cfg.show_track_number,
                            per_disc,
                        );
                    });
                }
                row.add_controller(motion);

                // Single click on the icon: toggle play/pause when it's the active
                // track, otherwise start it (same as activating the row).
                {
                    let item_weak = item.downgrade();
                    let np = Rc::clone(&now_playing_setup);
                    let disp = Rc::clone(&displayed_setup);
                    let on_activate = Rc::clone(&on_activate_setup);
                    icon_btn.connect_clicked(move |_| {
                        let Some(item) = item_weak.upgrade() else {
                            return;
                        };
                        let pos = item.position();
                        if pos == gtk4::INVALID_LIST_POSITION {
                            return;
                        }
                        let pos = pos as usize;
                        let cloned = {
                            let tracks = disp.borrow();
                            let Some(track) = tracks.get(pos) else { return };
                            if np.current().as_deref() == Some(track.path.as_str()) {
                                np.request_toggle();
                                return;
                            }
                            tracks.clone()
                        };
                        if let Some(cb) = on_activate.borrow().as_ref() {
                            cb(cloned, pos);
                        }
                    });
                }
            });
        }

        {
            let displayed_ref = Rc::clone(&displayed);
            let now_playing_ref = Rc::clone(&now_playing);
            let multi_disc_ref = Rc::clone(&multi_disc);
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let pos = item.position() as usize;
                let disp = displayed_ref.borrow();
                let Some(track) = disp.get(pos) else { return };

                let Some(child) = item.child() else { return };
                let Some(row) = track_row_of(&child) else {
                    return;
                };
                let per_disc = cfg.show_disc_headers && multi_disc_ref.get();
                if cfg.show_disc_headers {
                    if let Some(header) = child
                        .downcast_ref::<GtkBox>()
                        .and_then(|w| w.first_child())
                        .and_downcast::<Label>()
                    {
                        let starts_disc = pos == 0
                            || disp.get(pos - 1).map(|p| p.disc_num) != Some(track.disc_num);
                        let show = per_disc && starts_disc;
                        if show {
                            header.set_text(&format!(
                                "{} {}",
                                gettext("Disc"),
                                track.disc_num.unwrap_or(1)
                            ));
                        }
                        header.set_visible(show);
                    }
                }
                // Stash the bound position so the bus-driven repaint can resolve
                // this realized row back to its track without the ListItem.
                unsafe { row.set_data(ROW_POS_KEY, pos) };
                let Some(slot) = row.first_child().and_downcast::<Overlay>() else {
                    return;
                };
                let Some(info) = slot.next_sibling().and_downcast::<GtkBox>() else {
                    return;
                };
                let Some(title_lbl) = info.first_child().and_downcast::<Label>() else {
                    return;
                };
                let Some(artist_lbl) = title_lbl.next_sibling().and_downcast::<Label>() else {
                    return;
                };
                let Some(dur_lbl) = row.last_child().and_downcast::<Label>() else {
                    return;
                };

                title_lbl.set_text(&track.display_title());
                if cfg.show_artist {
                    artist_lbl.set_text(&track.display_artist());
                }
                if cfg.show_duration {
                    dur_lbl.set_text(&track.duration_str());
                }

                paint_slot(
                    &row,
                    &now_playing_ref,
                    &track.path,
                    cfg.show_track_number,
                    display_no(track, pos, per_disc),
                );
            });
        }

        let list_view = ListView::new(Some(selection), Some(factory));
        list_view.add_css_class("audra-track-list");

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        // Belt-and-suspenders clip: the outer `.audra-list-card` already
        // has overflow:Hidden + border-radius, but GTK sometimes lets
        // row labels paint a sliver outside the scrolled viewport when a
        // row is half-visible at the edge during scroll. Clipping at the
        // ScrolledWindow level too keeps the duration from "leaking"
        // below the card's bottom edge.
        scroll.set_overflow(gtk4::Overflow::Hidden);
        scroll.set_child(Some(&list_view));

        // Bottom-edge fade overlay: a non-interactive Box pinned to the
        // bottom of the card, painted with a vertical gradient that goes
        // from transparent at the top to the card's tint color at the
        // bottom. Partial rows scrolling past the edge visually dissolve
        // into the card instead of being chopped by the rounded corner.
        let fade = GtkBox::new(Orientation::Vertical, 0);
        fade.add_css_class("audra-list-fade");
        fade.set_valign(Align::End);
        fade.set_halign(Align::Fill);
        fade.set_height_request(28);
        fade.set_can_target(false);

        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&scroll));
        scroll_overlay.add_overlay(&fade);

        // Soft card around the list: a tinted, rounded container. Overflow is
        // clipped so the scrollbar lives inside the rounded corners.
        let card = GtkBox::new(Orientation::Vertical, 0);
        card.add_css_class("audra-list-card");
        card.set_overflow(gtk4::Overflow::Hidden);
        card.set_vexpand(true);
        card.set_hexpand(true);
        card.append(&scroll_overlay);

        // Clamp keeps the list from spreading across the whole window on wide
        // monitors — the "GNOME Music tabla ancha" look we want to avoid.
        let clamp = content_clamp();
        clamp.set_child(Some(&card));

        let outer = GtkBox::new(Orientation::Vertical, 0);
        outer.set_vexpand(true);
        outer.set_hexpand(true);

        // Optional action row `[N songs]  spacer  [▶ Play all]`, wrapped in
        // the same Clamp parameters as the list so it anchors to the same
        // right edge — this is what makes Songs view and album-detail look
        // identical.
        let on_play_all: PlayAllCb = Rc::new(RefCell::new(None));
        let lbl_count: Option<Label> = if cfg.show_action_row {
            let action_bar = GtkBox::new(Orientation::Horizontal, 8);
            action_bar.set_margin_top(8);
            action_bar.set_margin_bottom(6);
            action_bar.set_margin_start(4);
            action_bar.set_margin_end(4);

            let lbl = Label::new(None);
            lbl.add_css_class("heading");
            lbl.add_css_class("dim-label");
            lbl.set_xalign(0.0);
            lbl.set_valign(Align::Center);

            let spacer = GtkBox::new(Orientation::Horizontal, 0);
            spacer.set_hexpand(true);

            let btn = play_all_button(&gettext("Play all"));
            {
                let on_play_all_c = Rc::clone(&on_play_all);
                let displayed_c = Rc::clone(&displayed);
                btn.connect_clicked(move |_| {
                    let tracks = displayed_c.borrow().clone();
                    if let Some(cb) = on_play_all_c.borrow().as_ref() {
                        cb(tracks);
                    }
                });
            }

            action_bar.append(&lbl);
            action_bar.append(&spacer);
            action_bar.append(&btn);

            let action_clamp = content_clamp();
            action_clamp.set_child(Some(&action_bar));
            outer.append(&action_clamp);

            Some(lbl)
        } else {
            None
        };

        outer.append(&clamp);

        let this = Rc::new(Self {
            root: outer.upcast(),
            model,
            full_tracks: RefCell::new(Vec::new()),
            displayed: Rc::clone(&displayed),
            multi_disc: Rc::clone(&multi_disc),
            active_filter: RefCell::new(String::new()),
            on_activate: Rc::clone(&on_activate),
            lbl_count,
            on_play_all,
        });
        this.refresh_count();

        // Forward row activation to the user callback with the *displayed*
        // slice (filtered view), keeping the index space consistent with what
        // the user is actually seeing.
        {
            let on_activate_c = Rc::clone(&on_activate);
            let displayed_c = Rc::clone(&displayed);
            list_view.connect_activate(move |_, idx| {
                let tracks = displayed_c.borrow().clone();
                if let Some(cb) = on_activate_c.borrow().as_ref() {
                    cb(tracks, idx as usize);
                }
            });
        }

        // React to "now playing" changes via the bus by repainting the rows in
        // place. The listener holds only a *widget* WeakRef (plus shared Rc
        // clones), never the `Rc<TrackList>` — call sites like the album-detail
        // page drop the `Rc<TrackList>` as soon as they've appended `.root`, so
        // a `Weak<Self>` listener would die immediately and never fire. Tying the
        // listener's life to the list widget instead keeps it alive exactly as
        // long as the row it repaints, and `false` drops it once the widget is
        // gone (page popped / view rebuilt).
        {
            let lv_weak = list_view.downgrade();
            let displayed_c = Rc::clone(&displayed);
            let np_c = Rc::clone(&now_playing);
            let multi_c = Rc::clone(&multi_disc);
            now_playing.subscribe(move |_path| {
                let Some(lv) = lv_weak.upgrade() else {
                    return false;
                };
                let per_disc = cfg.show_disc_headers && multi_c.get();
                repaint_now_playing(&lv, &displayed_c, &np_c, cfg.show_track_number, per_disc);
                true
            });
        }

        this
    }

    /// Replace the underlying data set. Reapplies the active filter if any.
    pub fn load(&self, tracks: Vec<Track>) {
        *self.full_tracks.borrow_mut() = tracks;
        let filter = self.active_filter.borrow().clone();
        self.apply_filter(&filter);
    }

    pub fn filter(&self, query: &str) {
        *self.active_filter.borrow_mut() = query.to_string();
        self.apply_filter(query);
    }

    pub fn set_on_activate(&self, cb: impl Fn(Vec<Track>, usize) + 'static) {
        *self.on_activate.borrow_mut() = Some(Rc::new(cb));
    }

    /// Set the callback fired by the "Play all" button. No-op if the config
    /// disables the action row.
    pub fn set_on_play_all(&self, cb: impl Fn(Vec<Track>) + 'static) {
        *self.on_play_all.borrow_mut() = Some(Rc::new(cb));
    }

    fn refresh_count(&self) {
        if let Some(ref lbl) = self.lbl_count {
            let n = self.displayed.borrow().len();
            let text = format!("{} {}", n, ngettext("song", "songs", n as u32));
            lbl.set_text(&text);
        }
    }

    fn apply_filter(&self, query: &str) {
        let displayed: Vec<Track> = if query.is_empty() {
            self.full_tracks.borrow().clone()
        } else {
            let q = query.to_lowercase();
            self.full_tracks
                .borrow()
                .iter()
                .filter(|t| {
                    t.display_title().to_lowercase().contains(&q)
                        || t.display_artist().to_lowercase().contains(&q)
                        || t.display_album().to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        };
        let n = self.model.n_items();
        let additions: Vec<&str> = displayed.iter().map(|_| "").collect();
        let discs: std::collections::HashSet<i64> =
            displayed.iter().map(|t| t.disc_num.unwrap_or(1)).collect();
        self.multi_disc.set(discs.len() > 1);
        *self.displayed.borrow_mut() = displayed;
        self.model.splice(0, n, &additions);
        self.refresh_count();
    }
}

/// Resolve the `audra-track-row` Box from a list item's child, which is either
/// the row itself or (with disc headers enabled) a vertical wrapper holding
/// `[header label, row]`.
fn track_row_of(child: &gtk4::Widget) -> Option<GtkBox> {
    let b = child.clone().downcast::<GtkBox>().ok()?;
    if b.has_css_class("audra-track-row") {
        return Some(b);
    }
    b.last_child()
        .and_downcast::<GtkBox>()
        .filter(|r| r.has_css_class("audra-track-row"))
}

/// The number painted in a row's left slot: the position within the list, or
/// the track's own number when a multi-disc release is shown with per-disc
/// headers (so each disc reads 1, 2, 3… under its header).
fn display_no(track: &Track, pos: usize, per_disc: bool) -> usize {
    if per_disc {
        track.track_num.map(|n| n as usize).unwrap_or(pos + 1)
    } else {
        pos + 1
    }
}

// Listener cleanup is automatic: when the list widget is destroyed (page
// popped, view rebuilt) its WeakRef stops upgrading and the listener gets
// removed on the next publish — no explicit unsubscribe needed.

/// Repaint the now-playing indicator on every realized row of `list_view`, in
/// place. Driven by the [`NowPlaying`] bus.
///
/// We walk the realized rows and re-derive each one's slot directly instead of
/// splicing the model: a splice would reset the scroll to the top (jarring when
/// you double-click a track far down the list) and only *schedules* a rebind,
/// which GTK flushes on the next frame — so on auto-advance, with no pointer
/// motion to drive a frame, the active row stayed visually "stuck". Painting the
/// rows here is immediate and scroll-preserving; rows that are still off-screen
/// get the right state from `connect_bind` when they're scrolled into view.
fn repaint_now_playing(
    list_view: &ListView,
    displayed: &RefCell<Vec<Track>>,
    np: &NowPlaying,
    show_num: bool,
    per_disc: bool,
) {
    let disp = displayed.borrow();
    let mut child = list_view.first_child();
    while let Some(item_widget) = child {
        child = item_widget.next_sibling();
        let Some(row) = item_widget.first_child().as_ref().and_then(track_row_of) else {
            continue;
        };
        // Position stashed by `connect_bind`; resolves the recycled row back to
        // its track without holding the ListItem.
        let Some(pos) = (unsafe { row.data::<usize>(ROW_POS_KEY) }) else {
            continue;
        };
        let pos = unsafe { *pos.as_ref() };
        if let Some(track) = disp.get(pos) {
            paint_slot(&row, np, &track.path, show_num, display_no(track, pos, per_disc));
        }
    }
}

/// Paint the left slot (number ⇄ play/pause icon) for one row, deriving the
/// state from the [`NowPlaying`] bus and the row's `row-hover` CSS class:
///
/// - **active row** → play/pause icon showing the *action* (⏸ while playing,
///   ▶ while paused), plus the `playing` class for the row highlight;
/// - **hovered, not active** → ▶ icon (click starts the track);
/// - **otherwise** → the track number (album view) or nothing (Songs view).
fn paint_slot(row: &GtkBox, np: &NowPlaying, path: &str, show_num: bool, track_no: usize) {
    let Some(slot) = row.first_child().and_downcast::<Overlay>() else {
        return;
    };
    let Some(num_lbl) = slot.first_child().and_downcast::<Label>() else {
        return;
    };
    let Some(icon_btn) = num_lbl.next_sibling().and_downcast::<Button>() else {
        return;
    };
    let Some(icon_img) = icon_btn.child().and_downcast::<Image>() else {
        return;
    };

    let set_icon = |icon: Icon| {
        icons::set_image_icon(
            &icon_img,
            icon,
            ICON_SIZE,
            &icons::foreground_color(&icon_img),
        );
    };
    let is_active = np.current().as_deref() == Some(path);

    if is_active {
        row.add_css_class("playing");
        set_icon(if np.is_playing() {
            Icon::Pause
        } else {
            Icon::Play
        });
        num_lbl.set_visible(false);
        icon_btn.set_visible(true);
    } else {
        row.remove_css_class("playing");
        if row.has_css_class("row-hover") {
            set_icon(Icon::Play);
            num_lbl.set_visible(false);
            icon_btn.set_visible(true);
        } else {
            let num_text = if show_num {
                track_no.to_string()
            } else {
                String::new()
            };
            num_lbl.set_text(&num_text);
            num_lbl.set_visible(true);
            icon_btn.set_visible(false);
        }
    }
}

/// Resolve `item`'s track from `displayed` and repaint its slot. Used by the
/// hover controllers, which only carry the recycled [`ListItem`].
fn repaint_slot_from_item(
    row: &GtkBox,
    item: &ListItem,
    np: &NowPlaying,
    displayed: &RefCell<Vec<Track>>,
    show_num: bool,
    per_disc: bool,
) {
    let pos = item.position();
    if pos == gtk4::INVALID_LIST_POSITION {
        return;
    }
    let disp = displayed.borrow();
    if let Some(track) = disp.get(pos as usize) {
        paint_slot(
            row,
            np,
            &track.path,
            show_num,
            display_no(track, pos as usize, per_disc),
        );
    }
}
