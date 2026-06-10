//! Typed accessors over the `settings` key/value table.
//!
//! Every persisted setting is read and written through these methods, so the
//! key strings and the value encodings ("track"/"album"/"off", "0"/"1", …)
//! live in exactly one place. The methods are inherent on [`Database`] and
//! take `&self`, so callers use them under whatever lock they already hold —
//! a multi-setting read still costs a single `db.lock()`.

use crate::library::db::Database;
use crate::player::replaygain::{self, ReplayGainMode};
use crate::ui::theme::TintMode;
use anyhow::Result;

const MUSIC_FOLDER: &str = "music_folder";
const LANGUAGE: &str = "language";
const REPLAYGAIN: &str = "replaygain";
const DYNAMIC_COLOR: &str = "dynamic_color";
const USE_SYSTEM_FONT: &str = "use_system_font";
const VOLUME: &str = "volume";
const LASTFM_SESSION_KEY: &str = "lastfm_session_key";
const LASTFM_USERNAME: &str = "lastfm_username";

impl Database {
    pub fn music_folder(&self) -> Option<String> {
        self.get_setting(MUSIC_FOLDER)
    }

    pub fn set_music_folder(&self, folder: &str) -> Result<()> {
        self.set_setting(MUSIC_FOLDER, folder)
    }

    /// `None` means "follow the system locale" (stored as the empty string).
    pub fn language(&self) -> Option<String> {
        self.get_setting(LANGUAGE).filter(|s| !s.is_empty())
    }

    pub fn set_language(&self, lang: Option<&str>) -> Result<()> {
        self.set_setting(LANGUAGE, lang.unwrap_or(""))
    }

    /// `None` means ReplayGain off; unknown/legacy values also read as off.
    pub fn replaygain(&self) -> Option<ReplayGainMode> {
        replaygain::mode_from_setting(&self.get_setting(REPLAYGAIN).unwrap_or_default())
    }

    pub fn set_replaygain(&self, mode: Option<ReplayGainMode>) -> Result<()> {
        self.set_setting(REPLAYGAIN, replaygain::mode_as_setting(mode))
    }

    pub fn dynamic_color(&self) -> TintMode {
        TintMode::from_setting(&self.get_setting(DYNAMIC_COLOR).unwrap_or_default())
    }

    pub fn set_dynamic_color(&self, mode: TintMode) -> Result<()> {
        self.set_setting(DYNAMIC_COLOR, mode.as_setting())
    }

    pub fn use_system_font(&self) -> bool {
        self.get_setting(USE_SYSTEM_FONT).as_deref() == Some("1")
    }

    pub fn set_use_system_font(&self, on: bool) -> Result<()> {
        self.set_setting(USE_SYSTEM_FONT, if on { "1" } else { "0" })
    }

    pub fn volume(&self) -> f64 {
        self.get_setting(VOLUME)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.5)
    }

    pub fn set_volume(&self, value: f64) -> Result<()> {
        self.set_setting(VOLUME, &value.to_string())
    }

    pub fn lastfm_session_key(&self) -> Option<String> {
        self.get_setting(LASTFM_SESSION_KEY)
            .filter(|s| !s.is_empty())
    }

    pub fn lastfm_username(&self) -> Option<String> {
        self.get_setting(LASTFM_USERNAME).filter(|s| !s.is_empty())
    }

    pub fn set_lastfm_session(&self, session_key: &str, username: &str) -> Result<()> {
        self.set_setting(LASTFM_SESSION_KEY, session_key)?;
        self.set_setting(LASTFM_USERNAME, username)
    }

    pub fn clear_lastfm_session(&self) -> Result<()> {
        self.delete_setting(LASTFM_SESSION_KEY)?;
        self.delete_setting(LASTFM_USERNAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::open(":memory:").expect("open in-memory db")
    }

    #[test]
    fn defaults_when_unset() {
        let db = db();
        assert_eq!(db.music_folder(), None);
        assert_eq!(db.language(), None);
        assert_eq!(db.replaygain(), None);
        assert_eq!(db.dynamic_color(), TintMode::Partial);
        assert!(!db.use_system_font());
        assert_eq!(db.volume(), 0.5);
        assert_eq!(db.lastfm_session_key(), None);
        assert_eq!(db.lastfm_username(), None);
    }

    #[test]
    fn roundtrips() {
        let db = db();
        db.set_music_folder("/music").unwrap();
        assert_eq!(db.music_folder().as_deref(), Some("/music"));

        db.set_language(Some("es")).unwrap();
        assert_eq!(db.language().as_deref(), Some("es"));
        // Auto is stored as the empty string and reads back as None.
        db.set_language(None).unwrap();
        assert_eq!(db.language(), None);

        db.set_replaygain(Some(ReplayGainMode::Album)).unwrap();
        assert_eq!(db.replaygain(), Some(ReplayGainMode::Album));
        db.set_replaygain(None).unwrap();
        assert_eq!(db.replaygain(), None);

        db.set_dynamic_color(TintMode::Full).unwrap();
        assert_eq!(db.dynamic_color(), TintMode::Full);

        db.set_use_system_font(true).unwrap();
        assert!(db.use_system_font());

        db.set_volume(0.75).unwrap();
        assert_eq!(db.volume(), 0.75);
    }

    #[test]
    fn lastfm_session_set_and_clear() {
        let db = db();
        db.set_lastfm_session("sk123", "alice").unwrap();
        assert_eq!(db.lastfm_session_key().as_deref(), Some("sk123"));
        assert_eq!(db.lastfm_username().as_deref(), Some("alice"));
        db.clear_lastfm_session().unwrap();
        assert_eq!(db.lastfm_session_key(), None);
        assert_eq!(db.lastfm_username(), None);
    }
}
