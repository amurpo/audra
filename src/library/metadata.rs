use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("audra")
        .join("covers")
}

fn album_cache_path(artist: &str, album: &str) -> PathBuf {
    let key = format!("{}|{}", artist.to_lowercase(), album.to_lowercase());
    let hash = format!("{:x}", md5::compute(key.as_bytes()));
    cache_dir().join(format!("album_{}.jpg", hash))
}

fn artist_cache_path(artist: &str) -> PathBuf {
    let hash = format!("{:x}", md5::compute(artist.to_lowercase().as_bytes()));
    cache_dir().join(format!("artist_{}.jpg", hash))
}

fn write_cache(path: &PathBuf, data: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, data);
}

/// Busca carátula de álbum en MusicBrainz + Cover Art Archive.
/// Primero verifica el caché en disco; si no existe, hace la petición y lo guarda.
pub fn fetch_album_cover(artist: &str, album: &str) -> Option<Vec<u8>> {
    let path = album_cache_path(artist, album);
    if path.exists() {
        return std::fs::read(&path).ok();
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("audra/0.1 (https://github.com/audra-player; daigo.tnt@gmail.com)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    let query = format!("release:\"{}\" AND artist:\"{}\"", album, artist);

    let resp: serde_json::Value = client
        .get("https://musicbrainz.org/ws/2/release")
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "5")])
        .send()
        .ok()?
        .json()
        .ok()?;

    let releases = resp["releases"].as_array()?;

    for release in releases.iter().take(3) {
        let mbid = release["id"].as_str()?;
        let art_url = format!("https://coverartarchive.org/release/{}/front-500", mbid);

        if let Ok(art_resp) = client.get(&art_url).send() {
            if art_resp.status().is_success() {
                if let Ok(bytes) = art_resp.bytes() {
                    if !bytes.is_empty() {
                        let data = bytes.to_vec();
                        write_cache(&path, &data);
                        log::debug!("metadata: carátula descargada para '{}' - '{}'", artist, album);
                        return Some(data);
                    }
                }
            }
        }
        // Respetar el rate limit de MusicBrainz (1 req/s)
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }

    log::debug!("metadata: sin carátula en MusicBrainz para '{}' - '{}'", artist, album);
    None
}

/// Busca imagen de artista en Last.fm.
/// Lee la API key desde la variable de entorno LASTFM_API_KEY.
pub fn fetch_artist_image(artist: &str, lastfm_key: &str) -> Option<Vec<u8>> {
    let path = artist_cache_path(artist);
    if path.exists() {
        return std::fs::read(&path).ok();
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("audra/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp: serde_json::Value = client
        .get("http://ws.audioscrobbler.com/2.0/")
        .query(&[
            ("method", "artist.getinfo"),
            ("artist", artist),
            ("api_key", lastfm_key),
            ("format", "json"),
        ])
        .send()
        .ok()?
        .json()
        .ok()?;

    let images = resp["artist"]["image"].as_array()?;

    // Last.fm usa este hash para su imagen de placeholder — lo ignoramos
    const PLACEHOLDER_HASH: &str = "2a96cbd8b46e442fc41c2b86b821562f";

    let image_url = images.iter().rev().find_map(|img| {
        let url = img["#text"].as_str()?;
        if !url.is_empty() && !url.contains(PLACEHOLDER_HASH) {
            Some(url.to_string())
        } else {
            None
        }
    })?;

    let bytes = client.get(&image_url).send().ok()?.bytes().ok()?;
    if bytes.is_empty() {
        return None;
    }

    let data = bytes.to_vec();
    write_cache(&path, &data);
    log::debug!("metadata: imagen de artista descargada para '{}'", artist);
    Some(data)
}
