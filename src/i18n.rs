pub use gettextrs::{gettext, ngettext};

pub fn init(lang_override: Option<&str>) {
    // Use the LANGUAGE env var: GNU gettext checks it before LC_ALL/LANG on every
    // platform (Linux, Windows, macOS), so we never need OS-specific locale strings.
    // For "en": no en.mo exists, so gettext falls back to the original English msgids.
    // For system default (None): remove the override and let the OS locale decide.
    match lang_override {
        Some(lang) if !lang.is_empty() => std::env::set_var("LANGUAGE", lang),
        _ => std::env::remove_var("LANGUAGE"),
    }
    // setlocale("") re-reads the environment on every platform; it also satisfies
    // the internal locale-change detection that triggers gettext cache invalidation.
    gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");

    let compiled_dir = env!("AUDRA_LOCALE_DIR");
    let dir = if std::path::Path::new(compiled_dir).exists() {
        compiled_dir.to_string()
    } else {
        ["/usr/local/share/locale", "/usr/share/locale"]
            .iter()
            .find(|p| {
                std::path::Path::new(p)
                    .join("es/LC_MESSAGES/audra.mo")
                    .exists()
            })
            .map(|p| p.to_string())
            .unwrap_or_else(|| compiled_dir.to_string())
    };

    let _ = gettextrs::bindtextdomain("audra", &dir);
    let _ = gettextrs::bind_textdomain_codeset("audra", "UTF-8");
    let _ = gettextrs::textdomain("audra");
}
