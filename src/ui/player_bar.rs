use std::rc::Rc;

use crate::i18n::gettext;
use crate::library::{fmt_duration, Track};
use crate::ui::icons::{self, Icon};
use crate::ui::image_apply::{apply_image, ImageTarget};
use crate::ui::now_playing::NowPlaying;
use gtk4::prelude::*;
use gtk4::{
    Align, Box, Button, CenterBox, GestureClick, Image, Label, Orientation, ProgressBar, Scale,
    Stack, StackTransitionType,
};

const COVER_SIZE: i32 = 88;
const CTRL_ICON_SIZE: i32 = 20;
const PLAY_ICON_SIZE: i32 = 24;
/// Pixels the ▶ glyph is nudged right so it reads as centered in the round
/// button: a play triangle's visual center sits left of its geometric one, and
/// at 24px that gap is visible. ⏸ is symmetric, so `set_playing` clears it.
/// The shift stays within the button's min-size, so the circle isn't deformed.
/// TODO: drop this optical fudge once we ship a pre-centered play/pause icon
/// (own SVG / icon set) instead of the themed symbolic ones — see docs/TODO.md.
const PLAY_GLYPH_NUDGE: i32 = 3;

pub struct PlayerBar {
    pub root: Box,
    pub btn_prev: Button,
    pub btn_play_pause: Button,
    pub btn_next: Button,
    pub btn_shuffle: Button,
    pub btn_loop: Button,
    pub lbl_title: Label,
    pub lbl_artist: Label,
    pub lbl_elapsed: Label,
    pub lbl_total: Label,
    pub lbl_volume: Label,
    pub prog_gesture: GestureClick,
    pub prog_bar: ProgressBar,
    pub vol_scale: Scale,
    cover_img: Image,
    cover_stack: Stack,
    play_pause_icon: Image,
    /// Shared "what's playing" bus. `set_playing` pushes the play/paused state
    /// here so every track list flips its active-row icon in sync — this is the
    /// single point where that state is broadcast.
    now_playing: Rc<NowPlaying>,
}

