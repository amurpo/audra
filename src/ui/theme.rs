use std::cell::RefCell;

static JOST_FONT: &[u8] = include_bytes!("../../data/fonts/JostVariable.ttf");

/// All visual styling lives in this CSS file. Keep tweaks there, not inline.
const APP_CSS_BASE: &str = include_str!("../../data/style.css");

const JOST_FONT_CSS: &str = "
* {
    font-family: 'Jost', sans-serif;
}
";

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TintMode {
    /// No dynamic tint; use the theme's default surfaces.
    Off,
    /// Tint the window background; leave libadwaita accent colors alone.
    Partial,
    /// Tint the background AND redefine `@accent_bg_color` / `@accent_color`
    /// so suggested-action buttons, switches, progress bars, etc. pick up
    /// the cover's dominant color too.
    Full,
}

impl TintMode {
    pub fn from_setting(s: &str) -> Self {
        match s {
            "off" => TintMode::Off,
            "full" => TintMode::Full,
            _ => TintMode::Partial,
        }
    }
    pub fn as_setting(self) -> &'static str {
        match self {
            TintMode::Off => "off",
            TintMode::Partial => "partial",
            TintMode::Full => "full",
        }
    }
}

thread_local! {
    static PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
    static USE_JOST: RefCell<bool> = const { RefCell::new(false) };
    static TINT_RGB: RefCell<Option<(u8, u8, u8)>> = const { RefCell::new(None) };
    static TINT_MODE: RefCell<TintMode> = const { RefCell::new(TintMode::Partial) };
}

/// Cap the perceptual luminance (ITU-BT.709) of `rgb` so white foreground
/// text keeps contrast. Bright covers (pastels, snow, light skin) get
/// scaled toward black until Y ≤ `max_y`; dark colors pass through. Hue
/// and saturation are preserved because we scale all three channels by
/// the same factor.
fn cap_luminance(rgb: (u8, u8, u8), max_y: f32) -> (u8, u8, u8) {
    let (r, g, b) = rgb;
    let y = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    if y <= max_y {
        return rgb;
    }
    let f = max_y / y;
    (
        (r as f32 * f).round() as u8,
        (g as f32 * f).round() as u8,
        (b as f32 * f).round() as u8,
    )
}

/// Build a CSS snippet that tints the window background with the album's
/// dominant color. The color is capped at Y=100 first so the tint stays
/// readable under the white foreground text; alpha 0.88 then lets the
/// theme's `@window_bg_color` peek through just enough to soften the
/// edges without washing the hue out.
/// Approximate the perceived background color of the tinted window
/// itself (without the `@card_shade_color` overlay) so we can paint it
/// as a SOLID base on the floating popover surface. GTK then layers the
/// real `@card_shade_color` on top via `background-image`, reproducing
/// the exact composition the header / player bar / list card show.
fn tinted_window_solid(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    const ALPHA: f32 = 0.88;
    const WIN_BG: f32 = 36.0;
    let (r, g, b) = rgb;
    let mix = |c: u8| (c as f32 * ALPHA + WIN_BG * (1.0 - ALPHA)).round() as u8;
    (mix(r), mix(g), mix(b))
}

/// Player-bar control overrides that only make sense when a dynamic tint
/// is active. The per-cover accent would otherwise be inherited by
/// play/pause (`suggested-action`) and the shuffle/loop `accent` toggle,
/// either clashing with the tinted background or disappearing into it.
/// Neutralising those buttons against `@window_fg_color` keeps the
/// player controls stable regardless of which cover is playing. In Off
/// mode this block is not emitted, so libadwaita's system accent runs.
const PLAYER_CONTROL_NEUTRAL_CSS: &str = "
.audra-player-bar button.suggested-action {
    background-color: alpha(@window_fg_color, 0.14);
    color: @window_fg_color;
}
.audra-player-bar button.suggested-action:hover {
    background-color: alpha(@window_fg_color, 0.20);
}
.audra-player-bar button.accent {
    background-color: alpha(@window_fg_color, 0.12);
    color: @window_fg_color;
}
";

