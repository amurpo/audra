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

    pub fn get_auth_token() -> Result<AuthTokenResponse> {
        let proxy = PROXY_URL.trim_end_matches('/');
        let resp = Client::new().get(format!("{proxy}/auth/token")).send()?;
        if !resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error")?.as_str().map(str::to_string))
                .unwrap_or(text);
            anyhow::bail!("{}", msg);
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
            let text = resp.text().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error")?.as_str().map(str::to_string))
                .unwrap_or(text);
            anyhow::bail!("{}", msg);
        }
        Ok(resp.json()?)
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let sk = self.session_key.as_deref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión Last.fm"))?;

        let proxy = PROXY_URL.trim_end_matches('/');
        let body = json!({
            "sk": sk,
            "artist": artist,
            "track": track,
            "album": album,
            "timestamp": timestamp,
        });
        let resp = self.client.post(format!("{proxy}/scrobble")).json(&body).send()?;
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
        let _ = self.client.post(format!("{proxy}/nowplaying")).json(&body).send();
    }

    pub fn flush_queue(&self, db: &crate::library::db::Database) {
        let pending = match db.pending_scrobbles() {
            Ok(p) if !p.is_empty() => p,
            _ => return,
        };

        log::info!("scrobbler: {} scrobble(s) pendiente(s), enviando…", pending.len());

        for (queue_id, track, played_at) in pending {
            let artist = track.artist.clone().unwrap_or_default();
            let title  = track.title.clone().unwrap_or_default();
            let album  = track.album.clone().unwrap_or_default();
            let ts: i64 = played_at.parse().unwrap_or(0);

            match self.scrobble(&artist, &title, &album, ts) {
                Ok(()) => {
                    let _ = db.remove_scrobble(queue_id);
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
