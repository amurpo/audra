use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, FlowBox, FlowBoxChild, Image, Label, Orientation, Overlay, Picture,
    ScrolledWindow, Align, ContentFit, SelectionMode,
};
use libadwaita as adw;
use adw::prelude::*;
use gdk_pixbuf;
use std::rc::Rc;
use std::cell::RefCell;
use crate::library::{Artist, Album, Track};

const CARD_SIZE: i32 = 200;

type PlayCallback = Box<dyn Fn(Vec<Track>)>;

pub struct ArtistsView {
    pub root: adw::NavigationView,
    flow: FlowBox,
    artists_list: Rc<RefCell<Vec<Artist>>>,
    all_albums: Rc<RefCell<Vec<Album>>>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
}

impl ArtistsView {
    pub fn new() -> Self {
        let nav = adw::NavigationView::new();

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
        flow.set_activate_on_single_click(true);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&flow));

        let root_page = adw::NavigationPage::new(&scroll, "Artistas");
        root_page.set_tag(Some("artists-root"));
        nav.add(&root_page);

        let artists_list: Rc<RefCell<Vec<Artist>>> = Rc::new(RefCell::new(Vec::new()));
        let all_albums: Rc<RefCell<Vec<Album>>> = Rc::new(RefCell::new(Vec::new()));
        let on_play: Rc<RefCell<Option<PlayCallback>>> = Rc::new(RefCell::new(None));

        {
            let nav_c = nav.clone();
            let artists_c = Rc::clone(&artists_list);
            let albums_c = Rc::clone(&all_albums);
            let on_play_c = Rc::clone(&on_play);

            flow.connect_child_activated(move |_, child| {
                let idx = child.index() as usize;
                let artist_name = artists_c.borrow().get(idx).map(|a| a.name.clone());
                if let Some(name) = artist_name {
                    let artist_albums: Vec<Album> = albums_c
                        .borrow()
                        .iter()
                        .filter(|a| a.artist == name)
                        .cloned()
                        .collect();
                    let page = make_artist_detail_page(&name, artist_albums, Rc::clone(&on_play_c));
                    nav_c.push(&page);
                }
            });
        }

        Self { root: nav, flow, artists_list, all_albums, on_play }
    }

    pub fn set_on_play(&self, callback: impl Fn(Vec<Track>) + 'static) {
        *self.on_play.borrow_mut() = Some(std::boxed::Box::new(callback));
    }

    pub fn load_artists(&self, artists: Vec<Artist>, albums: Vec<Album>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        for artist in &artists {
            self.flow.append(&make_artist_card(artist));
        }
        *self.artists_list.borrow_mut() = artists;
        *self.all_albums.borrow_mut() = albums;
    }
}

fn make_artist_detail_page(
    artist_name: &str,
    albums: Vec<Album>,
    on_play: Rc<RefCell<Option<PlayCallback>>>,
) -> adw::NavigationPage {
    let header = adw::HeaderBar::new();

    let btn_play_all = Button::builder()
        .label("Reproducir todo")
        .css_classes(["suggested-action", "pill"])
        .build();
    header.pack_end(&btn_play_all);

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_column_spacing(3);
    flow.set_row_spacing(3);
    flow.set_margin_top(8);
    flow.set_margin_bottom(8);
    flow.set_margin_start(8);
    flow.set_margin_end(8);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(12);
    flow.set_activate_on_single_click(true);

    for album in &albums {
        flow.append(&make_album_card(album));
    }

    let albums_rc = Rc::new(albums);

    {
        let albums_c = Rc::clone(&albums_rc);
        let on_play_c = Rc::clone(&on_play);
        btn_play_all.connect_clicked(move |_| {
            let all_tracks: Vec<Track> = albums_c
                .iter()
                .flat_map(|a| a.tracks.iter().cloned())
                .collect();
            if let Some(cb) = on_play_c.borrow().as_ref() {
                cb(all_tracks);
            }
        });
    }

    {
        let albums_c = Rc::clone(&albums_rc);
        let on_play_c = Rc::clone(&on_play);
        flow.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            if let Some(album) = albums_c.get(idx) {
                if let Some(cb) = on_play_c.borrow().as_ref() {
                    cb(album.tracks.clone());
                }
            }
        });
    }

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&flow));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));

    adw::NavigationPage::new(&toolbar, artist_name)
}

fn scale_to_texture(data: &[u8], size: i32) -> Option<gtk4::gdk::Texture> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    let _ = loader.write(data);
    let _ = loader.close();
    let src = loader.pixbuf()?;
    let w = src.width();
    let h = src.height();
    let (sw, sh) = if w <= h {
        (size, size * h / w)
    } else {
        (size * w / h, size)
    };
    let scaled = src.scale_simple(sw, sh, gdk_pixbuf::InterpType::Bilinear)?;
    let x = (sw - size) / 2;
    let y = (sh - size) / 2;
    let dest = gdk_pixbuf::Pixbuf::new(src.colorspace(), src.has_alpha(), src.bits_per_sample(), size, size)?;
    scaled.copy_area(x, y, size, size, &dest, 0, 0);
    Some(gtk4::gdk::Texture::for_pixbuf(&dest))
}

fn make_album_card(album: &Album) -> FlowBoxChild {
    let overlay = Overlay::new();
    overlay.set_size_request(CARD_SIZE, CARD_SIZE);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_hexpand(false);
    overlay.set_vexpand(false);

    if let Some(ref data) = album.cover {
        if let Some(texture) = scale_to_texture(data.as_slice(), CARD_SIZE) {
            let picture = Picture::new();
            picture.set_content_fit(ContentFit::Fill);
            picture.set_halign(Align::Fill);
            picture.set_valign(Align::Fill);
            picture.set_paintable(Some(&texture));
            overlay.set_child(Some(&picture));
        } else {
            let ph = Image::from_icon_name("media-optical-symbolic");
            ph.set_pixel_size(64);
            ph.add_css_class("dim-label");
            overlay.set_child(Some(&ph));
        }
    } else {
        let ph = Image::from_icon_name("media-optical-symbolic");
        ph.set_pixel_size(64);
        ph.add_css_class("dim-label");
        overlay.set_child(Some(&ph));
    }

    let info = GtkBox::new(Orientation::Vertical, 1);
    info.set_valign(Align::End);
    info.set_halign(Align::Fill);
    info.add_css_class("album-overlay-box");

    let lbl_name = Label::new(Some(&album.name));
    lbl_name.add_css_class("album-overlay-title");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_xalign(0.0);

    info.append(&lbl_name);
    overlay.add_overlay(&info);

    let child = FlowBoxChild::new();
    child.add_css_class("mosaic-child");
    child.set_child(Some(&overlay));
    child.set_halign(Align::Center);
    child.set_valign(Align::Center);
    child
}

fn make_artist_card(artist: &Artist) -> FlowBoxChild {
    let vbox = GtkBox::new(Orientation::Vertical, 6);
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
    child.add_css_class("artist-card");
    child.set_child(Some(&vbox));
    child
}
