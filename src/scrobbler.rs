use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

pub struct LastFmClient {
    api_key: String,
    api_secret: String,
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

impl LastFmClient {
    pub fn new(api_key: &str, api_secret: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            session_key: None,
            client: Client::new(),
        }
    }

    pub fn with_session(mut self, session_key: &str) -> Self {
        self.session_key = Some(session_key.to_string());
        self
    }

    fn sign(&self, params: HashMap<&str, String>) -> String {
        let mut keys: Vec<&str> = params.keys().copied().collect();
        keys.sort();
        let mut base = String::new();
        for k in &keys {
            base.push_str(k);
            base.push_str(&params[k]);
        }
        base.push_str(&self.api_secret);
        format!("{:x}", md5::compute(base.as_bytes()))
    }

    pub fn authenticate(&self, token: &str) -> Result<String> {
        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "auth.getSession".to_string());
        params.insert("api_key", self.api_key.clone());
        params.insert("token", token.to_string());
        let sig = self.sign(params.clone());
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        let resp: SessionResponse = self.client
            .get(API_URL)
            .query(&params)
            .send()?
            .json()?;

        Ok(resp.session.key)
    }

    pub fn scrobble(&self, artist: &str, track: &str, album: &str, timestamp: i64) -> Result<()> {
        let sk = self.session_key.as_deref().ok_or(anyhow::anyhow!("Sin sesión Last.fm"))?;

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "track.scrobble".to_string());
        params.insert("api_key", self.api_key.clone());
        params.insert("sk", sk.to_string());
        params.insert("artist[0]", artist.to_string());
        params.insert("track[0]", track.to_string());
        params.insert("album[0]", album.to_string());
        params.insert("timestamp[0]", timestamp.to_string());

        let sig = self.sign(params.clone());
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        let resp = self.client.post(API_URL).form(&params).send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Last.fm scrobble error: {}", resp.status());
        }
        Ok(())
    }

    pub fn update_now_playing(&self, artist: &str, track: &str, album: &str) -> Result<()> {
        let sk = self.session_key.as_deref().ok_or(anyhow::anyhow!("Sin sesión Last.fm"))?;

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("method", "track.updateNowPlaying".to_string());
        params.insert("api_key", self.api_key.clone());
        params.insert("sk", sk.to_string());
        params.insert("artist", artist.to_string());
        params.insert("track", track.to_string());
        params.insert("album", album.to_string());

        let sig = self.sign(params.clone());
        params.insert("api_sig", sig);
        params.insert("format", "json".to_string());

        self.client.post(API_URL).form(&params).send()?;
        Ok(())
    }
}
