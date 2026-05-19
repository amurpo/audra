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
    // Mutate the environment through GLib, not std::env. On Windows
    // std::env::set_var only calls SetEnvironmentVariableW, which does not
    // update the C runtime's getenv snapshot that MinGW's libintl reads — so
    // the LANGUAGE override would be invisible to gettext and it would fall
    // back to the OS locale. glib::setenv updates the CRT environment too,
    // keeping selection working identically on every platform.
    let desired = lang_override.filter(|s| !s.is_empty());
    let current = std::env::var("LANGUAGE").ok();
    match desired {
        Some(lang) if current.as_deref() != Some(lang) => {
            let _ = glib::setenv("LANGUAGE", lang, true);
        }
        None if current.is_some() => glib::unsetenv("LANGUAGE"),
        _ => {}
    }
    // setlocale("") re-reads the environment on every platform; it also satisfies
    // the internal locale-change detection that triggers gettext cache invalidation.
    gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");

    // Resolve the catalog directory by probing an ordered list of candidates
    // for an actual compiled catalog. AUDRA_LOCALE_DIR is baked at build time
    // and only exists on the build machine (dev tree). Packaged builds on
    // Windows/macOS ship the catalog next to the executable, so those
    // exe-relative locations must be checked too — the /usr paths don't exist
    // there at all.
    let compiled_dir = env!("AUDRA_LOCALE_DIR");
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("share/locale"));
            candidates.push(exe_dir.join("locale"));
            if let Some(prefix) = exe_dir.parent() {
                candidates.push(prefix.join("share/locale"));
            }
        }
    }
    candidates.push(std::path::PathBuf::from(compiled_dir));
    candidates.push(std::path::PathBuf::from("/usr/local/share/locale"));
    candidates.push(std::path::PathBuf::from("/usr/share/locale"));
    let dir = candidates
        .iter()
        .find(|p| p.join("es/LC_MESSAGES/audra.mo").exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| compiled_dir.to_string());

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
