//! Extract a representative color palette from a cover image, most dominant
//! color first, using the `color_thief` median-cut quantizer — the same
//! approach Amberol uses for its dynamic background.

use color_thief::ColorFormat;
use gdk_pixbuf::{InterpType, PixbufLoader};
use gtk4::prelude::*;

/// Thumbnail edge for palette extraction. Big enough that median cut has
/// enough samples to separate distinct colors, small enough to stay cheap.
/// Sized together with [`SAMPLE_STEP`]: the two decide how many pixels the
/// quantizer actually sees, and starving it is what makes rich covers collapse
/// into muddy near-greys.
const PALETTE_THUMB: i32 = 160;

/// Pixel step handed to `color_thief` as its `quality` argument (1 = densest,
/// 10 = sparsest). Kept at the densest setting because the quantizer is far
/// more sample-starved than the number suggests: `color-thief` 0.2.2 advances
/// its loop counter by `colors_count * step` even though the counter indexes
/// *pixels*, so the real step is 3x this value on RGB covers (4x on RGBA).
/// At the old 96px/step-10 combination that left ~300 samples to separate five
/// colors, and median cut answered by averaging distinct tones together into
/// grey. 160px at step 1 feeds it ~8500 instead. Extraction runs once per album
/// on a worker thread, so the extra work never touches the UI.
const SAMPLE_STEP: u8 = 1;

/// How many representative colors to pull from a cover. Only the first few ever
/// become gradient layers, but the extras are not waste:
///
/// * [`crate::ui::theme`] reads the *whole* palette to decide whether a cover is
///   genuinely monochrome. Colors below its chroma floor are ignored as hue
///   noise, so a sleeve whose only real color sits in fourth place would be
///   misread as flat — and get a synthesised tint — from a shorter list.
/// * Median cut is not a prefix: asking for five boxes partitions the color
///   space more finely than asking for three, so the leading colors come back
///   further apart instead of averaged over fewer, larger boxes.
pub const PALETTE_COLORS: u8 = 5;

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
    let colors = color_thief::get_palette(&packed, fmt, SAMPLE_STEP, max_colors.max(2)).ok()?;
    if colors.is_empty() {
        return None;
    }
    Some(colors.into_iter().map(|c| (c.r, c.g, c.b)).collect())
}
