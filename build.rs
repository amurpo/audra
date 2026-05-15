use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_API_KEY");
    println!("cargo:rerun-if-env-changed=LASTFM_API_SECRET");

    let out_dir = env::var("OUT_DIR").unwrap();
    let api_key = env::var("LASTFM_API_KEY").unwrap_or_default();
    let api_secret = env::var("LASTFM_API_SECRET").unwrap_or_default();

    let content = format!(
        "pub const API_KEY: &str = \"{}\";\npub const API_SECRET: &str = \"{}\";\n",
        api_key, api_secret
    );

    fs::write(Path::new(&out_dir).join("credentials_gen.rs"), content).unwrap();
}
