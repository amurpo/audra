use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_PROXY_URL");

    let out_dir = env::var("OUT_DIR").unwrap();
    let proxy_url = env::var("LASTFM_PROXY_URL").unwrap_or_default();
    let content = format!("pub const PROXY_URL: &str = \"{}\";\n", proxy_url);
    fs::write(Path::new(&out_dir).join("credentials_gen.rs"), content).unwrap();

    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
        println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
    }
}
