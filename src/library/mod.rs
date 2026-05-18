pub mod art;
pub mod db;
pub mod metadata;
pub mod scanner;

use crate::i18n::gettext;
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
            let cover = None; // cargado de forma asíncrona por la UI
            Album {
                name,
                artist,
                tracks,
                cover,
            }
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
        .map(|(name, (album_count, track_count))| Artist {
            name,
            album_count,
            track_count,
        })
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
        self.title
            .clone()
            .unwrap_or_else(|| gettext("Unknown title"))
    }

    pub fn display_artist(&self) -> String {
        self.artist
            .clone()
            .unwrap_or_else(|| gettext("Unknown artist"))
    }

    pub fn display_album(&self) -> String {
        self.album
            .clone()
            .unwrap_or_else(|| "Álbum desconocido".to_string())
    }

    pub fn duration_str(&self) -> String {
        match self.duration_secs {
            Some(s) => fmt_duration(s.max(0) as u64),
            None => "--:--".to_string(),
        }
    }
}

pub fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: Option<&str>, album: Option<&str>, num: Option<i64>) -> Track {
        Track {
            id: None,
            path: format!("/m/{:?}-{:?}-{:?}.mp3", artist, album, num),
            title: Some("Title".into()),
            artist: artist.map(str::to_string),
            album: album.map(str::to_string),
            track_num: num,
            duration_secs: Some(200),
        }
    }

    #[test]
    fn fmt_duration_pads_seconds_and_rolls_minutes() {
        assert_eq!(fmt_duration(0), "0:00");
        assert_eq!(fmt_duration(5), "0:05");
        assert_eq!(fmt_duration(65), "1:05");
        assert_eq!(fmt_duration(3599), "59:59");
        assert_eq!(fmt_duration(3600), "60:00");
    }

    #[test]
    fn duration_str_handles_none_and_negative() {
        let mut t = track(Some("A"), Some("X"), Some(1));
        t.duration_secs = Some(125);
        assert_eq!(t.duration_str(), "2:05");
        t.duration_secs = None;
        assert_eq!(t.duration_str(), "--:--");
        // Negative durations are clamped to zero, not panicked on.
        t.duration_secs = Some(-10);
        assert_eq!(t.duration_str(), "0:00");
    }

    #[test]
    fn display_helpers_prefer_value_then_fall_back() {
        let with = track(Some("Radiohead"), Some("OK Computer"), Some(1));
        assert_eq!(with.display_artist(), "Radiohead");
        assert_eq!(with.display_album(), "OK Computer");
        assert_eq!(with.display_title(), "Title");

        // Missing fields produce a non-empty placeholder (text is i18n-dependent).
        let without = track(None, None, None);
        assert!(!without.display_artist().is_empty());
        assert!(!without.display_album().is_empty());
    }

    #[test]
    fn group_into_albums_groups_by_artist_and_album_and_sorts_tracks() {
        let tracks = vec![
            track(Some("A"), Some("Beta"), Some(2)),
            track(Some("A"), Some("Beta"), Some(1)),
            track(Some("A"), Some("Alpha"), Some(1)),
            track(Some("B"), Some("Alpha"), Some(1)),
        ];
        let albums = group_into_albums(&tracks);
        assert_eq!(albums.len(), 3);
        // Albums are sorted by name.
        assert_eq!(albums[0].name, "Alpha");
        assert_eq!(albums.last().unwrap().name, "Beta");

        let beta = albums.iter().find(|a| a.name == "Beta").unwrap();
        assert_eq!(beta.artist, "A");
        assert_eq!(beta.tracks.len(), 2);
        // Tracks inside an album are sorted by track number.
        assert_eq!(beta.tracks[0].track_num, Some(1));
        assert_eq!(beta.tracks[1].track_num, Some(2));
        assert!(beta.cover.is_none());
    }

    #[test]
    fn group_into_albums_separates_same_album_name_by_artist() {
        let tracks = vec![
            track(Some("A"), Some("Greatest Hits"), Some(1)),
            track(Some("B"), Some("Greatest Hits"), Some(1)),
        ];
        let albums = group_into_albums(&tracks);
        assert_eq!(
            albums.len(),
            2,
            "same title, different artist => two albums"
        );
    }

    #[test]
    fn group_into_artists_counts_albums_and_tracks() {
        let tracks = vec![
            track(Some("A"), Some("Alpha"), Some(1)),
            track(Some("A"), Some("Alpha"), Some(2)),
            track(Some("A"), Some("Beta"), Some(1)),
            track(Some("B"), Some("Gamma"), Some(1)),
        ];
        let artists = group_into_artists(&group_into_albums(&tracks));
        assert_eq!(artists.len(), 2);
        assert_eq!(artists[0].name, "A"); // sorted by name
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].track_count, 3);
        assert_eq!(artists[1].name, "B");
        assert_eq!(artists[1].album_count, 1);
        assert_eq!(artists[1].track_count, 1);
    }
}
