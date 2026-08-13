//! Last.fm application credentials, compiled into the binary.
//!
//! Emptying either constant builds without Last.fm support: the app hides the
//! feature instead of failing at runtime (see `LastFmClient::is_configured`).

pub const API_KEY: &str = "e09764b551700d1f1d19bb248ecd6936";
pub const API_SECRET: &str = "049ccec7c034a10dc07c4c1caa60dc36";
