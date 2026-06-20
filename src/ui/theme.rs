use std::cell::RefCell;

static BUNDLED_FONT: &[u8] = include_bytes!("../../data/fonts/InterVariable.ttf");

/// File name used both for the FontConfig extraction path and as the variable
/// font's internal family name (`fc-query` reports "Inter Variable").
const BUNDLED_FONT_FILE: &str = "InterVariable.ttf";

/// All visual styling lives in this CSS file. Keep tweaks there, not inline.
const APP_CSS_BASE: &str = include_str!("../../data/style.css");

// The variable TTF registers its family as "Inter Variable"; fall back to a
// system "Inter" and then the generic sans if neither is present.
// The variable TTF registers its family as "Inter Variable"; fall back to a
// system "Inter" and then the generic sans if neither is present.
const BUNDLED_FONT_CSS: &str = "
* {
    font-family: 'Inter Variable', 'Inter', sans-serif;
}
";

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
    static USE_BUNDLED_FONT: RefCell<bool> = const { RefCell::new(false) };
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

/// Convert RGB (0-255) to HSL, with hue in [0,1) turns and s,l in [0,1].
/// Achromatic colors return hue 0 and saturation 0.
fn rgb_to_hsl(rgb: (u8, u8, u8)) -> (f32, f32, f32) {
    let r = rgb.0 as f32 / 255.0;
    let g = rgb.1 as f32 / 255.0;
    let b = rgb.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l); // achromatic
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = (if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    }) / 6.0;
    (h, s, l)
}

/// Convert HSL (hue in turns, wrapped to [0,1); s,l in [0,1]) back to RGB.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
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

