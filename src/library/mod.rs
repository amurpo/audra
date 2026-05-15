pub mod art;
pub mod db;
pub mod metadata;
pub mod scanner;

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Album {
    pub name: String,
    pub artist: String,
    pub tracks: Vec<Track>,
    pub cover: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Artist {
    pub name: String,
    pub album_count: usize,
    pub track_count: usize,
}

pub fn group_into_albums(tracks: &[Track]) -> Vec<Album> {
    let mut map: HashMap<(String, String), Vec<Track>> = HashMap::new();
    for track in tracks {
        let key = (track.display_artist(), track.display_album());
        map.entry(key).or_default().push(track.clone());
    }
    let mut albums: Vec<Album> = map
        .into_iter()
        .map(|((artist, name), mut tracks)| {
            tracks.sort_by_key(|t| t.track_num.unwrap_or(999));
            let cover = tracks.first().and_then(|t| art::read_cover_art(&t.path));
            Album { name, artist, tracks, cover }
        })
        .collect();
    albums.sort_by(|a, b| a.name.cmp(&b.name));
    albums
}

pub fn group_into_artists(albums: &[Album]) -> Vec<Artist> {
    let mut map: HashMap<String, (usize, usize)> = HashMap::new();
    for album in albums {
        let e = map.entry(album.artist.clone()).or_default();
        e.0 += 1;
        e.1 += album.tracks.len();
    }
    let mut artists: Vec<Artist> = map
        .into_iter()
        .map(|(name, (album_count, track_count))| Artist { name, album_count, track_count })
        .collect();
    artists.sort_by(|a, b| a.name.cmp(&b.name));
    artists
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: Option<i64>,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_num: Option<i64>,
    pub duration_secs: Option<i64>,
}

impl Track {
    pub fn display_title(&self) -> String {
        self.title.clone().unwrap_or_else(|| "Sin título".to_string())
    }

    pub fn display_artist(&self) -> String {
        self.artist.clone().unwrap_or_else(|| "Artista desconocido".to_string())
    }

    pub fn display_album(&self) -> String {
        self.album.clone().unwrap_or_else(|| "Álbum desconocido".to_string())
    }

    pub fn duration_str(&self) -> String {
        match self.duration_secs {
            Some(s) => format!("{}:{:02}", s / 60, s % 60),
            None => "--:--".to_string(),
        }
    }
}
