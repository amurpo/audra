//! Extract a representative color palette from a cover image, most dominant
//! color first, using the `color_thief` median-cut quantizer — the same
//! approach Amberol uses for its dynamic background.

use color_thief::ColorFormat;
use gdk_pixbuf::{InterpType, PixbufLoader};
use gtk4::prelude::*;

/// Thumbnail edge for palette extraction. Big enough that median cut has
/// enough samples to separate distinct colors, small enough to stay cheap.
const PALETTE_THUMB: i32 = 96;

/// Extract up to `max_colors` representative colors from the cover, most
/// dominant first, using the `color_thief` median-cut quantizer (the same
/// approach Amberol uses for its dynamic background). Returns `None` when the
/// image can't be decoded or no color is found.
pub fn palette(bytes: &[u8], max_colors: u8) -> Option<Vec<(u8, u8, u8)>> {
    let loader = PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let src = loader.pixbuf()?;
    let thumb = src.scale_simple(PALETTE_THUMB, PALETTE_THUMB, InterpType::Bilinear)?;
    let channels = if thumb.has_alpha() { 4 } else { 3 };
    let rowstride = thumb.rowstride() as usize;
    let raw = thumb.read_pixel_bytes();
    let pixels: &[u8] = raw.as_ref();
    let w = PALETTE_THUMB as usize * channels;
    // color_thief expects tightly packed rows; strip any rowstride padding.
    let mut packed = Vec::with_capacity(PALETTE_THUMB as usize * w);
    for y in 0..PALETTE_THUMB as usize {
        let start = y * rowstride;
        if start + w <= pixels.len() {
            packed.extend_from_slice(&pixels[start..start + w]);
        }
    }
    let fmt = if channels == 4 {
        ColorFormat::Rgba
    } else {
        ColorFormat::Rgb
    };
    // quality 10 = fastest sampling; runs off the main thread anyway.
    let colors = color_thief::get_palette(&packed, fmt, 10, max_colors.max(2)).ok()?;
    if colors.is_empty() {
        return None;
    }
    Some(colors.into_iter().map(|c| (c.r, c.g, c.b)).collect())
}
