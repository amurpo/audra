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

fn download(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    let bytes = client.get(url).send().ok()?.bytes().ok()?;
    if bytes.is_empty() {
        None
    } else {
        Some(bytes.to_vec())
    }
}

/// Busca carátula de álbum: MusicBrainz → TheAudioDB.
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

    let data = musicbrainz_album_cover(&client, artist, album)
        .or_else(|| audiodb_album_cover(&client, artist, album))?;

    write_cache(&path, &data);
    Some(data)
}

fn musicbrainz_album_cover(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
) -> Option<Vec<u8>> {
    // Escape Lucene special chars so quotes in titles don't break the query.
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let query = format!("release:\"{}\" AND artist:\"{}\"", esc(album), esc(artist));

    let resp: serde_json::Value = client
        .get("https://musicbrainz.org/ws/2/release")
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "5")])
        .send()
        .ok()?
        .json()
        .ok()?;

    for release in resp["releases"].as_array()?.iter().take(3) {
        let mbid = release["id"].as_str()?;
        let url = format!("https://coverartarchive.org/release/{}/front-500", mbid);
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes() {
                    if !bytes.is_empty() {
                        log::debug!("metadata: carátula MusicBrainz '{}' - '{}'", artist, album);
                        return Some(bytes.to_vec());
                    }
                }
            }
        }
        // Respetar el rate limit de MusicBrainz (1 req/s)
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    None
}

fn audiodb_album_cover(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
) -> Option<Vec<u8>> {
    let resp: serde_json::Value = client
        .get("https://www.theaudiodb.com/api/v1/json/2/searchalbum.php")
        .query(&[("s", artist), ("a", album)])
        .send()
        .ok()?
        .json()
        .ok()?;

    let albums = resp["album"]
        .as_array()
        .filter(|a| !a.is_empty())
        .or_else(|| {
            log::debug!(
                "metadata: TheAudioDB no encontró álbum '{}' - '{}'",
                artist,
                album
            );
            None
        })?;
    let url = albums[0]["strAlbumThumb"]
        .as_str()
        .or_else(|| {
            log::debug!(
                "metadata: TheAudioDB sin imagen para '{}' - '{}'",
                artist,
                album
            );
            None
        })?
        .to_string();
    let data = download(client, &url)?;
    log::debug!("metadata: carátula TheAudioDB '{}' - '{}'", artist, album);
    Some(data)
}

/// Busca foto del artista: Deezer → TheAudioDB → iTunes (portada de álbum).
pub fn fetch_artist_photo(artist: &str) -> Option<Vec<u8>> {
    let path = artist_cache_path(artist);
    if path.exists() {
        return std::fs::read(&path).ok();
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("audra/0.1 (https://github.com/audra-player; daigo.tnt@gmail.com)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let img_url = deezer_artist_photo(&client, artist)
        .or_else(|| audiodb_artist_photo(&client, artist))
        .or_else(|| itunes_album_art(&client, artist))?;

    let data = download(&client, &img_url)?;
    write_cache(&path, &data);
    log::debug!("metadata: foto descargada para artista '{}'", artist);
    Some(data)
}

fn deezer_artist_photo(client: &reqwest::blocking::Client, artist: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get("https://api.deezer.com/search/artist")
        .query(&[("q", artist), ("limit", "1")])
        .send()
        .ok()?
        .json()
        .ok()?;

    let data = resp["data"]
        .as_array()
        .filter(|a| !a.is_empty())
        .or_else(|| {
            log::debug!("metadata: Deezer no encontró artista '{}'", artist);
            None
        })?;
    let url = data[0]["picture_xl"]
        .as_str()
        .or_else(|| data[0]["picture_big"].as_str())
        .or_else(|| {
            log::debug!("metadata: Deezer sin foto para '{}'", artist);
            None
        })?
        .to_string();

    // Deezer retorna "artist//" (hash vacío) cuando no tiene foto del artista
    if url.contains("artist//") {
        return None;
    }
    Some(url)
}

fn audiodb_artist_photo(client: &reqwest::blocking::Client, artist: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get("https://www.theaudiodb.com/api/v1/json/2/search.php")
        .query(&[("s", artist)])
        .send()
        .ok()?
        .json()
        .ok()?;

    resp["artists"]
        .as_array()
        .filter(|a| !a.is_empty())
        .and_then(|a| a[0]["strArtistThumb"].as_str())
        .map(|s| s.to_string())
}

// Último recurso: portada del álbum más reciente vía iTunes (música-específico, sin clave)
fn itunes_album_art(client: &reqwest::blocking::Client, artist: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", artist),
            ("media", "music"),
            ("entity", "musicAlbum"),
            ("limit", "1"),
        ])
        .send()
        .ok()?
        .json()
        .ok()?;

    let url = resp["results"]
        .as_array()
        .filter(|a| !a.is_empty())
        .and_then(|a| a[0]["artworkUrl100"].as_str())
        .or_else(|| {
            log::debug!("metadata: iTunes sin resultado para '{}'", artist);
            None
        })?;
    Some(url.replace("100x100bb", "600x600bb"))
}
