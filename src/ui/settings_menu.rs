//! The header-bar settings popover: scan/refresh, Last.fm account, the
//! appearance setting rows (font, ReplayGain, dynamic color, language),
//! library reset and the About dialog.
//!
//! Extracted from `build_window` so the window builder stays at orchestration
//! altitude. Construction and signal wiring live together here; in
//! `build_window` they were two distant blocks only because the views and
//! scan widgets the handlers capture did not exist yet when the header was
//! built.

use adw::prelude::*;
use glib::clone;
use gtk4::prelude::*;
use gtk4::{gio, Button, FileDialog, MenuButton, Popover};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::player::{
    replaygain::{self, ReplayGainMode},
    Player,
};
use crate::scrobbler::LastFmClient;
use crate::ui::lastfm_dialog::show_lastfm_dialog;
use crate::ui::main_window::{start_scan, Views};
use crate::ui::reset::show_reset_dialog;
use crate::ui::theme::{set_tint_mode, update_font, TintMode};
use crate::ui::widgets::segmented_setting_row;

/// Everything the popover rows and handlers capture. All fields are cheap
/// `Rc`/`Arc`/GObject clones; the `*_init` fields are the persisted settings
/// the caller already read from the DB (the DB handle itself comes via
/// `views.db`).
pub(crate) struct SettingsMenuCtx {
    pub window: adw::ApplicationWindow,
    pub views: Views,
    pub scan_loading_box: gtk4::Box,
    pub scan_spinner: gtk4::Spinner,
    pub lastfm: Arc<Mutex<Option<LastFmClient>>>,
    pub player: Rc<RefCell<Player>>,
    pub apply_language: Rc<dyn Fn(Option<&'static str>)>,
    pub use_system_font: bool,
    pub replaygain_init: Option<ReplayGainMode>,
    pub dyn_color_init: TintMode,
    pub lang_init: Option<&'static str>,
}

/// Build the settings `MenuButton` (icon, popover, rows, handlers) ready to
/// be packed into the header bar.
pub(crate) fn build(ctx: SettingsMenuCtx) -> MenuButton {
    let SettingsMenuCtx {
        window,
        views,
        scan_loading_box,
        scan_spinner,
        lastfm,
        player,
        apply_language,
        use_system_font,
        replaygain_init,
        dyn_color_init,
        lang_init,
    } = ctx;
    let db = Arc::clone(&views.db);

    let menu_btn = MenuButton::new();
    let menu_icon = crate::ui::icons::image(crate::ui::icons::Icon::FolderMusic, 20);
    menu_btn.set_child(Some(&menu_icon));
    menu_btn.set_tooltip_text(Some(&gettext("Library")));
    menu_btn.add_css_class("flat");

    let popover = Popover::new();
    popover.add_css_class("audra-shaded");
    let pop_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    pop_box.set_margin_top(4);
    pop_box.set_margin_bottom(4);
    pop_box.set_margin_start(4);
    pop_box.set_margin_end(4);
    // Fixed width so the popover does not resize when labels change length
    // across languages.
    pop_box.set_size_request(264, -1);

    let scan_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);

