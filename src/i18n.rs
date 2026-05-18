pub use gettextrs::{gettext, ngettext};

pub fn init() {
    gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    let _ = gettextrs::bindtextdomain("audra", env!("AUDRA_LOCALE_DIR"));
    let _ = gettextrs::bind_textdomain_codeset("audra", "UTF-8");
    let _ = gettextrs::textdomain("audra");
}