/// Multiply the HSL saturation of `rgb` by `factor` (clamped to [0,1]),
/// preserving hue and lightness. The dominant-color extractor averages a
/// quantised histogram bucket, which tends to mute the result toward grey;
/// boosting chroma here makes the tint read as the cover's real color
/// instead of a washed-out tone — without ever touching opacity.
fn boost_saturation(rgb: (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let (h, s, l) = rgb_to_hsl(rgb);
    if s == 0.0 {
        return rgb; // achromatic: nothing to saturate
    }
    hsl_to_rgb(h, (s * factor).clamp(0.0, 1.0), l)
}

/// Below this HSL saturation a color's hue is too unreliable to use (it reads
/// as grey), so hue-based variation is pointless and we fall back to lightness.
const CHROMA_MIN_S: f32 = 0.12;
/// Hue spread (in turns, ~25°) at or above which a palette already has enough
/// color variety that the aurora gradients differ on their own.
const HUE_VARIED: f32 = 25.0 / 360.0;

/// Largest circular distance (in turns, 0..=0.5) between any pair of hues.
fn hue_spread(hues: &[f32]) -> f32 {
    let mut max = 0.0_f32;
    for (i, &a) in hues.iter().enumerate() {
        for &b in &hues[i + 1..] {
            let d = (a - b).rem_euclid(1.0);
            max = max.max(d.min(1.0 - d));
        }
    }
    max
}

/// When a cover's palette collapses to near-identical colors (a minimalist or
/// monochrome sleeve), the three aurora gradients stack the same tone and the
/// background reads flat. Detect that and synthesise variation from the
/// dominant color: analogous hue rotations (±24°) when the cover has real
/// chroma, a lightness ramp when it is essentially grey. A palette that already
/// spans distinct hues passes through untouched. Always returns three colors,
/// most dominant first.
fn diversify_palette(palette: &[(u8, u8, u8)]) -> Vec<(u8, u8, u8)> {
    let base = palette[0];
    let hues: Vec<f32> = palette
        .iter()
        .filter_map(|&c| {
            let (h, s, _) = rgb_to_hsl(c);
            (s >= CHROMA_MIN_S).then_some(h)
        })
        .collect();
    if hue_spread(&hues) >= HUE_VARIED {
        let mut out: Vec<_> = palette.iter().take(3).copied().collect();
        while out.len() < 3 {
            out.push(base);
        }
        return out;
    }
    let (h, s, l) = rgb_to_hsl(base);
    // Open a light-to-dark gap between the layers, widened as the base darkens:
    // on dark, narrow-gamut covers (an etching-style sleeve) hue rotation alone
    // is barely visible because low-luminance tones all read as near-black, so
    // the lightness spread is what actually gives the gradients depth.
    let lift = 0.12 + (0.45 - l).max(0.0) * 0.6;
    let l_light = (l + lift).min(0.62);
    let l_dark = (l * 0.72).max(0.04);
    if s >= CHROMA_MIN_S {
        // Saturated: pair the lightness gap with analogous hue rotation (±24°).
        vec![
            hsl_to_rgb(h - 24.0 / 360.0, s, l_light),
            base,
            hsl_to_rgb(h + 24.0 / 360.0, s, l_dark),
        ]
    } else {
        // Near-grey: hue rotation buys nothing, so the lightness ramp alone
        // carries the depth — still far better than three identical fields.
        vec![
            hsl_to_rgb(h, s, l_light),
            base,
            hsl_to_rgb(h, s, l_dark),
        ]
    }
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
    // Synthesise color variety for flat covers so the three gradients differ
    // (see `diversify_palette`), then boost chroma so the layers read as the
    // cover's real colors and cap luminance at Y=150 so white text stays
    // legible even where layers stack.
    let colors: Vec<(u8, u8, u8)> = diversify_palette(palette)
        .into_iter()
        .map(|c| cap_luminance(boost_saturation(c, 1.3), 150.0))
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
    if USE_BUNDLED_FONT.with(|c| *c.borrow()) {
        css.push_str(BUNDLED_FONT_CSS);
    }
    let mode = TINT_MODE.with(|c| *c.borrow());
    let tint = TINT_PALETTE.with(|c| {
        c.borrow()
            .as_ref()
            .map(|palette| dynamic_tint_css(palette, mode))
            .unwrap_or_default()
    });
    if tint.is_empty() {
        // No active tint (Off mode, or no cover extracted yet). Reset the window
        // background EXPLICITLY instead of just dropping the rule: the tinted
        // `window` rule sets `background-image` with a transition, and GTK keeps
        // that last gradient applied when the rule merely disappears from the
        // reloaded provider — so the previous cover's color would linger after
        // switching dynamic color off. Forcing it back to the theme default
        // clears it. (`@window_bg_color` is always defined by libadwaita.)
        css.push_str("window { background-color: @window_bg_color; background-image: none; }\n");
    } else {
        css.push_str(&tint);
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
    let font_path = font_dir.join(BUNDLED_FONT_FILE);
    if !font_path.exists() {
        std::fs::write(&font_path, BUNDLED_FONT).ok()?;
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

/// Drop Pango's cached default font map so the next `PangoContext` (and thus
/// every widget built afterwards) is created from the *current* FontConfig,
/// picking up the bundled font we just added with `FcConfigAppFontAddFile`.
///
/// Without this, older Pango (Debian/Ubuntu, where the .deb runs) keeps the
/// font map it built at GTK init and never re-scans the runtime font addition,
/// so 'Inter' is invisible and the CSS silently falls back to the system font.
/// Newer Pango (Fedora) re-scans on its own — which is exactly why the font
/// only broke on the .deb and worked everywhere else. Must run before our
/// widgets exist (we call it from `setup_css`, before the window is built).
fn refresh_font_map() {
    use std::ffi::c_void;
    extern "C" {
        fn pango_cairo_font_map_set_default(fontmap: *mut c_void);
    }
    unsafe { pango_cairo_font_map_set_default(std::ptr::null_mut()) };
}

pub fn setup_css(use_bundled_font: bool) {
    USE_BUNDLED_FONT.with(|c| *c.borrow_mut() = use_bundled_font);
    // Always make the bundled font available to Pango, even when the user
    // currently prefers the system font. Registering it is harmless (the CSS
    // decides whether it's actually applied) and it means the in-app toggle can
    // switch to the bundled font instantly without touching FontConfig again. The font-map
    // refresh right after guarantees the addition is visible on every distro;
    // doing it here, before any widget builds its Pango context, makes it stick.
    if let Some(path) = extract_font() {
        register_font(&path);
        refresh_font_map();
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

pub fn update_font(use_bundled_font: bool) {
    USE_BUNDLED_FONT.with(|c| *c.borrow_mut() = use_bundled_font);
    // The font is registered once (and the font map refreshed) in `setup_css`
    // at startup, so by now 'Inter' already lives in every widget's font map.
    // Switching is therefore a pure CSS change — no FontConfig work, and no
    // font-map reset (which wouldn't reach the already-created widgets anyway).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// HSL round-trips back to (near) the original RGB for representative hues.
    #[test]
    fn hsl_round_trip() {
        for &rgb in &[
            (200, 40, 40),
            (40, 200, 90),
            (40, 90, 200),
            (128, 128, 128),
            (10, 10, 10),
            (245, 245, 245),
        ] {
            let (h, s, l) = rgb_to_hsl(rgb);
            let back = hsl_to_rgb(h, s, l);
            let close = |a: u8, b: u8| (a as i16 - b as i16).abs() <= 1;
            assert!(
                close(rgb.0, back.0) && close(rgb.1, back.1) && close(rgb.2, back.2),
                "{rgb:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn hue_spread_is_circular() {
        // Red sits at hue 0; a hue just below 1.0 is only a sliver away.
        assert!(hue_spread(&[0.01, 0.99]) < 0.05);
        // Opposite hues are half a turn apart, the maximum.
        assert!((hue_spread(&[0.0, 0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(hue_spread(&[0.3]), 0.0);
    }

    /// A palette that already spans distinct hues is returned unchanged.
    #[test]
    fn varied_palette_passes_through() {
        let p = vec![(200, 40, 40), (40, 200, 90), (40, 90, 200)];
        assert_eq!(diversify_palette(&p), p);
    }

    /// A flat but saturated palette yields three *different* colors.
    #[test]
    fn flat_saturated_palette_is_diversified() {
        let p = vec![(30, 90, 200), (32, 92, 198), (28, 88, 202)];
        let out = diversify_palette(&p);
        assert_eq!(out.len(), 3);
        assert!(out[0] != out[1] || out[1] != out[2]);
        assert_ne!(out[0], out[2]);
        // The dominant color stays the middle layer.
        assert_eq!(out[1], p[0]);
    }

    /// A near-grey palette still gains depth via a lightness ramp.
    #[test]
    fn flat_grey_palette_gets_lightness_ramp() {
        let p = vec![(120, 120, 122), (118, 118, 120)];
        let out = diversify_palette(&p);
        assert_eq!(out.len(), 3);
        let lum = |c: (u8, u8, u8)| c.0 as u16 + c.1 as u16 + c.2 as u16;
        assert!(lum(out[0]) > lum(out[1]) && lum(out[1]) > lum(out[2]));
    }

    /// A single-color palette is padded to three usable colors.
    #[test]
    fn single_color_palette_is_padded() {
        let out = diversify_palette(&[(180, 60, 60)]);
        assert_eq!(out.len(), 3);
    }

    /// A dark, narrow-gamut cover (etching-style sleeve) gets a visibly lighter
    /// layer so the aurora has depth instead of reading as a flat dark field.
    #[test]
    fn dark_palette_gains_light_layer() {
        let base = (60, 40, 25); // dark sepia/brown
        let out = diversify_palette(&[base]);
        let lum = |c: (u8, u8, u8)| 0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32;
        assert!(lum(out[0]) > lum(base) + 30.0, "lightest layer too dark: {out:?}");
        assert!(lum(out[0]) > lum(out[2]), "no light-to-dark ramp: {out:?}");
    }
}