    let item_scan = Button::with_label(&gettext("Scan collection"));
    item_scan.add_css_class("flat");
    item_scan.set_hexpand(true);
    item_scan.set_halign(gtk4::Align::Fill);
    item_scan.connect_clicked(clone!(
        #[strong]
        window,
        #[strong]
        views,
        #[weak]
        popover,
        #[weak]
        scan_loading_box,
        #[weak]
        scan_spinner,
        move |_| {
            popover.popdown();
            let dialog = FileDialog::new();
            dialog.select_folder(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[strong]
                    views,
                    #[weak]
                    scan_loading_box,
                    #[weak]
                    scan_spinner,
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                start_scan(
                                    path.to_string_lossy().to_string(),
                                    views.clone(),
                                    scan_loading_box,
                                    scan_spinner,
                                );
                            }
                        }
                    }
                ),
            );
        }
    ));

    let item_refresh =
        crate::ui::icons::flat_icon_button(crate::ui::icons::Icon::Refresh, 20, None);
    item_refresh.add_css_class("flat");
    item_refresh.set_tooltip_text(Some(&gettext("Refresh collection")));
    item_refresh.connect_clicked(clone!(
        #[strong]
        views,
        #[weak]
        popover,
        #[weak]
        scan_loading_box,
        #[weak]
        scan_spinner,
        move |_| {
            popover.popdown();
            if let Some(folder) = views.db.lock().unwrap().get_setting("music_folder") {
                start_scan(folder, views.clone(), scan_loading_box, scan_spinner);
            }
        }
    ));

    scan_row.append(&item_scan);
    scan_row.append(&item_refresh);

    let pop_sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep.set_margin_top(4);
    pop_sep.set_margin_bottom(4);

    let item_lastfm = Button::with_label(&gettext("Last.fm Account"));
    item_lastfm.add_css_class("flat");
    item_lastfm.set_halign(gtk4::Align::Fill);
    item_lastfm.connect_clicked(clone!(
        #[strong]
        window,
        #[strong]
        db,
        #[strong]
        lastfm,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            show_lastfm_dialog(&window, Arc::clone(&db), Arc::clone(&lastfm));
        }
    ));

    let pop_sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep2.set_margin_top(4);
    pop_sep2.set_margin_bottom(4);

    let font_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    font_row.set_margin_top(2);
    font_row.set_margin_bottom(2);
    font_row.set_margin_start(8);
    font_row.set_margin_end(8);
    let font_label = gtk4::Label::new(Some(&gettext("System font")));
    font_label.set_hexpand(true);
    font_label.set_xalign(0.0);
    let font_switch = gtk4::Switch::new();
    font_switch.set_active(use_system_font);
    font_switch.set_valign(gtk4::Align::Center);
    font_switch.connect_state_set(clone!(
        #[strong]
        db,
        move |_, state| {
            let _ = db
                .lock()
                .unwrap()
                .set_setting("use_system_font", if state { "1" } else { "0" });
            update_font(!state);
            glib::Propagation::Proceed
        }
    ));
    font_row.append(&font_label);
    font_row.append(&font_switch);

    let rg_row = segmented_setting_row(
        &gettext("ReplayGain"),
        &[
            (gettext("Off"), None),
            (gettext("Track"), Some(ReplayGainMode::Track)),
            (gettext("Album"), Some(ReplayGainMode::Album)),
        ],
        replaygain_init,
        {
            let db = Arc::clone(&db);
            let player = Rc::clone(&player);
            move |mode| {
                player.borrow_mut().replaygain_mode = mode;
                let _ = db
                    .lock()
                    .unwrap()
                    .set_setting("replaygain", replaygain::mode_as_setting(mode));
            }
        },
    );

    let dc_row = segmented_setting_row(
        &gettext("Dynamic color"),
        &[
            (gettext("Off"), TintMode::Off),
            (gettext("Partial"), TintMode::Partial),
            (gettext("Full"), TintMode::Full),
        ],
        dyn_color_init,
        {
            let db = Arc::clone(&db);
            move |mode: TintMode| {
                let _ = db
                    .lock()
                    .unwrap()
                    .set_setting("dynamic_color", mode.as_setting());
                set_tint_mode(mode);
            }
        },
    );

    let pop_sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep3.set_margin_top(14);
    pop_sep3.set_margin_bottom(3);

    let lang_row = segmented_setting_row(
        &gettext("Language"),
        &[
            ("Auto".to_string(), None),
            ("English".to_string(), Some("en")),
            ("Español".to_string(), Some("es")),
        ],
        lang_init,
        {
            let apply_language = Rc::clone(&apply_language);
            move |lang| apply_language(lang)
        },
    );

    let item_reset = Button::new();
    item_reset.add_css_class("flat");
    item_reset.set_halign(gtk4::Align::Fill);
    item_reset.set_margin_top(3);
    let reset_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let reset_icon = crate::ui::icons::image(crate::ui::icons::Icon::DeleteBin, 16);
    reset_icon.add_css_class("menu-destructive");
    {
        let reset_icon = reset_icon.clone();
        reset_icon.connect_realize(move |img| {
            crate::ui::icons::set_image_icon(
                img,
                crate::ui::icons::Icon::DeleteBin,
                16,
                &crate::ui::icons::error_color(img),
            );
        });
    }
    let reset_lbl = gtk4::Label::new(Some(&gettext("Reset library…")));
    reset_lbl.add_css_class("menu-destructive");
    reset_box.append(&reset_icon);
    reset_box.append(&reset_lbl);
    item_reset.set_child(Some(&reset_box));
    item_reset.connect_clicked(clone!(
        #[strong]
        window,
        #[strong]
        views,
        #[strong]
        scan_loading_box,
        #[strong]
        scan_spinner,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            show_reset_dialog(
                &window,
                views.clone(),
                scan_loading_box.clone(),
                scan_spinner.clone(),
            );
        }
    ));

    let pop_sep4 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    pop_sep4.set_margin_top(4);
    pop_sep4.set_margin_bottom(4);

    let item_about = Button::with_label(&gettext("About Audra"));
    item_about.add_css_class("flat");
    item_about.set_halign(gtk4::Align::Fill);
    item_about.connect_clicked(clone!(
        #[strong]
        window,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            let about = adw::AboutDialog::builder()
                .application_name("Audra")
                .application_icon("io.github.amurpo.audra")
                .developer_name("Daniel Avila")
                .version(env!("CARGO_PKG_VERSION"))
                .comments(gettext("Native music player with Last.fm scrobbling"))
                .copyright("© Daniel Avila")
                .license_type(gtk4::License::Gpl30)
                .website("https://amurpo.github.io/audra/")
                .issue_url("https://github.com/amurpo/audra/issues")
                .translator_credits(gettext("translator-credits"))
                .build();
            about.add_credit_section(
                Some(&gettext("Acknowledgments")),
                &[&format!(
                    "{} https://github.com/amurpo/audra",
                    gettext("View on GitHub")
                )],
            );
            about.add_css_class("audra-shaded");
            about.present(Some(&window));
        }
    ));

    pop_box.append(&scan_row);
    pop_box.append(&pop_sep);
    pop_box.append(&item_lastfm);
    pop_box.append(&pop_sep2);
    pop_box.append(&font_row);
    pop_box.append(&rg_row);
    pop_box.append(&dc_row);
    pop_box.append(&lang_row);
    pop_box.append(&pop_sep3);
    pop_box.append(&item_reset);
    pop_box.append(&pop_sep4);
    pop_box.append(&item_about);
    popover.set_child(Some(&pop_box));
    menu_btn.set_popover(Some(&popover));

    menu_btn
}
