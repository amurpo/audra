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
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Label, ListItem, ListView, Orientation, Overlay, ScrolledWindow,
    SignalListItemFactory, SingleSelection, StringList,
};

use crate::i18n::{gettext, ngettext};
use crate::library::Track;
use crate::ui::now_playing::NowPlaying;
use crate::ui::widgets::{content_clamp, play_all_button};

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
        }
    }
    /// Album detail: shows the track number (artist is implied by the page).
    pub fn album_detail() -> Self {
        Self {
            show_track_number: true,
            show_artist: false,
            show_duration: true,
            show_action_row: true,
        }
    }
}

type ActivateCb = Rc<RefCell<Option<Rc<dyn Fn(Vec<Track>, usize)>>>>;
type PlayAllCb = Rc<RefCell<Option<Rc<dyn Fn(Vec<Track>)>>>>;

pub struct TrackList {
    /// The widget to embed in the parent container. Already includes
    /// `adw::Clamp` + `ScrolledWindow` + `ListView`.
    pub root: gtk4::Widget,
    model: StringList,
    full_tracks: RefCell<Vec<Track>>,
    displayed: Rc<RefCell<Vec<Track>>>,
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
        let model = StringList::new(&[]);
        let selection = SingleSelection::new(Some(model.clone()));
        let factory = SignalListItemFactory::new();

        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<ListItem>().unwrap();

            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.add_css_class("audra-track-row");

            let num = Label::new(None);
            num.add_css_class("num");
            num.add_css_class("audra-mono");
            num.set_xalign(1.0);
            num.set_valign(Align::Center);
            num.set_visible(cfg.show_track_number);
            row.append(&num);

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

            item.set_child(Some(&row));
        });

        {
            let displayed_ref = Rc::clone(&displayed);
            let now_playing_ref = Rc::clone(&now_playing);
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let pos = item.position() as usize;
                let disp = displayed_ref.borrow();
                let Some(track) = disp.get(pos) else { return };

                let Some(row) = item.child().and_downcast::<GtkBox>() else {
                    return;
                };
                let Some(num_lbl) = row.first_child().and_downcast::<Label>() else {
                    return;
                };
                let Some(info) = num_lbl.next_sibling().and_downcast::<GtkBox>() else {
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

                if cfg.show_track_number {
                    num_lbl.set_text(&(pos + 1).to_string());
                }
                title_lbl.set_text(&track.display_title());
                if cfg.show_artist {
                    artist_lbl.set_text(&track.display_artist());
                }
                if cfg.show_duration {
                    dur_lbl.set_text(&track.duration_str());
                }

                let is_playing = now_playing_ref
                    .current()
                    .as_deref()
                    .is_some_and(|p| p == track.path);
                if is_playing {
                    row.add_css_class("playing");
                } else {
                    row.remove_css_class("playing");
                }
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

        let on_activate: ActivateCb = Rc::new(RefCell::new(None));
        let this = Rc::new(Self {
            root: outer.upcast(),
            model,
            full_tracks: RefCell::new(Vec::new()),
            displayed: Rc::clone(&displayed),
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

        // React to "now playing" changes via the bus: refresh the indicator on
        // visible rows. Listener returns `false` once the TrackList is gone, so
        // the bus can drop it automatically.
        {
            let weak: Weak<Self> = Rc::downgrade(&this);
            now_playing.subscribe(move |_path| {
                if let Some(tl) = weak.upgrade() {
                    tl.refresh_playing_indicator();
                    true
                } else {
                    false
                }
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

    /// Force `connect_bind` to re-run on the visible rows so the `.playing`
    /// class follows the bus value. `StringList` does not expose
    /// `items_changed` directly, so we splice the model with itself — the rows
    /// are recycled (no widget tear-down), only the bind callback re-runs.
    fn refresh_playing_indicator(&self) {
        let n = self.model.n_items();
        if n == 0 {
            return;
        }
        let empty: Vec<&str> = (0..n).map(|_| "").collect();
        self.model.splice(0, n, &empty);
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
        *self.displayed.borrow_mut() = displayed;
        self.model.splice(0, n, &additions);
        self.refresh_count();
    }
}

// Listener cleanup is automatic: when the last `Rc<TrackList>` is dropped the
// bus's `Weak::upgrade` starts returning `None` and the listener gets removed
// on the next publish — no explicit unsubscribe needed.