impl PlayerBar {
    pub fn new(now_playing: Rc<NowPlaying>) -> Self {
        let root = Box::new(Orientation::Vertical, 0);
        root.set_vexpand(false);
        root.add_css_class("audra-player-bar");

        // --- Carátula ---
        // Stack con dos hijos: placeholder (CSS+Unicode) y la imagen real.
        // Usar un Stack en lugar de Image::from_icon_name evita la dependencia
        // del tema de íconos del sistema, que produce resultados distintos en
        // Fedora, Debian y Windows.
        let cover_stack = Stack::new();
        cover_stack.set_transition_type(StackTransitionType::Crossfade);
        cover_stack.set_transition_duration(150);
        cover_stack.set_halign(Align::Fill);
        cover_stack.set_valign(Align::Fill);

        let placeholder_box = Box::new(Orientation::Vertical, 0);
        placeholder_box.add_css_class("bar-cover-placeholder");
        placeholder_box.set_halign(Align::Fill);
        placeholder_box.set_valign(Align::Fill);
        let note_lbl = Label::new(Some("♪"));
        note_lbl.add_css_class("bar-cover-note");
        note_lbl.add_css_class("dim-label");
        note_lbl.set_halign(Align::Center);
        note_lbl.set_valign(Align::Center);
        note_lbl.set_vexpand(true);
        placeholder_box.append(&note_lbl);

        // set_pixel_size hace que Image reporte exactamente COVER_SIZE como tamaño
        // natural sin importar qué paintable esté cargado.
        let cover_img = Image::new();
        cover_img.set_pixel_size(COVER_SIZE);
        cover_img.set_halign(Align::Fill);
        cover_img.set_valign(Align::Fill);

        cover_stack.add_named(&placeholder_box, Some("placeholder"));
        cover_stack.add_named(&cover_img, Some("art"));
        cover_stack.set_visible_child_name("placeholder");

        // Wrapper para overflow:hidden + border-radius (Image solo no puede clipear)
        let cover_wrap = Box::new(Orientation::Horizontal, 0);
        cover_wrap.add_css_class("cover-thumb");
        cover_wrap.set_size_request(COVER_SIZE, COVER_SIZE);
        cover_wrap.set_hexpand(false);
        cover_wrap.set_vexpand(false);
        cover_wrap.set_halign(Align::Start);
        cover_wrap.set_valign(Align::Center);
        cover_wrap.set_overflow(gtk4::Overflow::Hidden);
        cover_wrap.append(&cover_stack);

        // --- Centre column: controls stacked on top of title/artist ---
        // Vertically centered against the cover so the controls drop a few
        // pixels below the cover's top edge instead of sticking to it; the
        // title sits naturally beneath the controls.
        let center = Box::new(Orientation::Vertical, 2);
        center.set_hexpand(true);
        center.set_valign(Align::Center);

        let controls = Box::new(Orientation::Horizontal, 2);
        controls.set_halign(Align::Center);

        let btn_shuffle =
            icons::flat_icon_button(Icon::Shuffle, CTRL_ICON_SIZE, Some(&gettext("Shuffle")));
        let btn_prev =
            icons::flat_icon_button(Icon::SkipBack, CTRL_ICON_SIZE, Some(&gettext("Previous")));
        let (btn_play_pause, play_pause_icon) =
            icons::icon_button(Icon::Play, PLAY_ICON_SIZE, Some(&gettext("Play / Pause")));
        btn_play_pause.add_css_class("circular");
        btn_play_pause.add_css_class("suggested-action");
        // Optically center the play triangle in the round button; `set_playing`
        // resets the nudge for the symmetric pause glyph. Center alignment makes
        // the margin a clean shift instead of just narrowing a stretched image.
        play_pause_icon.set_halign(Align::Center);
        play_pause_icon.set_valign(Align::Center);
        play_pause_icon.set_margin_start(PLAY_GLYPH_NUDGE);
        let btn_next =
            icons::flat_icon_button(Icon::SkipForward, CTRL_ICON_SIZE, Some(&gettext("Next")));
        let btn_loop =
            icons::flat_icon_button(Icon::Repeat, CTRL_ICON_SIZE, Some(&gettext("Repeat")));

        controls.append(&btn_shuffle);
        controls.append(&btn_prev);
        controls.append(&btn_play_pause);
        controls.append(&btn_next);
        controls.append(&btn_loop);

        // Title + artist, packed tight (spacing 0 — they already read as a
        // pair via the heading / dim-label styles).
        let info = Box::new(Orientation::Vertical, 0);
        info.set_halign(Align::Center);

        let lbl_title = Label::new(Some(&gettext("No playback")));
        lbl_title.add_css_class("audra-bar-title");
        lbl_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        lbl_title.set_max_width_chars(40);

        let lbl_artist = Label::new(Some(""));
        lbl_artist.add_css_class("dim-label");
        lbl_artist.add_css_class("audra-bar-artist");
        lbl_artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        lbl_artist.set_max_width_chars(40);

        info.append(&lbl_title);
        info.append(&lbl_artist);

        center.append(&controls);
        center.append(&info);

        // --- Volumen (derecha) ---
        // Vertically centered against the cover so the volume row sits near
        // the cover's mid-line instead of floating up at the top edge.
        let vol_box = Box::new(Orientation::Horizontal, 4);
        vol_box.set_valign(Align::Center);
        vol_box.set_hexpand(false);

        let vol_icon = icons::image(Icon::VolumeUp, CTRL_ICON_SIZE);
        vol_icon.add_css_class("dim-label");

        // Step 0.01 (1%) for keyboard / scroll input; the previous 0.05
        // jumped 5% per keypress which felt twitchy. Width 140 (was 90)
        // gives roughly 1% per pixel for click-to-position, so small
        // mouse movements no longer translate into big level changes.
        let vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
        vol_scale.set_value(0.5);
        vol_scale.set_size_request(140, -1);
        vol_scale.set_draw_value(false);
        vol_scale.set_tooltip_text(Some(&gettext("Volume")));

        let lbl_volume = Label::new(Some("50%"));
        lbl_volume.add_css_class("dim-label");
        lbl_volume.add_css_class("caption");
        lbl_volume.add_css_class("audra-mono");
        lbl_volume.set_width_chars(4);
        lbl_volume.set_xalign(1.0);

        vol_box.append(&vol_icon);
        vol_box.append(&vol_scale);
        vol_box.append(&lbl_volume);

        // Row height is driven by the cover. Controls and volume are both
        // pinned to the top (`valign: Start`) so they align with the cover's
        // top edge; the title/artist then drops underneath the controls
        // inside the same row, instead of pushing the layout taller.
        let top_row = CenterBox::new();
        top_row.set_vexpand(false);
        top_row.set_margin_top(10);
        top_row.set_margin_bottom(2);
        top_row.set_margin_start(24);
        top_row.set_margin_end(24);
        top_row.set_start_widget(Some(&cover_wrap));
        top_row.set_center_widget(Some(&center));
        top_row.set_end_widget(Some(&vol_box));

        // --- Barra de progreso ---
        let bottom_row = Box::new(Orientation::Horizontal, 8);
        bottom_row.set_vexpand(false);
        bottom_row.set_margin_bottom(6);
        bottom_row.set_margin_start(28);
        bottom_row.set_margin_end(28);
        bottom_row.set_valign(Align::Center);

        let lbl_elapsed = Label::new(Some("0:00"));
        lbl_elapsed.add_css_class("dim-label");
        lbl_elapsed.add_css_class("caption");
        lbl_elapsed.add_css_class("audra-mono");
        lbl_elapsed.set_width_chars(5);
        lbl_elapsed.set_xalign(1.0);

        let prog_bar = ProgressBar::new();
        prog_bar.set_hexpand(true);
        prog_bar.set_valign(Align::Center);
        let prog_gesture = GestureClick::new();
        prog_bar.add_controller(prog_gesture.clone());

        let lbl_total = Label::new(Some("0:00"));
        lbl_total.add_css_class("dim-label");
        lbl_total.add_css_class("caption");
        lbl_total.add_css_class("audra-mono");
        lbl_total.set_width_chars(5);
        lbl_total.set_xalign(0.0);

        bottom_row.append(&lbl_elapsed);
        bottom_row.append(&prog_bar);
        bottom_row.append(&lbl_total);

        root.append(&top_row);
        root.append(&bottom_row);

        Self {
            root,
            btn_prev,
            btn_play_pause,
            btn_next,
            btn_shuffle,
            btn_loop,
            lbl_title,
            lbl_artist,
            lbl_elapsed,
            lbl_total,
            prog_bar,
            prog_gesture,
            vol_scale,
            lbl_volume,
            cover_img,
            cover_stack,
            play_pause_icon,
            now_playing,
        }
    }

