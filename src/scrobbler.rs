use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

const PROXY_URL: &str = crate::credentials::PROXY_URL;

pub struct LastFmClient {
    session_key: Option<String>,
    client: Client,
}

#[derive(Deserialize)]
pub struct AuthTokenResponse {
    pub token: String,
    pub auth_url: String,
}

#[derive(Deserialize)]
pub struct AuthSessionResponse {
    pub session_key: String,
    pub username: String,
}

/// Extract the human-readable error message from a failed proxy response.
/// Tries to parse `{"error": "..."}` JSON; falls back to the raw body.
fn proxy_error(resp: reqwest::blocking::Response) -> anyhow::Error {
    let text = resp.text().unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(str::to_string))
        .unwrap_or(text);
    anyhow::anyhow!("{}", msg)
}

impl LastFmClient {
    pub fn new() -> Self {
        Self {
            session_key: None,
            client: Client::new(),
        }
    }

    pub fn with_session(mut self, session_key: &str) -> Self {
        self.session_key = Some(session_key.to_string());
        self
    }

    pub fn is_configured() -> bool {
        !PROXY_URL.is_empty()
    }

    pub fn session_key(&self) -> Option<&str> {
        self.session_key.as_deref()
    }

    pub fn get_auth_token() -> Result<AuthTokenResponse> {
        let proxy = PROXY_URL.trim_end_matches('/');
        let resp = Client::new().get(format!("{proxy}/auth/token")).send()?;
        if !resp.status().is_success() {
            return Err(proxy_error(resp));
        }
        Ok(resp.json()?)
    }

    pub fn get_session(token: &str) -> Result<AuthSessionResponse> {
        let proxy = PROXY_URL.trim_end_matches('/');
        let resp = Client::new()
            .get(format!("{proxy}/auth/session"))
            .query(&[("token", token)])
            .send()?;
        if !resp.status().is_success() {
            return Err(proxy_error(resp));
        }
        Ok(resp.json()?)
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let sk = self
            .session_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión Last.fm"))?;

        let proxy = PROXY_URL.trim_end_matches('/');
        let body = json!({
            "sk": sk,
            "artist": artist,
            "track": track,
            "album": album,
            "timestamp": timestamp,
        });
        let resp = self
            .client
            .post(format!("{proxy}/scrobble"))
            .json(&body)
            .send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Last.fm scrobble error: {}", resp.status());
        }
        Ok(())
    }

    pub fn update_now_playing(&self, artist: &str, track: &str, album: &str) {
        let sk = match self.session_key.as_deref() {
            Some(s) => s.to_string(),
            None => return,
        };

        let proxy = PROXY_URL.trim_end_matches('/');
        let body = json!({
            "sk": sk,
            "artist": artist,
            "track": track,
            "album": album,
        });
        let _ = self
            .client
            .post(format!("{proxy}/nowplaying"))
            .json(&body)
            .send();
    }

    // Takes the shared DB handle and locks it only briefly per operation, so
    // the connection mutex is never held across a blocking network request.
    pub fn flush_queue(&self, db: &std::sync::Arc<std::sync::Mutex<crate::library::db::Database>>) {
        let pending = match db.lock().unwrap().pending_scrobbles() {
            Ok(p) if !p.is_empty() => p,
            _ => return,
        };

        log::info!(
            "scrobbler: {} scrobble(s) pendiente(s), enviando…",
            pending.len()
        );

        for (queue_id, track, played_at) in pending {
            let artist = track.artist.clone().unwrap_or_default();
            let title = track.title.clone().unwrap_or_default();
            let album = track.album.clone().unwrap_or_default();
            let ts: i64 = played_at.parse().unwrap_or(0);

            match self.scrobble(&artist, &title, &album, ts) {
                Ok(()) => {
                    let _ = db.lock().unwrap().remove_scrobble(queue_id);
                    log::debug!("scrobbler: flush OK '{}' - '{}'", artist, title);
                }
                Err(e) => {
                    log::warn!("scrobbler: flush falló, abortando — {}", e);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn new_client_has_no_session() {
        let c = LastFmClient::new();
        assert_eq!(c.session_key(), None);
    }

    #[test]
    fn with_session_sets_the_key() {
        let c = LastFmClient::new().with_session("abc123");
        assert_eq!(c.session_key(), Some("abc123"));
    }

    #[test]
    fn is_configured_reflects_proxy_url_constant() {
        // Mirrors the build-time credential: empty when LASTFM_PROXY_URL is unset.
        assert_eq!(LastFmClient::is_configured(), !PROXY_URL.is_empty());
    }

    #[test]
    fn scrobble_without_session_errors_before_any_network_call() {
        let c = LastFmClient::new();
        let err = c.scrobble("A", "T", "Al", 0).unwrap_err();
        assert!(err.to_string().contains("sin sesión"));
    }

    #[test]
    fn update_now_playing_without_session_is_a_silent_noop() {
        // No session => returns early, never touches the network.
        LastFmClient::new().update_now_playing("A", "T", "Al");
    }

    #[test]
    fn flush_queue_with_empty_db_does_nothing() {
        let db = Arc::new(Mutex::new(
            crate::library::db::Database::open(":memory:").unwrap(),
        ));
        // Empty queue => early return, so the missing session never matters.
        LastFmClient::new().flush_queue(&db);
        assert!(db.lock().unwrap().pending_scrobbles().unwrap().is_empty());
    }

    #[test]
    fn auth_token_response_deserializes() {
        let r: AuthTokenResponse =
            serde_json::from_str(r#"{"token":"tok","auth_url":"https://x/y"}"#).unwrap();
        assert_eq!(r.token, "tok");
        assert_eq!(r.auth_url, "https://x/y");
    }

    #[test]
    fn auth_session_response_deserializes() {
        let r: AuthSessionResponse =
            serde_json::from_str(r#"{"session_key":"sk","username":"bob"}"#).unwrap();
        assert_eq!(r.session_key, "sk");
        assert_eq!(r.username, "bob");
    }
}
