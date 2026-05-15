use gtk4::prelude::*;
use gtk4::{
    Box, FlowBox, FlowBoxChild, Label, Orientation,
    ScrolledWindow, Align, SelectionMode,
};
use libadwaita as adw;
use crate::library::Artist;

pub struct ArtistsView {
    pub root: ScrolledWindow,
    pub flow: FlowBox,
}

impl ArtistsView {
    pub fn new() -> Self {
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

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&flow));

        Self { root: scroll, flow }
    }

    pub fn load_artists(&self, artists: Vec<Artist>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        for artist in &artists {
            self.flow.append(&make_artist_card(artist));
        }
    }
}

fn make_artist_card(artist: &Artist) -> FlowBoxChild {
    let vbox = Box::new(Orientation::Vertical, 6);
    vbox.set_margin_bottom(8);
    vbox.set_width_request(140);

    let avatar = adw::Avatar::new(80, Some(&artist.name), true);
    avatar.set_halign(Align::Center);

    let lbl_name = Label::new(Some(&artist.name));
    lbl_name.add_css_class("body");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_max_width_chars(16);
    lbl_name.set_halign(Align::Center);

    let info_str = format!(
        "{} álbum{} · {} canción{}",
        artist.album_count,
        if artist.album_count == 1 { "" } else { "es" },
        artist.track_count,
        if artist.track_count == 1 { "" } else { "es" },
    );
    let lbl_info = Label::new(Some(&info_str));
    lbl_info.add_css_class("dim-label");
    lbl_info.add_css_class("caption");
    lbl_info.set_halign(Align::Center);

    vbox.append(&avatar);
    vbox.append(&lbl_name);
    vbox.append(&lbl_info);

    let child = FlowBoxChild::new();
    child.set_child(Some(&vbox));
    child
}
