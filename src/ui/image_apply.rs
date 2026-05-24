//! Single point where image bytes meet a widget.
//!
//! Every cover/photo surface (album grid card, artist round photo, player
//! bar) goes through [`apply_image`]. Historically each surface had its own
//! variant and they drifted apart — one would hand raw bytes to
//! `Texture::from_bytes`, another would scale to widget size, another would
//! do it sync vs async. The result was bugs that only showed up on one
//! surface (e.g. cards growing in the album grid after a picker pick,
//! pixelated avatars on Avatar) and had to be hunted down in each call
//! site. Centralising the pipeline means a fix lands once.
//!
//! The bytes are decoded on a worker thread (GTK widgets are `!Send`), the
//! scaled pixels are handed back to the GLib main loop, and the texture is
//! applied to the target there.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gtk4::{Image, Picture, Stack};
use libadwaita as adw;

use crate::ui::image_utils::{pixels_to_texture, scale_to_pixels, ScaledPixels};

/// One of the on-screen surfaces that can render a cover/photo. Every
/// variant carries the widgets it needs to (a) render bytes and (b) fall
/// back to its own placeholder when bytes are `None`.
#[derive(Clone)]
pub enum ImageTarget {
    /// Album-art grid card: toggles the "art"/"placeholder" stack.
    AlbumCover { picture: Picture, stack: Stack },
    /// Artist round photo: `adw::Avatar::set_custom_image`.
    ArtistPhoto { avatar: adw::Avatar },
    /// Bottom player bar: `gtk::Image` inside an "art"/"placeholder" stack.
    PlayerCover { image: Image, stack: Stack },
}

impl ImageTarget {
    fn clear(&self) {
        match self {
            ImageTarget::AlbumCover { stack, .. } => stack.set_visible_child_name("placeholder"),
            ImageTarget::ArtistPhoto { avatar } => {
                avatar.set_custom_image(None::<&gtk4::gdk::Texture>);
            }
            ImageTarget::PlayerCover { stack, .. } => stack.set_visible_child_name("placeholder"),
        }
    }

    fn apply_scaled(&self, scaled: ScaledPixels, size: i32) {
        let (px, rs, alpha) = scaled;
        let texture = pixels_to_texture(px, rs, alpha, size);
        match self {
            ImageTarget::AlbumCover { picture, stack } => {
                picture.set_paintable(Some(&texture));
                stack.set_visible_child_name("art");
            }
            ImageTarget::ArtistPhoto { avatar } => {
                avatar.set_custom_image(Some(&texture));
            }
            ImageTarget::PlayerCover { image, stack } => {
                image.set_paintable(Some(&texture));
                stack.set_visible_child_name("art");
            }
        }
    }
}

/// Decode `bytes` on a worker thread, scale to `size`, then apply the
/// resulting texture to `target` from the GLib main loop. `None` clears the
/// widget to its placeholder synchronously.
pub fn apply_image(target: ImageTarget, bytes: Option<&[u8]>, size: i32) {
    let data = match bytes {
        Some(d) => d.to_vec(),
        None => {
            target.clear();
            return;
        }
    };

    let result: Arc<Mutex<Option<ScaledPixels>>> = Arc::new(Mutex::new(None));
    let done = Arc::new(AtomicBool::new(false));
    let result_tx = Arc::clone(&result);
    let done_tx = Arc::clone(&done);
    std::thread::spawn(move || {
        if let Some(scaled) = scale_to_pixels(&data, size) {
            *result_tx.lock().unwrap() = Some(scaled);
        }
        done_tx.store(true, Ordering::Relaxed);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if !done.load(Ordering::Relaxed) {
            return glib::ControlFlow::Continue;
        }
        if let Some(scaled) = result.lock().unwrap().take() {
            target.apply_scaled(scaled, size);
        }
        glib::ControlFlow::Break
    });
}
