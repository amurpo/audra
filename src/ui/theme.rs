static JOST_FONT: &[u8] = include_bytes!("../../data/fonts/JostVariable.ttf");

const APP_CSS: &str = "
picture.cover-art {
    border-radius: 8px;
}
picture.artist-image {
    border-radius: 9999px;
}
.cover-placeholder {
    border-radius: 8px;
    background-color: alpha(currentColor, 0.05);
    padding: 18px;
}
flowboxchild.mosaic-child {
    padding: 0;
    transition: opacity 120ms;
}
flowboxchild.mosaic-child:hover {
    opacity: 0.82;
}
.album-overlay-box {
    padding: 28px 8px 7px 8px;
    background: linear-gradient(rgba(0,0,0,0), rgba(0,0,0,0.72));
}
.album-overlay-title {
    font-weight: bold;
    font-size: 0.85em;
    color: white;
}
.album-overlay-artist {
    font-size: 0.78em;
    color: rgba(255,255,255,0.72);
}
.lastfm-ok {
    color: #33d17a;
}
.lastfm-err {
    color: #e01b24;
}
.cover-thumb {
    border-radius: 6px;
    background-color: alpha(currentColor, 0.05);
}
flowboxchild.artist-card {
    border-radius: 12px;
    transition: background-color 150ms;
    padding: 4px;
}
flowboxchild.artist-card:hover {
    background-color: alpha(currentColor, 0.07);
}
.scan-loading-overlay {
    background-color: alpha(@window_bg_color, 0.92);
}
.scan-loading-card {
    border-radius: 18px;
    padding: 36px 52px;
}
.bar-cover-placeholder {
    border-radius: 6px;
    background-color: alpha(currentColor, 0.06);
}
.bar-cover-note {
    font-size: 26px;
}
.album-cover-note {
    font-size: 52px;
}
label.now-playing-title {
    color: @accent_color;
    font-weight: bold;
}
* {
    font-family: 'Jost', sans-serif;
}
";

fn extract_font() -> Option<std::path::PathBuf> {
    let font_dir = dirs::config_dir()?.join("audra").join("fonts");
    std::fs::create_dir_all(&font_dir).ok()?;
    let font_path = font_dir.join("JostVariable.ttf");
    if !font_path.exists() {
        std::fs::write(&font_path, JOST_FONT).ok()?;
    }
    Some(font_path)
}

fn register_font(path: &std::path::Path) {
    use std::ffi::{CString, c_char, c_void};
    extern "C" {
        fn FcConfigGetCurrent() -> *mut c_void;
        fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> bool;
    }
    if let Ok(cpath) = CString::new(path.to_string_lossy().as_bytes()) {
        unsafe {
            let config = FcConfigGetCurrent();
            FcConfigAppFontAddFile(config, cpath.as_ptr());
        }
    }
}

pub fn setup_css() {
    if let Some(path) = extract_font() {
        register_font(&path);
    }
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(APP_CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
