//! Bundled [Remix Icon](https://remixicon.com) SVGs (Remix Icon License v1.0).
//!
//! Icons are rasterized with `resvg` into `GdkTexture` paintables so they render
//! the same on Linux, Windows, and macOS without relying on the host icon theme
//! or SVG loaders inside `GtkIconTheme`.

use gtk4::gdk::{RGBA, Texture};
use gtk4::prelude::*;
use gtk4::{Button, Image, Settings};
use libadwaita as adw;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

macro_rules! remix_svg {
    ($file:literal) => {
        include_bytes!(concat!(
            "../../data/icons/remix/",
            $file,
            ".svg"
        ))
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Shuffle,
    SkipBack,
    Play,
    Pause,
    SkipForward,
    Repeat,
    VolumeUp,
    ArrowLeft,
    User,
    Loader,
    CheckCircle,
    FolderMusic,
    Refresh,
    DeleteBin,
    Search,
    Album,
    Group,
    ListUnordered,
}

impl Icon {
    fn svg_bytes(self) -> &'static [u8] {
        match self {
            Icon::Shuffle => remix_svg!("shuffle-line"),
            Icon::SkipBack => remix_svg!("skip-back-line"),
            Icon::Play => remix_svg!("play-line"),
            Icon::Pause => remix_svg!("pause-line"),
            Icon::SkipForward => remix_svg!("skip-forward-line"),
            Icon::Repeat => remix_svg!("repeat-line"),
            Icon::VolumeUp => remix_svg!("volume-up-line"),
            Icon::ArrowLeft => remix_svg!("arrow-left-line"),
            Icon::User => remix_svg!("user-line"),
            Icon::Loader => remix_svg!("loader-4-line"),
            Icon::CheckCircle => remix_svg!("checkbox-circle-line"),
            Icon::FolderMusic => remix_svg!("folder-music-line"),
            Icon::Refresh => remix_svg!("refresh-line"),
            Icon::DeleteBin => remix_svg!("delete-bin-line"),
            Icon::Search => remix_svg!("search-line"),
            Icon::Album => remix_svg!("album-line"),
            Icon::Group => remix_svg!("group-line"),
            Icon::ListUnordered => remix_svg!("list-unordered"),
        }
    }

}

static TEXTURE_CACHE: OnceLock<Mutex<HashMap<(Icon, i32, u32), Texture>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<(Icon, i32, u32), Texture>> {
    TEXTURE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn color_key(color: &RGBA) -> u32 {
    let r = (color.red().clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (color.green().clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (color.blue().clamp(0.0, 1.0) * 255.0).round() as u32;
    let a = (color.alpha().clamp(0.0, 1.0) * 255.0).round() as u32;
    (r << 24) | (g << 16) | (b << 8) | a
}

fn rgba_hex(color: &RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.red().clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.green().clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.blue().clamp(0.0, 1.0) * 255.0).round() as u8
    )
}

fn render_texture(icon: Icon, size: i32, color: &RGBA) -> Texture {
    let size = size.max(1);
    let key = (icon, size, color_key(color));
    if let Some(tex) = cache().lock().unwrap().get(&key) {
        return tex.clone();
    }

    let svg = std::str::from_utf8(icon.svg_bytes()).expect("remix svg utf-8");
    let colored = svg.replace("currentColor", &rgba_hex(color));
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&colored, &opt).expect("parse remix svg");
    let size_u = size as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size_u, size_u).expect("icon pixmap");
    pixmap.fill(resvg::tiny_skia::Color::TRANSPARENT);
    let scale = size as f32 / 24.0;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixbuf = gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(pixmap.data().to_vec()),
        gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        size,
        size,
        size * 4,
    );
    let texture = Texture::for_pixbuf(&pixbuf);

    cache().lock().unwrap().insert(key, texture.clone());
    texture
}

pub fn foreground_color(widget: &impl IsA<gtk4::Widget>) -> RGBA {
    widget.color()
}

pub fn error_color(widget: &impl IsA<gtk4::Widget>) -> RGBA {
    widget
        .style_context()
        .lookup_color("error_color")
        .unwrap_or_else(|| RGBA::new(0.8, 0.15, 0.15, 1.0))
}

fn prefer_dark_ui() -> bool {
    Settings::default()
        .map(|s| s.is_gtk_application_prefer_dark_theme())
        .unwrap_or(false)
}

fn default_fg_rgba() -> RGBA {
    if prefer_dark_ui() {
        RGBA::new(1.0, 1.0, 1.0, 1.0)
    } else {
        RGBA::new(0.2, 0.2, 0.2, 1.0)
    }
}

pub fn set_image_icon(img: &Image, icon: Icon, size: i32, color: &RGBA) {
    let tex = render_texture(icon, size, color);
    img.set_paintable(Some(&tex));
    img.set_pixel_size(size);
}

fn refresh_themed_image(img: &Image, icon: Icon, size: i32) {
    let color = img
        .root()
        .map(|w| foreground_color(&w))
        .unwrap_or_else(default_fg_rgba);
    set_image_icon(img, icon, size, &color);
}

fn bind_themed_image(img: &Image, icon: Icon, size: i32) {
    let img = img.clone();
    img.connect_realize({
        let img = img.clone();
        move |_| refresh_themed_image(&img, icon, size)
    });
    if let Some(settings) = Settings::default() {
        settings.connect_gtk_application_prefer_dark_theme_notify({
            let img = img.clone();
            move |_| refresh_themed_image(&img, icon, size)
        });
    }
}

/// Image that tracks light/dark foreground from the widget style context.
pub fn image(icon: Icon, size: i32) -> Image {
    let img = Image::new();
    bind_themed_image(&img, icon, size);
    img
}

pub fn flat_icon_button(icon: Icon, size: i32, tooltip: Option<&str>) -> Button {
    let btn = Button::new();
    let img = image(icon, size);
    btn.set_child(Some(&img));
    btn.add_css_class("flat");
    if let Some(t) = tooltip {
        btn.set_tooltip_text(Some(t));
    }
    btn
}

pub fn icon_button(icon: Icon, size: i32, tooltip: Option<&str>) -> (Button, Image) {
    let btn = Button::new();
    let img = image(icon, size);
    btn.set_child(Some(&img));
    if let Some(t) = tooltip {
        btn.set_tooltip_text(Some(t));
    }
    (btn, img)
}

fn refresh_status_page_icon(page: &adw::StatusPage, icon: Icon, size: i32) {
    let color = page
        .root()
        .map(|w| foreground_color(&w))
        .unwrap_or_else(default_fg_rgba);
    let tex = render_texture(icon, size, &color);
    page.set_icon_name(None);
    page.set_paintable(Some(&tex));
}

pub fn set_status_page_icon(page: &adw::StatusPage, icon: Icon, size: i32) {
    page.set_icon_name(None);
    let page_ref = page.clone();
    page_ref.connect_realize({
        let page_ref = page_ref.clone();
        move |_| refresh_status_page_icon(&page_ref, icon, size)
    });
    if let Some(settings) = Settings::default() {
        settings.connect_gtk_application_prefer_dark_theme_notify({
            let page_ref = page_ref.clone();
            move |_| refresh_status_page_icon(&page_ref, icon, size)
        });
    }
}

