use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::credentials::{API_KEY, API_SECRET};

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_ROOT: &str = "https://www.last.fm/api/auth/";

pub struct LastFmClient {
    session_key: Option<String>,
    client: Client,
}

pub struct AuthTokenResponse {
    pub token: String,
    pub auth_url: String,
}

pub struct AuthSessionResponse {
    pub session_key: String,
    pub username: String,
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

#[derive(Deserialize)]
struct SessionBody {
    session: SessionInner,
}

#[derive(Deserialize)]
struct SessionInner {
    name: String,
    key: String,
}

/// Build the `api_sig` Last.fm requires on every authenticated call: sort the
/// parameters by name, concatenate `name` and value with no separators, append
/// the shared secret, and MD5 the result.
///
/// `format` and `api_sig` itself are excluded by construction — they are only
/// added afterwards, by `signed_params`.
fn sign(params: &[(&str, String)]) -> String {
    let mut sorted: Vec<&(&str, String)> = params.iter().collect();
    sorted.sort_by_key(|(name, _)| *name);

    let mut buf = String::new();
    for (name, value) in sorted {
        buf.push_str(name);
        buf.push_str(value);
    }
    buf.push_str(API_SECRET);

    format!("{:x}", md5::compute(buf.as_bytes()))
}

/// Sign `params` and append the signature plus `format=json`, giving the full
/// parameter list to send.
fn signed_params(mut params: Vec<(&'static str, String)>) -> Vec<(&'static str, String)> {
    let sig = sign(&params);
    params.push(("api_sig", sig));
    params.push(("format", "json".to_string()));
    params
}

/// Turn a Last.fm reply into an error when it carries one.
///
/// The API reports failures as `{"error": 14, "message": "..."}`, and does not
/// always pair them with a non-2xx status, so the body is what decides.
fn check_api_error(body: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct ApiError {
        // Required so that a successful reply carrying a `message` field of its
        // own is never mistaken for a failure.
        #[allow(dead_code)]
        error: i64,
        message: String,
    }

    if let Ok(e) = serde_json::from_str::<ApiError>(body) {
        anyhow::bail!("{}", e.message);
    }
    Ok(())
}

/// Read a response body, failing on transport errors, HTTP errors and the
/// API's own in-band error replies alike.
fn read_body(resp: reqwest::blocking::Response) -> Result<String> {
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    check_api_error(&body)?;
    if !status.is_success() {
        anyhow::bail!("Last.fm HTTP {}", status);
    }
    Ok(body)
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
        !API_KEY.is_empty() && !API_SECRET.is_empty()
    }

    pub fn session_key(&self) -> Option<&str> {
        self.session_key.as_deref()
    }

    pub fn get_auth_token() -> Result<AuthTokenResponse> {
        let params = signed_params(vec![
            ("api_key", API_KEY.to_string()),
            ("method", "auth.getToken".to_string()),
        ]);

        let resp = Client::new().get(API_ROOT).query(&params).send()?;
        let body = read_body(resp)?;
        let token = serde_json::from_str::<TokenBody>(&body)?.token;

        // The user opens this in a browser to approve the token; only then does
        // auth.getSession trade it for a session key.
        let auth_url = format!("{AUTH_ROOT}?api_key={API_KEY}&token={token}");
        Ok(AuthTokenResponse { token, auth_url })
    }

    pub fn get_session(token: &str) -> Result<AuthSessionResponse> {
        let params = signed_params(vec![
            ("api_key", API_KEY.to_string()),
            ("method", "auth.getSession".to_string()),
            ("token", token.to_string()),
        ]);

        let resp = Client::new().get(API_ROOT).query(&params).send()?;
        let body = read_body(resp)?;
        let session = serde_json::from_str::<SessionBody>(&body)?.session;

        Ok(AuthSessionResponse {
            session_key: session.key,
            username: session.name,
        })
    }

    /// Parameters shared by scrobble and now-playing: the track itself plus the
    /// credentials. `album` is dropped when empty — Last.fm rejects blank
    /// optional fields rather than ignoring them.
    fn track_params(
        &self,
        method: &'static str,
        artist: &str,
        track: &str,
        album: &str,
    ) -> Result<Vec<(&'static str, String)>> {
        let sk = self
            .session_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión Last.fm"))?;

        if artist.is_empty() || track.is_empty() {
            anyhow::bail!("el track no tiene artista o título");
        }

        let mut params = vec![
            ("api_key", API_KEY.to_string()),
            ("artist", artist.to_string()),
            ("method", method.to_string()),
            ("sk", sk.to_string()),
            ("track", track.to_string()),
        ];
        if !album.is_empty() {
            params.push(("album", album.to_string()));
        }
        Ok(params)
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let mut params = self.track_params("track.scrobble", artist, track, album)?;
        params.push(("timestamp", timestamp.to_string()));

        let resp = self
            .client
            .post(API_ROOT)
            .form(&signed_params(params))
            .send()?;
        read_body(resp)?;
        Ok(())
    }

    pub fn update_now_playing(&self, artist: &str, track: &str, album: &str) {
        let params = match self.track_params("track.updateNowPlaying", artist, track, album) {
            Ok(p) => p,
            Err(_) => return,
        };

        let _ = self
            .client
            .post(API_ROOT)
            .form(&signed_params(params))
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
    fn is_configured_reflects_the_embedded_credentials() {
        assert_eq!(
            LastFmClient::is_configured(),
            !API_KEY.is_empty() && !API_SECRET.is_empty()
        );
    }

    #[test]
    fn signature_sorts_parameters_by_name() {
        // Given out of order, the digest must still be the one for the
        // alphabetical concatenation.
        let sig = sign(&[
            ("method", "auth.getToken".to_string()),
            ("api_key", "KEY".to_string()),
        ]);
        let expected = format!(
            "{:x}",
            md5::compute(format!("api_keyKEYmethodauth.getToken{API_SECRET}").as_bytes())
        );
        assert_eq!(sig, expected);
    }

    #[test]
    fn signed_params_appends_signature_and_format_without_signing_them() {
        let base = vec![
            ("api_key", "KEY".to_string()),
            ("method", "auth.getToken".to_string()),
        ];
        let full = signed_params(base.clone());

        let sig = full
            .iter()
            .find(|(k, _)| *k == "api_sig")
            .map(|(_, v)| v.clone())
            .unwrap();
        // Signing the base alone reproduces it, so neither api_sig nor format
        // was part of the signed input.
        assert_eq!(sig, sign(&base));
        assert!(full.iter().any(|(k, v)| *k == "format" && v == "json"));
    }

    #[test]
    fn track_params_omits_an_empty_album() {
        let c = LastFmClient::new().with_session("sk");
        let p = c
            .track_params("track.scrobble", "Artist", "Title", "")
            .unwrap();
        assert!(!p.iter().any(|(k, _)| *k == "album"));

        let p = c
            .track_params("track.scrobble", "Artist", "Title", "Album")
            .unwrap();
        assert!(p.iter().any(|(k, v)| *k == "album" && v == "Album"));
    }

    #[test]
    fn scrobble_without_session_errors_before_any_network_call() {
        let c = LastFmClient::new();
        let err = c.scrobble("A", "T", "Al", 0).unwrap_err();
        assert!(err.to_string().contains("sin sesión"));
    }

    #[test]
    fn scrobble_without_artist_or_title_errors_before_any_network_call() {
        let c = LastFmClient::new().with_session("sk");
        let err = c.scrobble("", "T", "Al", 0).unwrap_err();
        assert!(err.to_string().contains("artista"));
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
    fn api_errors_are_reported_with_their_message() {
        let err = check_api_error(r#"{"error":14,"message":"Unauthorized Token"}"#).unwrap_err();
        assert_eq!(err.to_string(), "Unauthorized Token");
    }

    #[test]
    fn a_successful_body_is_not_treated_as_an_error() {
        assert!(check_api_error(r#"{"token":"tok"}"#).is_ok());
    }

    #[test]
    fn token_body_deserializes() {
        let b: TokenBody = serde_json::from_str(r#"{"token":"tok"}"#).unwrap();
        assert_eq!(b.token, "tok");
    }

    #[test]
    fn session_body_deserializes() {
        let b: SessionBody =
            serde_json::from_str(r#"{"session":{"name":"bob","key":"sk","subscriber":0}}"#)
                .unwrap();
        assert_eq!(b.session.key, "sk");
        assert_eq!(b.session.name, "bob");
    }
}
