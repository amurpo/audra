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
    static TINT_PALETTE: RefCell<Option<Vec<(u8, u8, u8)>>> = const { RefCell::new(None) };
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

/// Mix `rgb` toward white until its perceptual luminance (BT.709) reaches at
/// least `min_y`, preserving hue. Colors already brighter than `min_y` pass
/// through untouched. Used for the progress / volume fills: a dark cover would
/// otherwise yield a fill too dark to read against the player bar, but going
/// fully neutral (white) throws away the dynamic color — lifting toward white
/// keeps the hue while guaranteeing it stands out.
fn lighten_to(rgb: (u8, u8, u8), min_y: f32) -> (u8, u8, u8) {
    let (r, g, b) = rgb;
    let y = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    if y >= min_y {
        return rgb;
    }
    let t = (min_y - y) / (255.0 - y);
    let mix = |c: u8| (c as f32 + (255.0 - c as f32) * t).round() as u8;
    (mix(r), mix(g), mix(b))
}

/// Multiply the HSL saturation of `rgb` by `factor` (clamped to [0,1]),
/// preserving hue and lightness. The dominant-color extractor averages a
/// quantised histogram bucket, which tends to mute the result toward grey;
/// boosting chroma here makes the tint read as the cover's real color
/// instead of a washed-out tone — without ever touching opacity.
fn boost_saturation(rgb: (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let r = rgb.0 as f32 / 255.0;
    let g = rgb.1 as f32 / 255.0;
    let b = rgb.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return rgb; // achromatic: nothing to saturate
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let s = (s * factor).clamp(0.0, 1.0);
    let h = (if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    }) / 6.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r1), to_u8(g1), to_u8(b1))
}

/// Player-bar control overrides that only make sense when a dynamic tint
/// is active. The per-cover accent would otherwise be inherited by
/// play/pause (`suggested-action`) and the shuffle/loop `accent` toggle,
/// either clashing with the tinted background or disappearing into it.
/// Neutralising those buttons against `@window_fg_color` keeps the
/// player controls stable regardless of which cover is playing. In Off
/// mode this block is not emitted, so libadwaita's system accent runs.
/// The progress / volume fills are handled separately (see `dynamic_tint_css`)
/// because they look better keeping the cover color than going neutral.
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

