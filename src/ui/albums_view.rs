use gtk4::prelude::*;
use gtk4::{
    FlowBox, FlowBoxChild, Image, Label, Orientation, Overlay, Picture,
    ScrolledWindow, Stack, Align, ContentFit, SelectionMode, StackTransitionType,
};
use glib;
use gdk_pixbuf;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::library::Album;
use crate::library::db::Database;

const CARD_SIZE: i32 = 200;

type CoverMap = Rc<RefCell<HashMap<String, (Stack, Picture)>>>;

// Pixels ya escalados listos para convertir en Texture en el hilo principal
// (key, pixel_bytes, rowstride, has_alpha)
type ScaledCover = (String, Vec<u8>, i32, bool);

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
        flow.set_homogeneous(true);
        flow.set_column_spacing(8);
        flow.set_row_spacing(8);
        flow.set_margin_top(12);
        flow.set_margin_bottom(12);
        flow.set_margin_start(12);
        flow.set_margin_end(12);
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

    pub fn load_albums(&self, albums: Vec<Album>, db: Arc<Mutex<Database>>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.covers.borrow_mut().clear();

        let mut need_fetch: Vec<(String, String, String)> = Vec::new();

        for album in &albums {
            let key = format!("{}|{}", album.artist, album.name);
            let (card, stack, picture) = make_album_card(album);
            self.covers.borrow_mut().insert(key.clone(), (stack, picture));
            self.flow.append(&card);

            let track_path = album.tracks.first()
                .map(|t| t.path.clone())
                .unwrap_or_default();
            need_fetch.push((album.artist.clone(), album.name.clone(), track_path));
        }

        *self.albums_data.borrow_mut() = albums;

        if !need_fetch.is_empty() {
            self.start_cover_fetch(need_fetch, db);
        }
    }

    pub fn get_album_tracks(&self, idx: usize) -> Vec<crate::library::Track> {
        self.albums_data
            .borrow()
            .get(idx)
            .map(|a| a.tracks.clone())
            .unwrap_or_default()
    }

    pub fn filter(&self, query: &str) {
        if query.is_empty() {
            self.flow.set_filter_func(|_| true);
        } else {
            let q = query.to_lowercase();
            let albums = Rc::clone(&self.albums_data);
            self.flow.set_filter_func(move |child| {
                let idx = child.index() as usize;
                if let Some(album) = albums.borrow().get(idx) {
                    album.name.to_lowercase().contains(&q)
                        || album.artist.to_lowercase().contains(&q)
                } else {
                    false
                }
            });
        }
    }

    fn start_cover_fetch(&self, albums: Vec<(String, String, String)>, db: Arc<Mutex<Database>>) {
        use std::sync::atomic::{AtomicBool, Ordering};

        // La queue almacena pixels ya escalados — el hilo principal solo crea el Texture (rápido)
        let queue: Arc<Mutex<Vec<ScaledCover>>> = Arc::new(Mutex::new(Vec::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let queue_tx = Arc::clone(&queue);
        let finished_tx = Arc::clone(&finished);

        std::thread::spawn(move || {
            let mut need_lastfm: Vec<(String, String)> = Vec::new();

            for (artist, album_name, track_path) in &albums {
                let key = format!("{}|{}", artist, album_name);

                // Fase 1: cache en DB
                if let Some(bytes) = db.lock().unwrap().get_cover(artist, album_name) {
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        queue_tx.lock().unwrap().push((key, scaled.0, scaled.1, scaled.2));
                        continue;
                    }
                }

                // Fase 2: disco local → guardar en DB
                if let Some(bytes) = crate::library::art::read_cover_art(track_path) {
                    let _ = db.lock().unwrap().set_cover(artist, album_name, &bytes);
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        queue_tx.lock().unwrap().push((key, scaled.0, scaled.1, scaled.2));
                        continue;
                    }
                }

                // Fase 3: Last.fm como último recurso
                need_lastfm.push((artist.clone(), album_name.clone()));
            }

            for (artist, album_name) in need_lastfm {
                std::thread::sleep(std::time::Duration::from_millis(1100));
                if let Some(bytes) =
                    crate::library::metadata::fetch_album_cover(&artist, &album_name)
                {
                    let _ = db.lock().unwrap().set_cover(&artist, &album_name, &bytes);
                    if let Some(scaled) = scale_to_pixels(&bytes, CARD_SIZE) {
                        let key = format!("{}|{}", artist, album_name);
                        queue_tx.lock().unwrap().push((key, scaled.0, scaled.1, scaled.2));
                    }
                }
            }

            finished_tx.store(true, Ordering::Relaxed);
        });

        let covers = Rc::clone(&self.covers);
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let mut q = queue.lock().unwrap();
            // pixels_to_texture es solo una envoltura de bytes — muy rápido, no bloquea el UI
            for (key, pixels, rowstride, has_alpha) in q.drain(..) {
                if let Some((stack, picture)) = covers.borrow().get(&key) {
                    let texture = pixels_to_texture(pixels, rowstride, has_alpha);
                    picture.set_paintable(Some(&texture));
                    stack.set_visible_child_name("art");
                }
            }
            drop(q);
            if finished.load(std::sync::atomic::Ordering::Relaxed) {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

/// Decodifica y escala la imagen en el hilo de fondo.
/// Devuelve (pixel_bytes, rowstride, has_alpha) listos para crear un MemoryTexture.
fn scale_to_pixels(data: &[u8], size: i32) -> Option<(Vec<u8>, i32, bool)> {
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
    let dest = gdk_pixbuf::Pixbuf::new(
        src.colorspace(), src.has_alpha(), src.bits_per_sample(), size, size,
    )?;
    scaled.copy_area(x, y, size, size, &dest, 0, 0);
    let rowstride = dest.rowstride();
    let has_alpha = dest.has_alpha();
    let pixels = dest.read_pixel_bytes().to_vec();
    Some((pixels, rowstride, has_alpha))
}

/// Crea un GDK Texture desde bytes ya escalados — corre en el hilo principal, es instantáneo.
fn pixels_to_texture(pixels: Vec<u8>, rowstride: i32, has_alpha: bool) -> gtk4::gdk::Texture {
    let format = if has_alpha {
        gtk4::gdk::MemoryFormat::R8g8b8a8
    } else {
        gtk4::gdk::MemoryFormat::R8g8b8
    };
    let bytes = glib::Bytes::from_owned(pixels);
    gtk4::gdk::MemoryTexture::new(CARD_SIZE, CARD_SIZE, format, &bytes, rowstride as usize)
        .upcast()
}

fn make_album_card(album: &Album) -> (FlowBoxChild, Stack, Picture) {
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

    let placeholder = Image::from_icon_name("media-optical-symbolic");
    placeholder.set_pixel_size(64);
    placeholder.add_css_class("dim-label");
    cover_stack.add_named(&placeholder, Some("placeholder"));

    cover_stack.set_visible_child_name("placeholder");

    overlay.set_child(Some(&cover_stack));

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
    child.set_halign(Align::Center);
    child.set_valign(Align::Center);

    (child, cover_stack, cover_picture)
}
