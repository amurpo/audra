use gtk4::prelude::*;
use gtk4::{
    Box, FlowBox, FlowBoxChild, Label, Orientation, Picture,
    ScrolledWindow, Align, ContentFit, SelectionMode,
};
use libadwaita as adw;
use glib;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::library::Artist;

type ImageMap = Rc<RefCell<HashMap<String, Picture>>>;

pub struct ArtistsView {
    pub root: ScrolledWindow,
    pub flow: FlowBox,
    images: ImageMap,
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

        Self {
            root: scroll,
            flow,
            images: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn load_artists(&self, artists: Vec<Artist>, lastfm_key: Option<String>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.images.borrow_mut().clear();

        let mut need_fetch: Vec<String> = Vec::new();

        for artist in &artists {
            let (card, picture) = make_artist_card(artist);
            self.images.borrow_mut().insert(artist.name.clone(), picture);
            self.flow.append(&card);
            need_fetch.push(artist.name.clone());
        }

        if let Some(key) = lastfm_key {
            if !need_fetch.is_empty() {
                self.start_image_fetch(need_fetch, key);
            }
        }
    }

    fn start_image_fetch(&self, artists: Vec<String>, lastfm_key: String) {
        use std::sync::{Arc, Mutex};
        use std::sync::atomic::{AtomicBool, Ordering};

        let queue: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let queue_tx = Arc::clone(&queue);
        let finished_tx = Arc::clone(&finished);

        std::thread::spawn(move || {
            for artist_name in artists {
                if let Some(bytes) =
                    crate::library::metadata::fetch_artist_image(&artist_name, &lastfm_key)
                {
                    queue_tx.lock().unwrap().push((artist_name, bytes));
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            finished_tx.store(true, Ordering::Relaxed);
        });

        let images = Rc::clone(&self.images);
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let mut q = queue.lock().unwrap();
            for (name, bytes) in q.drain(..) {
                if let Some(picture) = images.borrow().get(&name) {
                    let gbytes = glib::Bytes::from(&bytes);
                    if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&gbytes) {
                        picture.set_paintable(Some(&texture));
                        picture.set_visible(true);
                    }
                }
            }
            drop(q);
            if finished.load(Ordering::Relaxed) {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

fn make_artist_card(artist: &Artist) -> (FlowBoxChild, Picture) {
    let vbox = Box::new(Orientation::Vertical, 6);
    vbox.set_margin_bottom(8);
    vbox.set_width_request(140);

    // Avatar con imagen de artista (reemplazada al llegar del fetch)
    let avatar = adw::Avatar::new(80, Some(&artist.name), true);
    avatar.set_halign(Align::Center);

    // Picture invisible inicialmente — se muestra cuando hay imagen
    let picture = Picture::new();
    picture.set_content_fit(ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_size_request(80, 80);
    picture.add_css_class("artist-image");
    picture.set_visible(false);

    // Contenedor que superpone avatar + picture
    let overlay = gtk4::Overlay::new();
    overlay.set_halign(Align::Center);
    overlay.set_size_request(80, 80);
    overlay.set_child(Some(&avatar));
    overlay.add_overlay(&picture);

    let lbl_name = Label::new(Some(&artist.name));
    lbl_name.add_css_class("body");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_max_width_chars(16);
    lbl_name.set_halign(Align::Center);

    let info_str = format!("{} álbum{} · {} canción{}",
        artist.album_count,
        if artist.album_count == 1 { "" } else { "es" },
        artist.track_count,
        if artist.track_count == 1 { "" } else { "es" },
    );
    let lbl_info = Label::new(Some(&info_str));
    lbl_info.add_css_class("dim-label");
    lbl_info.add_css_class("caption");
    lbl_info.set_halign(Align::Center);

    vbox.append(&overlay);
    vbox.append(&lbl_name);
    vbox.append(&lbl_info);

    let child = FlowBoxChild::new();
    child.set_child(Some(&vbox));

    (child, picture)
}
