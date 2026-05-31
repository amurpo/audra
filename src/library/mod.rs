pub mod art;
pub mod db;
pub mod dedup;
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

/// Canonical album grouping. `music_folder` enables the folder-aware
/// deduplication pipeline; `None` falls back to pure tag grouping.
pub fn group_into_albums(tracks: &[Track], music_folder: Option<&str>) -> Vec<Album> {
    dedup::group_albums(tracks, music_folder)
}

/// Build the Artists view from the canonical album list. Names come from the
/// *track-level* performer (`track.display_artist()`, which prefers
/// `track.artist` and never falls back to `album_artist`), so compilations
/// labelled "Various Artists" still surface every participating artist as its
/// own entry — and we preserve the fix from commit 95f22ca that keeps the
/// performer ("INFIX") visible even when `album_artist` carries the composer
/// ("千住明").
pub fn group_into_artists(albums: &[Album]) -> Vec<Artist> {
    use std::collections::HashSet;
    // key: lowercase artist name for case-insensitive dedup
    // value: (album_keys, total_tracks, name_freq) — name_freq picks the
    // display name: the variant that appears on the most tracks wins.
    type ArtistAgg = (HashSet<String>, usize, HashMap<String, usize>);
    let mut map: HashMap<String, ArtistAgg> = HashMap::new();
    for album in albums {
        let album_key = format!("{}|{}", album.artist, album.name);
        // Count per-artist track contributions to this album in one pass so
        // the same artist appearing twice still contributes correctly.
        let mut per_artist: HashMap<String, usize> = HashMap::new();
        for t in &album.tracks {
            *per_artist.entry(t.display_artist()).or_insert(0) += 1;
        }
        for (artist, count) in per_artist {
            let key = artist.to_lowercase();
            let entry = map
                .entry(key)
                .or_insert_with(|| (HashSet::new(), 0, HashMap::new()));
            entry.0.insert(album_key.clone());
            entry.1 += count;
            *entry.2.entry(artist).or_insert(0) += count;
        }
    }
    let mut artists: Vec<Artist> = map
        .into_iter()
        .map(|(_, (album_keys, track_count, name_freq))| {
            // Tie break on the smallest name (lexicographic) so the chosen
            // display name is deterministic across runs. A bare `max_by_key`
            // over a HashMap returns an arbitrary tied variant each launch,
            // which flipped the name and orphaned the artist's cached photo
            // (keyed on the exact name).
            let name = name_freq
                .into_iter()
                .max_by(|(n1, c1), (n2, c2)| c1.cmp(c2).then_with(|| n2.cmp(n1)))
                .map(|(n, _)| n)
                .unwrap_or_default();
            Artist {
                name,
                album_count: album_keys.len(),
                track_count,
            }
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
    pub disc_num: Option<i64>,
    pub album_artist: Option<String>,
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
            .unwrap_or_else(|| gettext("Unknown album"))
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
            disc_num: None,
            album_artist: None,
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
        let albums = group_into_albums(&tracks, None);
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
        let albums = group_into_albums(&tracks, None);
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
        let artists = group_into_artists(&group_into_albums(&tracks, None));
        assert_eq!(artists.len(), 2);
        assert_eq!(artists[0].name, "A"); // sorted by name
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].track_count, 3);
        assert_eq!(artists[1].name, "B");
        assert_eq!(artists[1].album_count, 1);
        assert_eq!(artists[1].track_count, 1);
    }

    #[test]
    fn group_into_artists_surfaces_individual_performers_on_compilations() {
        // Rurouni Kenshin OST shape: one folder, many performers. The
        // Artists view must list every performer (Curio, Yellow Monkey…),
        // not just a single "Various Artists" lump that hides them.
        let mf = Some("/Music");
        let tracks = vec![
            Track {
                id: None,
                path: "/Music/Rurouni Kenshin OST/01.mp3".into(),
                title: Some("Sobakasu".into()),
                artist: Some("Judy and Mary".into()),
                album: Some("Rurouni Kenshin OST".into()),
                track_num: Some(1),
                duration_secs: Some(200),
                disc_num: None,
                album_artist: None,
            },
            Track {
                id: None,
                path: "/Music/Rurouni Kenshin OST/02.mp3".into(),
                title: Some("Kimi ni Furerudake de".into()),
                artist: Some("Curio".into()),
                album: Some("Rurouni Kenshin OST".into()),
                track_num: Some(2),
                duration_secs: Some(200),
                disc_num: None,
                album_artist: None,
            },
            Track {
                id: None,
                path: "/Music/Rurouni Kenshin OST/03.mp3".into(),
                title: Some("HEART OF SWORD".into()),
                artist: Some("T.M. Revolution".into()),
                album: Some("Rurouni Kenshin OST".into()),
                track_num: Some(3),
                duration_secs: Some(200),
                disc_num: None,
                album_artist: None,
            },
        ];
        let albums = group_into_albums(&tracks, mf);
        let artists = group_into_artists(&albums);
        let names: std::collections::HashSet<_> = artists.iter().map(|a| a.name.as_str()).collect();
        assert!(
            names.contains("Curio"),
            "Curio must appear (was hidden before)"
        );
        assert!(names.contains("Judy and Mary"));
        assert!(names.contains("T.M. Revolution"));
        // The compilation umbrella label must NOT itself show up as an
        // artist alongside the real performers.
        assert!(
            !names.contains("Various Artists"),
            "compilation label should not pollute the Artists list"
        );
    }

    #[test]
    fn group_into_artists_uses_track_artist_not_album_artist() {
        // Regression guard for commit 95f22ca: when album_artist holds the
        // composer (e.g. 千住明) and artist holds the actual performer
        // (INFIX), the Artists view must list INFIX — never 千住明.
        let mf = Some("/Music");
        let mut track = Track {
            id: None,
            path: "/Music/INFIX/V Gundam Score I/1.mp3".into(),
            title: Some("Track".into()),
            artist: Some("INFIX".into()),
            album: Some("V Gundam Score I".into()),
            track_num: Some(1),
            duration_secs: Some(200),
            disc_num: None,
            album_artist: None,
        };
        track.album_artist = Some("千住明".into());
        let albums = group_into_albums(&[track], mf);
        let artists = group_into_artists(&albums);
        let names: std::collections::HashSet<_> = artists.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains("INFIX"));
        assert!(!names.contains("千住明"));
    }
}
