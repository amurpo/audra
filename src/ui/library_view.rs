//! "Songs" view — a global, flat track list.
//!
//! Thin wrapper around [`TrackList`]. The action row (`[N songs]` + "Play
//! all") and the surrounding `adw::Clamp` are owned by `TrackList` itself, so
//! this view and the album-detail page render exactly the same chrome.
//!
//! The big "All your music" title uses the shared [`section_header_label`]
//! helper — same size, same alignment as the album / artist titles on detail
//! pages. The Songs view doesn't carry a back arrow because it isn't
//! navigable, but detail pages put one next to their title.
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box, Orientation};

use crate::i18n::gettext;
use crate::library::Track;
use crate::ui::now_playing::NowPlaying;
use crate::ui::track_list::{TrackList, TrackListConfig};
use crate::ui::widgets::{content_clamp, page_title_row};

pub struct LibraryView {
    pub root: gtk4::Widget,
    list: Rc<TrackList>,
}

impl LibraryView {
    pub fn new(now_playing: Rc<NowPlaying>) -> Self {
        let list = TrackList::new(TrackListConfig::songs(), now_playing);

        // Songs is the root view of its stack and not navigable, but we
        // still build the row with an invisible back slot so the title
        // anchors at the same X / Y as detail pages.
        let title_row = page_title_row(&gettext("All your music"), false);
        let title_clamp = content_clamp();
        title_clamp.set_child(Some(&title_row));

        let outer = Box::new(Orientation::Vertical, 0);
        outer.append(&title_clamp);
        outer.append(&list.root);

        Self {
            root: outer.upcast(),
            list,
        }
    }

    pub fn set_on_play_all(&self, cb: impl Fn(Vec<Track>, usize) + 'static) {
        // The global "Songs" list interprets "Play all" as "play the whole
        // displayed list starting at no specific index"; the album-detail
        // page does the same. Keep the wider `(tracks, idx)` callback shape
        // for the caller and wrap it.
        self.list.set_on_play_all(move |tracks| {
            cb(tracks, usize::MAX);
        });
    }

    /// Fires when the user activates a row (double-click / Enter).
    pub fn set_on_activate(&self, cb: impl Fn(Vec<Track>, usize) + 'static) {
        self.list.set_on_activate(cb);
    }

    pub fn load_tracks(&self, tracks: Vec<Track>) {
        self.list.load(tracks);
    }

    pub fn filter(&self, query: &str) {
        self.list.filter(query);
    }
}
