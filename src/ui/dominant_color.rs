//! Pick a representative color from a cover image.
//!
//! Histogram in 4 bits per channel (4096 buckets) over a 32x32 thumbnail,
//! skipping near-black, near-white and low-saturation pixels so backgrounds
//! and letterboxing don't dominate. Returns the average of the bucket with
//! the highest count.

use gdk_pixbuf::{InterpType, PixbufLoader};
use gtk4::prelude::*;

const THUMB: i32 = 32;
const BUCKET_BITS: u8 = 4;
const BUCKETS_PER_CH: usize = 1 << BUCKET_BITS;
const TOTAL_BUCKETS: usize = BUCKETS_PER_CH * BUCKETS_PER_CH * BUCKETS_PER_CH;

pub fn extract(bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let loader = PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let src = loader.pixbuf()?;
    let thumb = src.scale_simple(THUMB, THUMB, InterpType::Bilinear)?;
    let raw = thumb.read_pixel_bytes();
    let pixels: &[u8] = raw.as_ref();
    let channels = if thumb.has_alpha() { 4 } else { 3 };
    let rowstride = thumb.rowstride() as usize;
    let shift = 8 - BUCKET_BITS;

    let mut counts = vec![0u32; TOTAL_BUCKETS];
    let mut sums = vec![(0u64, 0u64, 0u64); TOTAL_BUCKETS];

    for y in 0..THUMB as usize {
        let row_start = y * rowstride;
        for x in 0..THUMB as usize {
            let i = row_start + x * channels;
            if i + 2 >= pixels.len() {
                continue;
            }
            let r = pixels[i];
            let g = pixels[i + 1];
            let b = pixels[i + 2];

            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            if !(30..=240).contains(&max) {
                continue;
            }
            if (max - min) < 18 {
                continue;
            }

            let br = (r >> shift) as usize;
            let bg = (g >> shift) as usize;
            let bb = (b >> shift) as usize;
            let idx = br * BUCKETS_PER_CH * BUCKETS_PER_CH + bg * BUCKETS_PER_CH + bb;
            counts[idx] += 1;
            let (sr, sg, sb) = sums[idx];
            sums[idx] = (sr + r as u64, sg + g as u64, sb + b as u64);
        }
    }

    let (best_idx, &top) = counts.iter().enumerate().max_by_key(|&(_, c)| *c)?;
    if top == 0 {
        return None;
    }
    let (sr, sg, sb) = sums[best_idx];
    let c = top as u64;
    Some(((sr / c) as u8, (sg / c) as u8, (sb / c) as u8))
}
