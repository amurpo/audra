use gtk4::prelude::*;
use gtk4::{
    Box, Label, ListView, Orientation, ScrolledWindow,
    SingleSelection, SignalListItemFactory, ListItem,
    StringList, Align,
};
use std::rc::Rc;
use std::cell::RefCell;
use crate::library::Track;

pub struct LibraryView {
    pub root: ScrolledWindow,
    pub list_view: ListView,
    model: StringList,
    full_tracks: Vec<Track>,
    displayed: Rc<RefCell<Vec<Track>>>,
    current_path: Rc<RefCell<Option<String>>>,
}

impl LibraryView {
    pub fn new(current_path: Rc<RefCell<Option<String>>>) -> Self {
        let displayed: Rc<RefCell<Vec<Track>>> = Rc::new(RefCell::new(Vec::new()));
        let model = StringList::new(&[]);
        let selection = SingleSelection::new(Some(model.clone()));

        let factory = SignalListItemFactory::new();

        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<ListItem>().unwrap();

            let row = Box::new(Orientation::Horizontal, 12);
            row.set_margin_top(6);
            row.set_margin_bottom(6);
            row.set_margin_start(12);
            row.set_margin_end(12);

            let info = Box::new(Orientation::Vertical, 2);
            info.set_hexpand(true);
            info.set_valign(Align::Center);

            let lbl_title = Label::new(None);
            lbl_title.set_xalign(0.0);
            lbl_title.add_css_class("body");
            lbl_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);

            let lbl_artist = Label::new(None);
            lbl_artist.set_xalign(0.0);
            lbl_artist.add_css_class("dim-label");
            lbl_artist.add_css_class("caption");
            lbl_artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);

            info.append(&lbl_title);
            info.append(&lbl_artist);

            let lbl_dur = Label::new(None);
            lbl_dur.add_css_class("dim-label");
            lbl_dur.add_css_class("caption");
            lbl_dur.set_valign(Align::Center);
            lbl_dur.set_width_chars(6);
            lbl_dur.set_xalign(1.0);

            row.append(&info);
            row.append(&lbl_dur);
            item.set_child(Some(&row));
        });

        {
            let displayed_ref = Rc::clone(&displayed);
            let current_path_ref = Rc::clone(&current_path);
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<ListItem>().unwrap();
                let pos = item.position() as usize;
                let disp = displayed_ref.borrow();
                let Some(track) = disp.get(pos) else { return };

                let Some(row) = item.child().and_downcast::<Box>() else { return };
                let Some(info) = row.first_child().and_downcast::<Box>() else { return };
                let Some(lbl_title) = info.first_child().and_downcast::<Label>() else { return };
                let Some(lbl_artist) = lbl_title.next_sibling().and_downcast::<Label>() else { return };
                let Some(lbl_dur) = row.last_child().and_downcast::<Label>() else { return };

                lbl_title.set_text(&track.display_title());
                lbl_artist.set_text(&track.display_artist());
                lbl_dur.set_text(&track.duration_str());

                let is_playing = current_path_ref
                    .borrow()
                    .as_deref()
                    .map_or(false, |p| p == track.path);
                if is_playing {
                    lbl_title.add_css_class("now-playing-title");
                } else {
                    lbl_title.remove_css_class("now-playing-title");
                }
            });
        }

        let list_view = ListView::new(Some(selection), Some(factory));
        list_view.add_css_class("library-list");

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list_view));

        Self {
            root: scroll,
            list_view,
            model,
            full_tracks: Vec::new(),
            displayed,
            current_path,
        }
    }

    pub fn load_tracks(&mut self, tracks: Vec<Track>) {
        self.full_tracks = tracks.clone();
        self.apply_displayed(tracks);
    }

    pub fn filter(&mut self, query: &str) {
        if query.is_empty() {
            let all = self.full_tracks.clone();
            self.apply_displayed(all);
        } else {
            let q = query.to_lowercase();
            let filtered: Vec<Track> = self.full_tracks.iter()
                .filter(|t| {
                    t.display_title().to_lowercase().contains(&q)
                        || t.display_artist().to_lowercase().contains(&q)
                        || t.display_album().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            self.apply_displayed(filtered);
        }
    }

    pub fn set_playing_path(&self, path: Option<&str>) {
        *self.current_path.borrow_mut() = path.map(|s| s.to_string());
        let n = self.model.n_items();
        let empty: Vec<&str> = (0..n).map(|_| "").collect();
        self.model.splice(0, n, &empty);
    }

    fn apply_displayed(&mut self, tracks: Vec<Track>) {
        let n = self.model.n_items();
        let additions: Vec<&str> = tracks.iter().map(|_| "").collect();
        *self.displayed.borrow_mut() = tracks;
        self.model.splice(0, n, &additions);
    }

    pub fn all_tracks(&self) -> Vec<Track> {
        self.displayed.borrow().clone()
    }
}
