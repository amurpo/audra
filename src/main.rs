mod credentials;
mod i18n;
mod library;
mod player;
mod scrobbler;
mod ui;

use adw::prelude::*;
use libadwaita as adw;
use library::db::Database;
use std::sync::{Arc, Mutex};

const APP_ID: &str = "com.audra.player";

fn main() {
    env_logger::init();

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("audra");
    std::fs::create_dir_all(&data_dir).ok();
    let db_path = data_dir.join("library.db");

    let db = Arc::new(Mutex::new(
        Database::open(&db_path).expect("No se pudo abrir la base de datos"),
    ));

    // One-time, idempotent migration of cover/photo keys to their canonical
    // (deduplicated) form so user-picked images survive the new grouping.
    {
        let g = db.lock().unwrap();
        if let Ok(tracks) = g.all_tracks() {
            let mf = g.get_setting("music_folder");
            let cover_map = library::dedup::canonical_key_map(&tracks, mf.as_deref());
            let _ = g.migrate_cover_keys(&cover_map);
            for (old, new) in library::dedup::canonical_artist_map(&tracks, mf.as_deref()) {
                library::metadata::rekey_artist_photo(&old, &new);
            }
        }
    }

    // Read language preference before i18n so setlocale uses the correct value
    let lang = db.lock().unwrap().get_setting("language");
    i18n::init(lang.as_deref().filter(|s| !s.is_empty()));

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::empty())
        .build();

    let db_ref = Arc::clone(&db);
    app.connect_activate(move |app| {
        ui::main_window::build_window(app, Arc::clone(&db_ref));
    });

    app.run();
}