fn dynamic_tint_css(rgb: (u8, u8, u8), mode: TintMode) -> String {
    if mode == TintMode::Off {
        return String::new();
    }
    let (r, g, b) = cap_luminance(rgb, 140.0);
    let (pr, pg, pb) = tinted_window_solid((r, g, b));
    // libadwaita applies different classes to its `dialog` node:
    //   AdwDialog       → `dialog.background`
    //   AdwAboutDialog  → `dialog.about`
    // Their default bg rule is `dialog-host > dialog.background sheet`
    // (specificity 0,1,3). Matching with `dialog-host > dialog.audra-shaded
    // sheet` lands at the same specificity and wins by source order,
    // covering both subclasses with one selector.
    let bg = format!(
        "window {{ background-image: linear-gradient(rgba({r},{g},{b},0.88), rgba({r},{g},{b},0.88)); }}\n\
         popover.audra-shaded > contents,\
         dialog-host > dialog.audra-shaded sheet {{\
             background-color: rgb({pr},{pg},{pb});\
             background-image: linear-gradient(@card_shade_color, @card_shade_color);\
         }}\n"
    );
    if mode == TintMode::Partial {
        // Partial leaves libadwaita's accent untouched, so the player
        // controls keep their system look — no neutral override needed.
        return bg;
    }
    // Full: redefine libadwaita's accent so suggested-action buttons,
    // .accent toggles, switches, progress bars, etc. follow the cover.
    // The accent is capped at Y=40 — much darker than the Y=140 bg — so
    // it contrasts against the tinted window instead of disappearing.
    let (ar, ag, ab) = cap_luminance(rgb, 40.0);
    // Special-case the "Play all" button (lives directly on the tinted
    // window, so a dark accent looks muddy) with `@card_shade_color` to
    // match the surrounding bars/cards. The player-bar controls get
    // neutralised against `@window_fg_color` so they stay stable across
    // covers instead of jumping with each track's accent.
    format!(
        "@define-color accent_bg_color rgb({ar},{ag},{ab});\n\
         @define-color accent_color rgb({ar},{ag},{ab});\n\
         button.audra-play-all {{ background-color: @card_shade_color; color: @window_fg_color; }}\n\
         {bg}{PLAYER_CONTROL_NEUTRAL_CSS}"
    )
}

fn build_css() -> String {
    let mut css = String::from(APP_CSS_BASE);
    if USE_JOST.with(|c| *c.borrow()) {
        css.push_str(JOST_FONT_CSS);
    }
    let mode = TINT_MODE.with(|c| *c.borrow());
    if let Some(rgb) = TINT_RGB.with(|c| *c.borrow()) {
        css.push_str(&dynamic_tint_css(rgb, mode));
    }
    css
}

fn reload() {
    PROVIDER.with(|cell| {
        if let Some(provider) = cell.borrow().as_ref() {
            provider.load_from_string(&build_css());
        }
    });
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
    USE_JOST.with(|c| *c.borrow_mut() = use_jost);
    if use_jost {
        if let Some(path) = extract_font() {
            register_font(&path);
        }
    }
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&build_css());
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
    USE_JOST.with(|c| *c.borrow_mut() = use_jost);
    if use_jost {
        if let Some(path) = extract_font() {
            register_font(&path);
        }
    }
    reload();
}

/// Set or clear the dynamic background tint. `None` reverts the window to
/// the theme's default background. Must run on the GTK main thread.
pub fn update_dynamic_tint(rgb: Option<(u8, u8, u8)>) {
    TINT_RGB.with(|c| *c.borrow_mut() = rgb);
    reload();
}

/// Change how the dynamic tint is applied. Takes effect immediately using
/// the last extracted color (no re-extraction needed).
pub fn set_tint_mode(mode: TintMode) {
    TINT_MODE.with(|c| *c.borrow_mut() = mode);
    reload();
}
