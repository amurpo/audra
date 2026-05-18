use gdk_pixbuf::{self, prelude::*};
use glib;
use gtk4::prelude::Cast;

pub fn scale_to_pixels(data: &[u8], size: i32) -> Option<(Vec<u8>, i32, bool)> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    let _ = loader.write(data);
    let _ = loader.close();
    let src = loader.pixbuf()?;
    let w = src.width();
    let h = src.height();
    if w <= 0 || h <= 0 {
        return None;
    }
    let (sw, sh) = if w <= h {
        (size, size * h / w)
    } else {
        (size * w / h, size)
    };
    let scaled = src.scale_simple(sw, sh, gdk_pixbuf::InterpType::Bilinear)?;
    let x = (sw - size) / 2;
    let y = (sh - size) / 2;
    let dest = gdk_pixbuf::Pixbuf::new(
        src.colorspace(),
        src.has_alpha(),
        src.bits_per_sample(),
        size,
        size,
    )?;
    scaled.copy_area(x, y, size, size, &dest, 0, 0);
    let rowstride = dest.rowstride();
    let has_alpha = dest.has_alpha();
    let pixels = dest.read_pixel_bytes().to_vec();
    Some((pixels, rowstride, has_alpha))
}

pub fn pixels_to_texture(
    pixels: Vec<u8>,
    rowstride: i32,
    has_alpha: bool,
    size: i32,
) -> gtk4::gdk::Texture {
    let format = if has_alpha {
        gtk4::gdk::MemoryFormat::R8g8b8a8
    } else {
        gtk4::gdk::MemoryFormat::R8g8b8
    };
    let bytes = glib::Bytes::from_owned(pixels);
    gtk4::gdk::MemoryTexture::new(size, size, format, &bytes, rowstride as usize).upcast()
}
