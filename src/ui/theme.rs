use std::cell::RefCell;

static JOST_FONT: &[u8] = include_bytes!("../../data/fonts/JostVariable.ttf");

/// All visual styling lives in this CSS file. Keep tweaks there, not inline.
const APP_CSS_BASE: &str = include_str!("../../data/style.css");

const JOST_FONT_CSS: &str = "
* {
    font-family: 'Jost', sans-serif;
}
";

thread_local! {
    static PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

fn build_css(use_jost: bool) -> String {
    if use_jost {
        format!("{}{}", APP_CSS_BASE, JOST_FONT_CSS)
    } else {
        APP_CSS_BASE.to_string()
    }
}

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
    use std::ffi::{c_char, c_void, CString};
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

pub fn setup_css(use_jost: bool) {
    if use_jost {
        if let Some(path) = extract_font() {
            register_font(&path);
        }
    }
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&build_css(use_jost));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    PROVIDER.with(|cell| {
        *cell.borrow_mut() = Some(provider);
    });
}

pub fn update_font(use_jost: bool) {
    if use_jost {
        if let Some(path) = extract_font() {
            register_font(&path);
        }
    }
    PROVIDER.with(|cell| {
        if let Some(provider) = cell.borrow().as_ref() {
            provider.load_from_string(&build_css(use_jost));
        }
    });
}
