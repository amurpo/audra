use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn compile_po(lang: &str, out_dir: &str) {
    let po_path = format!("po/{}.po", lang);
    let mo_dir = format!("{}/locale/{}/LC_MESSAGES", out_dir, lang);
    fs::create_dir_all(&mo_dir).expect("create locale dir");
    let mo_path = format!("{}/audra.mo", mo_dir);
    let status = Command::new("msgfmt")
        .args([&po_path, "-o", &mo_path])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!(
            "msgfmt failed compiling {po_path} ({s}). The 'gettext' package is required to build."
        ),
        Err(e) => panic!(
            "msgfmt not found ({e}). Install the 'gettext' package \
             (Debian/Ubuntu: apt install gettext, Fedora: dnf install gettext)."
        ),
    }
    println!("cargo:rerun-if-changed={}", po_path);
}

fn main() {
    println!("cargo:rustc-link-lib=fontconfig");
    println!("cargo:rerun-if-env-changed=LASTFM_PROXY_URL");

    // The bundled Remix SVGs are only embedded on macOS (see src/ui/icons.rs).
    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "macos" {
        println!("cargo:rerun-if-changed=data/icons/remix");
    }

    let out_dir = env::var("OUT_DIR").unwrap();

    compile_po("es", &out_dir);
    println!("cargo:rerun-if-env-changed=LOCALEDIR");
    let locale_dir = env::var("LOCALEDIR").unwrap_or_else(|_| format!("{}/locale", out_dir));
    println!("cargo:rustc-env=AUDRA_LOCALE_DIR={}", locale_dir);

    let proxy_url = env::var("LASTFM_PROXY_URL").unwrap_or_default();
    // {:?} emits a Rust string literal with quotes and backslashes escaped, so
    // a hostile LASTFM_PROXY_URL containing `"` cannot break out of the literal
    // or inject extra items into the generated module.
    let content = format!("pub const PROXY_URL: &str = {:?};\n", proxy_url);
    fs::write(Path::new(&out_dir).join("credentials_gen.rs"), content).unwrap();

    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        println!("cargo:rustc-link-arg=-mwindows");

        let mut res = winres::WindowsResource::new();
        res.set_icon("data/icons/audra.ico");
        res.set("FileDescription", "Audra Music Player");
        res.set("ProductName", "Audra");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.compile().unwrap();
    }
}
