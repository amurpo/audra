use gtk4::prelude::*;
use gtk4::{
    Box, Button, Image, Label, Orientation, Picture,
    ProgressBar, Scale, Stack, Align, ContentFit,
};
use gtk4::gdk;
use crate::library::Track;

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
    pub prog_bar: ProgressBar,
    pub vol_scale: Scale,
    cover_stack: Stack,
    cover_picture: Picture,
}

impl PlayerBar {
    pub fn new() -> Self {
        let root = Box::new(Orientation::Vertical, 0);

        // Fila superior: carátula + controles | info | volumen
        let top_row = Box::new(Orientation::Horizontal, 12);
        top_row.set_margin_top(10);
        top_row.set_margin_bottom(4);
        top_row.set_margin_start(12);
        top_row.set_margin_end(12);

        // --- Carátula (izquierda) ---
        let cover_stack = Stack::new();
        cover_stack.set_size_request(72, 72);
        cover_stack.set_valign(Align::Center);
        cover_stack.set_hexpand(false);
        cover_stack.set_vexpand(false);
        cover_stack.set_overflow(gtk4::Overflow::Hidden);
        cover_stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        cover_stack.set_transition_duration(200);

        let cover_picture = Picture::new();
        cover_picture.set_content_fit(ContentFit::Cover);
        cover_picture.set_can_shrink(true);
        cover_picture.set_size_request(72, 72);
        cover_picture.set_halign(Align::Fill);
        cover_picture.set_valign(Align::Fill);
        cover_picture.add_css_class("cover-art");
        cover_stack.add_named(&cover_picture, Some("art"));

        let placeholder = Image::from_icon_name("audio-x-generic-symbolic");
        placeholder.set_pixel_size(36);
        placeholder.add_css_class("dim-label");
        placeholder.add_css_class("cover-placeholder");
        cover_stack.add_named(&placeholder, Some("placeholder"));
        cover_stack.set_visible_child_name("placeholder");
        cover_stack.set_visible(false);

        // --- Controles + info juntos (centro) ---
        let center = Box::new(Orientation::Vertical, 4);
        center.set_hexpand(true);
        center.set_valign(Align::Center);

        let controls = Box::new(Orientation::Horizontal, 2);
        controls.set_halign(Align::Center);

        let btn_shuffle = Button::from_icon_name("media-playlist-shuffle-symbolic");
        btn_shuffle.add_css_class("flat");
        btn_shuffle.set_tooltip_text(Some("Aleatorio"));

        let btn_prev = Button::from_icon_name("media-skip-backward-symbolic");
        btn_prev.add_css_class("flat");
        btn_prev.set_tooltip_text(Some("Anterior"));

        let btn_play_pause = Button::from_icon_name("media-playback-start-symbolic");
        btn_play_pause.add_css_class("circular");
        btn_play_pause.add_css_class("suggested-action");
        btn_play_pause.set_tooltip_text(Some("Reproducir / Pausar"));

        let btn_next = Button::from_icon_name("media-skip-forward-symbolic");
        btn_next.add_css_class("flat");
        btn_next.set_tooltip_text(Some("Siguiente"));

        let btn_loop = Button::from_icon_name("media-playlist-repeat-symbolic");
        btn_loop.add_css_class("flat");
        btn_loop.set_tooltip_text(Some("Repetir"));

        // [shuffle][prev][ ▶ ][next][loop]  → play queda en el centro (posición 3/5)
        controls.append(&btn_shuffle);
        controls.append(&btn_prev);
        controls.append(&btn_play_pause);
        controls.append(&btn_next);
        controls.append(&btn_loop);

        let info = Box::new(Orientation::Vertical, 2);
        info.set_halign(Align::Center);

        let lbl_title = Label::new(Some("Sin reproducción"));
        lbl_title.add_css_class("heading");
        lbl_title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        lbl_title.set_max_width_chars(40);

        let lbl_artist = Label::new(Some(""));
        lbl_artist.add_css_class("dim-label");
        lbl_artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        lbl_artist.set_max_width_chars(40);

        info.append(&lbl_title);
        info.append(&lbl_artist);

        center.append(&controls);
        center.append(&info);

        // --- Volumen (derecha) ---
        let vol_box = Box::new(Orientation::Horizontal, 4);
        vol_box.set_valign(Align::Center);

        let vol_icon = Image::from_icon_name("audio-volume-high-symbolic");
        vol_icon.add_css_class("dim-label");

        let vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
        vol_scale.set_value(1.0);
        vol_scale.set_size_request(90, -1);
        vol_scale.set_draw_value(false);
        vol_scale.set_tooltip_text(Some("Volumen"));

        vol_box.append(&vol_icon);
        vol_box.append(&vol_scale);

        top_row.append(&cover_stack);
        top_row.append(&center);
        top_row.append(&vol_box);

        // Fila inferior: tiempo | barra de progreso | tiempo total
        let bottom_row = Box::new(Orientation::Horizontal, 8);
        bottom_row.set_margin_bottom(10);
        bottom_row.set_margin_start(16);
        bottom_row.set_margin_end(16);
        bottom_row.set_valign(Align::Center);

        let lbl_elapsed = Label::new(Some("0:00"));
        lbl_elapsed.add_css_class("dim-label");
        lbl_elapsed.add_css_class("caption");
        lbl_elapsed.set_width_chars(5);
        lbl_elapsed.set_xalign(1.0);

        let prog_bar = ProgressBar::new();
        prog_bar.set_hexpand(true);
        prog_bar.set_valign(Align::Center);

        let lbl_total = Label::new(Some("0:00"));
        lbl_total.add_css_class("dim-label");
        lbl_total.add_css_class("caption");
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
            vol_scale,
            cover_stack,
            cover_picture,
        }
    }

    pub fn update_track(&self, track: Option<&Track>) {
        match track {
            Some(t) => {
                self.cover_stack.set_visible(true);
                self.lbl_title.set_text(&t.display_title());
                self.lbl_artist.set_text(&t.display_artist());
                self.lbl_total.set_text(&t.duration_str());
                self.lbl_elapsed.set_text("0:00");
                self.prog_bar.set_fraction(0.0);
            }
            None => {
                self.cover_stack.set_visible(false);
                self.lbl_title.set_text("Sin reproducción");
                self.lbl_artist.set_text("");
                self.lbl_total.set_text("0:00");
                self.lbl_elapsed.set_text("0:00");
                self.prog_bar.set_fraction(0.0);
            }
        }
    }

    pub fn update_cover(&self, bytes: Option<&[u8]>) {
        if let Some(data) = bytes {
            let gbytes = glib::Bytes::from(data);
            match gdk::Texture::from_bytes(&gbytes) {
                Ok(texture) => {
                    self.cover_picture.set_paintable(Some(&texture));
                    self.cover_stack.set_visible_child_name("art");
                    return;
                }
                Err(e) => log::warn!("cover art: fallo al cargar textura GDK: {e}"),
            }
        }
        self.cover_stack.set_visible_child_name("placeholder");
    }

    pub fn update_progress(&self, elapsed_secs: f64, total_secs: f64) {
        self.lbl_elapsed.set_text(&fmt_duration(elapsed_secs as u64));
        if total_secs > 0.0 {
            self.prog_bar.set_fraction((elapsed_secs / total_secs).clamp(0.0, 1.0));
        }
    }

    pub fn set_playing(&self, playing: bool) {
        let icon = if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        self.btn_play_pause.set_icon_name(icon);
    }
}

fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
