pub use gettextrs::{gettext, ngettext};

pub fn init(lang_override: Option<&str>) {
    // Use the LANGUAGE env var: GNU gettext checks it before LC_ALL/LANG on every
    // platform (Linux, Windows, macOS), so we never need OS-specific locale strings.
    // For "en": no en.mo exists, so gettext falls back to the original English msgids.
    // For system default (None): remove the override and let the OS locale decide.
    // INVARIANT: must run on the main thread. GNU gettext selects the catalog
    // from the LANGUAGE env var and offers no per-domain alternative, so we
    // mutate the process environment — but only when the value actually
    // changes, to minimise the race window with background worker threads.
    let desired = lang_override.filter(|s| !s.is_empty());
    let current = std::env::var("LANGUAGE").ok();
    match desired {
        Some(lang) if current.as_deref() != Some(lang) => {
            std::env::set_var("LANGUAGE", lang)
        }
        None if current.is_some() => std::env::remove_var("LANGUAGE"),
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    // `init` mutates global process locale state entangled with the gettext C
    // library, so its real effect (catalog selection) can't be asserted
    // deterministically in a unit test without a compiled .mo and a controlled
    // OS locale. We only pin down that every input path is panic-free and that
    // repeated calls are idempotent — the contract callers actually rely on.
    #[test]
    fn init_is_panic_free_and_idempotent_for_every_input_path() {
        init(Some("es")); // explicit override
        init(Some("xx")); // unknown lang -> gettext falls back, must not panic
        init(Some("")); // empty string is treated as "no override"
        init(None); // system default
        init(None); // idempotent: a second clear must not panic
        init(Some("en")); // back to an explicit value after a clear
    }
}
