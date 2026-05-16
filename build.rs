use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_PROXY_URL");
    println!("cargo:rerun-if-env-changed=LASTFM_APP_TOKEN");

    let out_dir = env::var("OUT_DIR").unwrap();
    let proxy_url = env::var("LASTFM_PROXY_URL").unwrap_or_default();
    let app_token = env::var("LASTFM_APP_TOKEN").unwrap_or_default();

    let content = format!(
        "pub const PROXY_URL: &str = \"{}\";\npub const APP_TOKEN: &str = \"{}\";\n",
        proxy_url, app_token
    );

    fs::write(Path::new(&out_dir).join("credentials_gen.rs"), content).unwrap();
}
