use anyhow::Result;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::time::Duration;

const PROXY_URL: &str = crate::credentials::PROXY_URL;
const APP_TOKEN: &str = crate::credentials::APP_TOKEN;

pub struct LastFmClient {
    session_key: Option<String>,
    client: Client,
}

impl LastFmClient {
    pub fn new() -> Self {
        Self {
            session_key: None,
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub fn with_session(mut self, session_key: &str) -> Self {
        self.session_key = Some(session_key.to_string());
        self
    }

    pub fn is_configured() -> bool {
        !PROXY_URL.is_empty() && !APP_TOKEN.is_empty()
    }

    // Llama al proxy con reintentos. 4xx no reintenta (error de datos/auth).
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({ "method": method, "params": params });
        let mut last_err = anyhow::anyhow!("sin respuesta del proxy");

        for attempt in 0..3u32 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs((attempt * 2) as u64));
            }

            match self.client
                .post(PROXY_URL)
                .header("Authorization", format!("Bearer {}", APP_TOKEN))
                .json(&body)
                .send()
            {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json().map_err(Into::into);
                }
                Ok(resp) => {
                    let status = resp.status();
                    last_err = anyhow::anyhow!("proxy HTTP {}", status);
                    if status.is_client_error() {
                        break; // 4xx: no tiene sentido reintentar
                    }
                }
                Err(e) => {
                    last_err = e.into();
                }
            }
        }
        Err(last_err)
    }

    pub fn authenticate_with_password(&self, username: &str, password: &str) -> Result<String> {
        let resp = self.call("auth.getMobileSession", json!({
            "username": username,
            "password": password,
        }))?;

        resp["session"]["key"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                let msg = resp["message"].as_str().unwrap_or("error desconocido");
                anyhow::anyhow!("{}", msg)
            })
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let sk = self.session_key.as_deref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión Last.fm"))?;

        self.call("track.scrobble", json!({
            "sk": sk,
            "artist[0]": artist,
            "track[0]": track,
            "album[0]": album,
            "timestamp[0]": timestamp,
        }))?;

        Ok(())
    }

    // best-effort: no reintenta ni encola, es informativo
    pub fn update_now_playing(&self, artist: &str, track: &str, album: &str) {
        let sk = match self.session_key.as_deref() {
            Some(s) => s.to_string(),
            None => return,
        };
        let body = json!({
            "method": "track.updateNowPlaying",
            "params": { "sk": sk, "artist": artist, "track": track, "album": album }
        });
        let _ = self.client
            .post(PROXY_URL)
            .header("Authorization", format!("Bearer {}", APP_TOKEN))
            .json(&body)
            .send();
    }

    // Intenta enviar scrobbles pendientes en la DB. Se detiene al primer fallo
    // (si uno falla por red, los siguientes también fallarán).
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