    pub fn update_track(&self, track: Option<&Track>) {
        match track {
            Some(t) => {
                self.lbl_title.set_text(&t.display_title());
                self.lbl_artist.set_text(&t.display_artist());
                self.lbl_total.set_text(&t.duration_str());
                self.lbl_elapsed.set_text("0:00");
                self.prog_bar.set_fraction(0.0);
            }
            None => {
                self.cover_stack.set_visible_child_name("placeholder");
                self.lbl_title.set_text(&gettext("No playback"));
                self.lbl_artist.set_text("");
                self.lbl_total.set_text("0:00");
                self.lbl_elapsed.set_text("0:00");
                self.prog_bar.set_fraction(0.0);
            }
        }
    }

    pub fn update_cover(&self, bytes: Option<&[u8]>) {
        apply_image(
            ImageTarget::PlayerCover {
                image: self.cover_img.clone(),
                stack: self.cover_stack.clone(),
            },
            bytes,
            COVER_SIZE,
        );
    }

    pub fn update_progress(&self, elapsed_secs: f64, total_secs: f64) {
        self.lbl_elapsed
            .set_text(&fmt_duration(elapsed_secs as u64));
        if total_secs > 0.0 {
            self.prog_bar
                .set_fraction((elapsed_secs / total_secs).clamp(0.0, 1.0));
        }
    }

    pub fn set_playing(&self, playing: bool) {
        let icon = if playing { Icon::Pause } else { Icon::Play };
        icons::set_image_icon(
            &self.play_pause_icon,
            icon,
            PLAY_ICON_SIZE,
            &icons::foreground_color(&self.play_pause_icon),
        );
        // ⏸ is symmetric (centered); ▶ needs the optical nudge.
        self.play_pause_icon
            .set_margin_start(if playing { 0 } else { PLAY_GLYPH_NUDGE });
        // Broadcast so list rows flip their active-row ⏸ / ▶ icon in sync.
        self.now_playing.set_playing(playing);
    }
}
