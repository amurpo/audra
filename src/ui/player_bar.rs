use gtk4::prelude::*;
use gtk4::{
    Box, Button, CenterBox, Image, Label, Orientation,
    ProgressBar, Scale, Align,
};
use crate::library::Track;

const COVER_SIZE: i32 = 72;

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
    cover_img: Image,
}

impl PlayerBar {
    pub fn new() -> Self {
        let root = Box::new(Orientation::Vertical, 0);
        root.set_vexpand(false);

        // --- Carátula ---
        // set_pixel_size hace que Image reporte exactamente COVER_SIZE como tamaño
        // natural sin importar qué paintable esté cargado. Es la única forma
        // en GTK4 de fijar el tamaño máximo de un Image sin subclassing ni CSS max-*.
        let cover_img = Image::from_icon_name("audio-x-generic-symbolic");
        cover_img.set_pixel_size(COVER_SIZE);
        cover_img.add_css_class("dim-label");
        cover_img.set_hexpand(false);
        cover_img.set_vexpand(false);

        // Wrapper para overflow:hidden + border-radius (Image solo no puede clipear)
        let cover_wrap = Box::new(Orientation::Horizontal, 0);
        cover_wrap.add_css_class("cover-thumb");
        cover_wrap.set_size_request(COVER_SIZE, COVER_SIZE);
        cover_wrap.set_hexpand(false);
        cover_wrap.set_vexpand(false);
        cover_wrap.set_halign(Align::Start);
        cover_wrap.set_valign(Align::Center);
        cover_wrap.set_overflow(gtk4::Overflow::Hidden);
        cover_wrap.append(&cover_img);

        // --- Zona central: controles + info ---
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
        vol_box.set_hexpand(false);

        let vol_icon = Image::from_icon_name("audio-volume-high-symbolic");
        vol_icon.add_css_class("dim-label");

        let vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
        vol_scale.set_value(1.0);
        vol_scale.set_size_request(90, -1);
        vol_scale.set_draw_value(false);
        vol_scale.set_tooltip_text(Some("Volumen"));

        vol_box.append(&vol_icon);
        vol_box.append(&vol_scale);

        // CenterBox: centra los controles sin importar el ancho de cover o volumen
        let top_row = CenterBox::new();
        top_row.set_vexpand(false);
        top_row.set_margin_top(8);
        top_row.set_margin_bottom(4);
        top_row.set_margin_start(16);
        top_row.set_margin_end(16);
        top_row.set_start_widget(Some(&cover_wrap));
        top_row.set_center_widget(Some(&center));
        top_row.set_end_widget(Some(&vol_box));

        // --- Barra de progreso ---
        let bottom_row = Box::new(Orientation::Horizontal, 8);
        bottom_row.set_vexpand(false);
        bottom_row.set_margin_bottom(8);
        bottom_row.set_margin_start(20);
        bottom_row.set_margin_end(20);
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
            cover_img,
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
                self.cover_img.set_icon_name(Some("audio-x-generic-symbolic"));
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
            if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&gbytes) {
                // pixel_size ya está en COVER_SIZE: Image renderiza la textura
                // a ese tamaño exacto sin importar las dimensiones originales
                self.cover_img.set_paintable(Some(&texture));
                self.cover_img.remove_css_class("dim-label");
                return;
            }
        }
        self.cover_img.set_icon_name(Some("audio-x-generic-symbolic"));
        self.cover_img.add_css_class("dim-label");
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
