use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const API_KEY: &str = crate::credentials::API_KEY;
const API_SECRET: &str = crate::credentials::API_SECRET;

pub struct LastFmClient {
    session_key: Option<String>,
    client: Client,
}

#[derive(Deserialize)]
struct SessionResponse {
    session: Session,
}

#[derive(Deserialize)]
struct Session {
    key: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LastFmResult {
    Ok(SessionResponse),
    Err { #[allow(dead_code)] error: u32, message: String },
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

    fn sign(&self, params: &HashMap<&str, String>) -> String {
        let mut keys: Vec<&str> = params.keys().copied().collect();
        keys.sort();
        let mut base = String::new();
        for k in &keys {
            base.push_str(k);
            base.push_str(&params[k]);
        }
        base.push_str(API_SECRET);
        format!("{:x}", md5::compute(base.as_bytes()))
    }

    pub fn authenticate_with_password(&self, username: &str, password: &str) -> Result<String> {
        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "auth.getMobileSession".to_string());
        params.insert("api_key", API_KEY.to_string());
        params.insert("username", username.to_string());
        params.insert("password", password.to_string());
        let sig = self.sign(&params);
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        let resp: LastFmResult = self.client
            .post(API_URL)
            .form(&params)
            .send()?
            .json()?;

        match resp {
            LastFmResult::Ok(s) => Ok(s.session.key),
            LastFmResult::Err { message, .. } => anyhow::bail!("{}", message),
        }
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let sk = self.session_key.as_deref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión Last.fm"))?;

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "track.scrobble".to_string());
        params.insert("api_key", API_KEY.to_string());
        params.insert("sk", sk.to_string());
        params.insert("artist[0]", artist.to_string());
        params.insert("track[0]", track.to_string());
        params.insert("album[0]", album.to_string());
        params.insert("timestamp[0]", timestamp.to_string());

        let sig = self.sign(&params);
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        let resp = self.client.post(API_URL).form(&params).send()?;
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

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "track.updateNowPlaying".to_string());
        params.insert("api_key", API_KEY.to_string());
        params.insert("sk", sk);
        params.insert("artist", artist.to_string());
        params.insert("track", track.to_string());
        params.insert("album", album.to_string());

        let sig = self.sign(&params);
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        let _ = self.client.post(API_URL).form(&params).send();
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
