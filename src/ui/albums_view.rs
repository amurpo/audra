use gtk4::prelude::*;
use gtk4::{
    FlowBox, FlowBoxChild, Image, Label, Orientation, Overlay, Picture,
    ScrolledWindow, Stack, Align, ContentFit, SelectionMode, StackTransitionType,
};
use glib;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::library::Album;

const CARD_SIZE: i32 = 160;

type CoverMap = Rc<RefCell<HashMap<String, (Stack, Picture)>>>;

pub struct AlbumsView {
    pub root: ScrolledWindow,
    pub flow: FlowBox,
    albums_data: Rc<RefCell<Vec<Album>>>,
    covers: CoverMap,
}

impl AlbumsView {
    pub fn new() -> Self {
        let flow = FlowBox::new();
        flow.set_selection_mode(SelectionMode::Single);
        // homogeneous=false: cada tarjeta mantiene su tamaño natural fijo
        flow.set_homogeneous(false);
        flow.set_column_spacing(3);
        flow.set_row_spacing(3);
        flow.set_margin_top(3);
        flow.set_margin_bottom(3);
        flow.set_margin_start(3);
        flow.set_margin_end(3);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(12);
        flow.set_activate_on_single_click(true);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&flow));

        Self {
            root: scroll,
            flow,
            albums_data: Rc::new(RefCell::new(Vec::new())),
            covers: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn load_albums(&self, albums: Vec<Album>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.covers.borrow_mut().clear();

        let mut need_fetch: Vec<(String, String)> = Vec::new();

        for album in &albums {
            let key = format!("{}|{}", album.artist, album.name);
            let (card, stack, picture) = make_album_card(album);
            self.covers.borrow_mut().insert(key.clone(), (stack, picture));
            self.flow.append(&card);

            if album.cover.is_none() {
                need_fetch.push((album.artist.clone(), album.name.clone()));
            }
        }

        *self.albums_data.borrow_mut() = albums;

        if !need_fetch.is_empty() {
            self.start_cover_fetch(need_fetch);
        }
    }

    pub fn get_album_tracks(&self, idx: usize) -> Vec<crate::library::Track> {
        self.albums_data
            .borrow()
            .get(idx)
            .map(|a| a.tracks.clone())
            .unwrap_or_default()
    }

    fn start_cover_fetch(&self, albums: Vec<(String, String)>) {
        use std::sync::{Arc, Mutex};
        use std::sync::atomic::{AtomicBool, Ordering};

        let queue: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let queue_tx = Arc::clone(&queue);
        let finished_tx = Arc::clone(&finished);

        std::thread::spawn(move || {
            for (artist, album_name) in albums {
                std::thread::sleep(std::time::Duration::from_millis(1100));
                if let Some(bytes) =
                    crate::library::metadata::fetch_album_cover(&artist, &album_name)
                {
                    let key = format!("{}|{}", artist, album_name);
                    queue_tx.lock().unwrap().push((key, bytes));
                }
            }
            finished_tx.store(true, Ordering::Relaxed);
        });

        let covers = Rc::clone(&self.covers);
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let mut q = queue.lock().unwrap();
            for (key, bytes) in q.drain(..) {
                if let Some((stack, picture)) = covers.borrow().get(&key) {
                    let gbytes = glib::Bytes::from(&bytes);
                    if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&gbytes) {
                        picture.set_paintable(Some(&texture));
                        stack.set_visible_child_name("art");
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

fn make_album_card(album: &Album) -> (FlowBoxChild, Stack, Picture) {
    // Overlay fijo CARD_SIZE×CARD_SIZE — no expande, siempre cuadrado
    let overlay = Overlay::new();
    overlay.set_size_request(CARD_SIZE, CARD_SIZE);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_hexpand(false);
    overlay.set_vexpand(false);

    // Stack art | placeholder
    let cover_stack = Stack::new();
    cover_stack.set_halign(Align::Fill);
    cover_stack.set_valign(Align::Fill);
    cover_stack.set_hexpand(true);
    cover_stack.set_vexpand(true);
    cover_stack.set_overflow(gtk4::Overflow::Hidden);
    cover_stack.set_transition_type(StackTransitionType::Crossfade);
    cover_stack.set_transition_duration(150);

    let cover_picture = Picture::new();
    cover_picture.set_content_fit(ContentFit::Cover);
    cover_picture.set_can_shrink(true);
    cover_picture.set_halign(Align::Fill);
    cover_picture.set_valign(Align::Fill);
    cover_picture.set_hexpand(true);
    cover_picture.set_vexpand(true);
    cover_stack.add_named(&cover_picture, Some("art"));

    let placeholder = Image::from_icon_name("media-optical-symbolic");
    placeholder.set_pixel_size(64);
    placeholder.add_css_class("dim-label");
    cover_stack.add_named(&placeholder, Some("placeholder"));

    if let Some(ref data) = album.cover {
        let gbytes = glib::Bytes::from(data.as_slice());
        if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&gbytes) {
            cover_picture.set_paintable(Some(&texture));
            cover_stack.set_visible_child_name("art");
        } else {
            cover_stack.set_visible_child_name("placeholder");
        }
    } else {
        cover_stack.set_visible_child_name("placeholder");
    }

    overlay.set_child(Some(&cover_stack));

    // Texto overlay en la parte inferior con gradiente
    let info = gtk4::Box::new(Orientation::Vertical, 1);
    info.set_valign(Align::End);
    info.set_halign(Align::Fill);
    info.add_css_class("album-overlay-box");

    let lbl_name = Label::new(Some(&album.name));
    lbl_name.add_css_class("album-overlay-title");
    lbl_name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_name.set_xalign(0.0);

    let lbl_artist = Label::new(Some(&album.artist));
    lbl_artist.add_css_class("album-overlay-artist");
    lbl_artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl_artist.set_xalign(0.0);

    info.append(&lbl_name);
    info.append(&lbl_artist);
    overlay.add_overlay(&info);

    let child = FlowBoxChild::new();
    child.add_css_class("mosaic-child");
    child.set_child(Some(&overlay));

    (child, cover_stack, cover_picture)
}