/// The multi-color "aurora" background is adapted from Amberol
/// (GPL-3.0-or-later), © Emmanuele Bassi — specifically the stacked diagonal
/// gradients in its `src/gtk/style.css`. Amberol paints one `linear-gradient`
/// per palette color, each fading from 55% to transparent toward a different
/// corner, so the cover's colors blend in the corners. We reproduce that, but
/// always over an OPAQUE `background-color` base: the alpha lives only between
/// layers, never against the window surface, so the desktop can't bleed
/// through (the bug the first translucent version had).
fn dynamic_tint_css(palette: &[(u8, u8, u8)], mode: TintMode) -> String {
    if mode == TintMode::Off || palette.is_empty() {
        return String::new();
    }
    // Boost chroma so the layers read as the cover's real colors, and cap
    // luminance at Y=150 so white text stays legible even where layers stack.
    let colors: Vec<(u8, u8, u8)> = palette
        .iter()
        .take(3)
        .map(|&c| cap_luminance(boost_saturation(c, 1.3), 150.0))
        .collect();
    // Opaque, dark-tinted base from the dominant color (Y=45). Everything else
    // is layered on top of this, so there is never real transparency.
    let (kr, kg, kb) = cap_luminance(boost_saturation(palette[0], 1.3), 45.0);
    // One diagonal gradient per color, angles ~110° apart (Amberol's 127/217/336).
    const ANGLES: [u16; 3] = [127, 217, 336];
    let layers = colors
        .iter()
        .enumerate()
        .map(|(i, &(r, g, b))| {
            let a = ANGLES[i];
            format!("linear-gradient({a}deg, rgba({r},{g},{b},0.55), rgba({r},{g},{b},0) 70.71%)")
        })
        .collect::<Vec<_>>()
        .join(", ");
    // libadwaita applies different classes to its `dialog` node:
    //   AdwDialog       → `dialog.background`
    //   AdwAboutDialog  → `dialog.about`
    // Their default bg rule is `dialog-host > dialog.background sheet`
    // (specificity 0,1,3). Matching with `dialog-host > dialog.audra-shaded
    // sheet` lands at the same specificity and wins by source order,
    // covering both subclasses with one selector. Popovers/dialogs get the
    // solid base + @card_shade_color overlay (no gradient) so they stay calm.
    let bg = format!(
        "window {{ background-color: rgb({kr},{kg},{kb}); background-image: {layers}; transition: background-image 250ms ease; }}\n\
         popover.audra-shaded > contents,\
         dialog-host > dialog.audra-shaded sheet {{\
             background-color: rgb({kr},{kg},{kb});\
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
    // The accent is capped at Y=40 — much darker than the bg — so it
    // contrasts against the tinted window instead of disappearing.
    let (ar, ag, ab) = cap_luminance(boost_saturation(palette[0], 1.3), 40.0);
    // Special-case the "Play all" button (lives directly on the tinted
    // window, so a dark accent looks muddy) with `@card_shade_color` to
    // match the surrounding bars/cards. The player-bar buttons get
    // neutralised against `@window_fg_color` so they stay stable across
    // covers instead of jumping with each track's accent.
    //
    // The progress / volume fills DO keep the cover color, but lifted toward
    // white to Y=170 so they read against the (darkened-cover) player bar even
    // when the cover is dark. The Y=40 accent above is too dark for these.
    //
    // The same lifted color tints the "now playing" row's highlight band. The
    // band itself (and the play/pause icon) ship in style.css for *every* tint
    // mode; here we only recolor it from the system accent to the cover's
    // palette so the highlight matches the dynamic background. `@accent_color`
    // and `@accent_bg_color` stay at Y=40 so the buttons/switches that rely on
    // them are unaffected.
    let (fr, fg, fb) = lighten_to(boost_saturation(palette[0], 1.3), 170.0);
    format!(
        "@define-color accent_bg_color rgb({ar},{ag},{ab});\n\
         @define-color accent_color rgb({ar},{ag},{ab});\n\
         .audra-track-row.playing {{ background-color: rgba({fr},{fg},{fb},0.22); }}\n\
         button.audra-play-all {{ background-color: @card_shade_color; color: @window_fg_color; }}\n\
         .audra-player-bar progressbar > trough > progress {{ background-color: rgb({fr},{fg},{fb}); }}\n\
         .audra-player-bar scale > trough > highlight {{ background-color: rgb({fr},{fg},{fb}); }}\n\
         .audra-player-bar scale > trough > slider {{ background-color: rgb({fr},{fg},{fb}); }}\n\
         {bg}{PLAYER_CONTROL_NEUTRAL_CSS}"
    )
}

fn build_css() -> String {
    let mut css = String::from(APP_CSS_BASE);
    if USE_JOST.with(|c| *c.borrow()) {
        css.push_str(JOST_FONT_CSS);
    }
    let mode = TINT_MODE.with(|c| *c.borrow());
    TINT_PALETTE.with(|c| {
        if let Some(palette) = c.borrow().as_ref() {
            css.push_str(&dynamic_tint_css(palette, mode));
        }
    });
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

/// Set or clear the dynamic background palette (dominant color first). `None`
/// reverts the window to the theme's default background. Must run on the GTK
/// main thread.
pub fn update_dynamic_tint(palette: Option<Vec<(u8, u8, u8)>>) {
    TINT_PALETTE.with(|c| *c.borrow_mut() = palette);
    reload();
}

/// Change how the dynamic tint is applied. Takes effect immediately using
/// the last extracted color (no re-extraction needed).
pub fn set_tint_mode(mode: TintMode) {
    TINT_MODE.with(|c| *c.borrow_mut() = mode);
    reload();
}
