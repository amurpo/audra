use anyhow::Result;
use rusqlite::{Connection, params};
use crate::library::Track;

fn normalize_path(path: &str) -> String {
    let buf: std::path::PathBuf = std::path::Path::new(path).components().collect();
    let s = buf.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    return s.to_lowercase();
    #[cfg(not(target_os = "windows"))]
    return s;
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS tracks (
                id        INTEGER PRIMARY KEY,
                path      TEXT NOT NULL UNIQUE,
                title     TEXT,
                artist    TEXT,
                album     TEXT,
                track_num INTEGER,
                duration  INTEGER,
                added_at  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS playlists (
                id   INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE
            );
            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                position    INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, track_id)
            );
            CREATE TABLE IF NOT EXISTS scrobble_queue (
                id         INTEGER PRIMARY KEY,
                track_id   INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                played_at  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS album_covers (
                artist TEXT NOT NULL,
                album  TEXT NOT NULL,
                data   BLOB NOT NULL,
                PRIMARY KEY (artist, album)
            );
        ")?;
        Ok(())
    }

    pub fn upsert_track(&self, track: &Track) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tracks (path, title, artist, album, track_num, duration, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
               title=excluded.title, artist=excluded.artist,
               album=excluded.album, track_num=excluded.track_num,
               duration=excluded.duration",
            params![
                track.path, track.title, track.artist,
                track.album, track.track_num, track.duration_secs
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn all_tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, track_num, duration FROM tracks ORDER BY artist, album, track_num"
        )?;
        let tracks = stmt.query_map([], |row| {
            Ok(Track {
                id: Some(row.get(0)?),
                path: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
                album: row.get(4)?,
                track_num: row.get(5)?,
                duration_secs: row.get(6)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tracks)
    }

    pub fn queue_scrobble(&self, track_id: i64, played_at: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scrobble_queue (track_id, played_at) VALUES (?1, ?2)",
            params![track_id, played_at],
        )?;
        Ok(())
    }

    pub fn pending_scrobbles(&self) -> Result<Vec<(i64, Track, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT q.id, t.id, t.path, t.title, t.artist, t.album, t.track_num, t.duration, q.played_at
             FROM scrobble_queue q JOIN tracks t ON t.id = q.track_id"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Track {
                    id: Some(row.get(1)?),
                    path: row.get(2)?,
                    title: row.get(3)?,
                    artist: row.get(4)?,
                    album: row.get(5)?,
                    track_num: row.get(6)?,
                    duration_secs: row.get(7)?,
                },
                row.get::<_, String>(8)?,
            ))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn remove_scrobble(&self, queue_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM scrobble_queue WHERE id = ?1", params![queue_id])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM settings WHERE key = ?1",
            params![key],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn remove_track_by_path(&self, path: &str) -> Result<()> {
        self.conn.execute("DELETE FROM tracks WHERE path = ?1", params![path])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn remove_tracks_under_folder(&self, folder: &str) -> Result<usize> {
        let prefix = if folder.ends_with('/') {
            folder.to_string()
        } else {
            format!("{}/", folder)
        };
        let count = self.conn.execute(
            "DELETE FROM tracks WHERE path LIKE ?1 || '%'",
            params![prefix],
        )?;
        Ok(count)
    }

    pub fn remove_missing_from_folder(&self, folder: &str, existing_paths: &[String]) -> Result<usize> {
        let norm_folder: std::path::PathBuf = std::path::Path::new(&normalize_path(folder)).to_path_buf();
        let existing_set: std::collections::HashSet<String> =
            existing_paths.iter().map(|s| normalize_path(s)).collect();

        let mut stmt = self.conn.prepare("SELECT path FROM tracks")?;
        let to_delete: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter(|p| {
                let np = normalize_path(p);
                if existing_set.contains(&np) {
                    return false;
                }
                let norm_p: std::path::PathBuf = std::path::Path::new(&np).to_path_buf();
                // Purge a DB row when the file is missing under the current
                // library folder, OR when its file no longer exists anywhere
                // (orphans left by a previously-scanned folder). The app tracks
                // a single library folder, so out-of-folder rows whose files
                // are gone are stale ghosts. This only deletes DB rows; the
                // filesystem is never touched.
                norm_p.starts_with(&norm_folder)
                    || !std::path::Path::new(p).exists()
            })
            .collect();
        drop(stmt);

        for path in &to_delete {
            self.conn.execute("DELETE FROM tracks WHERE path = ?1", params![path])?;
        }
        Ok(to_delete.len())
    }

    pub fn get_cover(&self, artist: &str, album: &str) -> Option<Vec<u8>> {
        self.conn
            .query_row(
                "SELECT data FROM album_covers WHERE artist = ?1 AND album = ?2",
                params![artist, album],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_cover(&self, artist: &str, album: &str, data: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO album_covers (artist, album, data) VALUES (?1, ?2, ?3)",
            params![artist, album, data],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        // ":memory:" gives every test an isolated, disk-free SQLite instance.
        Database::open(":memory:").expect("open in-memory db")
    }

    fn track(path: &str, artist: &str, album: &str, num: i64) -> Track {
        Track {
            id: None,
            path: path.to_string(),
            title: Some(format!("{path} title")),
            artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            track_num: Some(num),
            duration_secs: Some(180),
        }
    }

    #[test]
    fn schema_is_created_on_open() {
        let db = db();
        // If init_schema didn't run these inserts would fail with "no such table".
        assert!(db.all_tracks().unwrap().is_empty());
        assert!(db.pending_scrobbles().unwrap().is_empty());
        assert_eq!(db.get_setting("missing"), None);
        assert_eq!(db.get_cover("a", "b"), None);
    }

    #[test]
    fn upsert_track_inserts_and_returns_rowid() {
        let db = db();
        let id = db.upsert_track(&track("/m/a.mp3", "A", "X", 1)).unwrap();
        assert!(id > 0);
        let all = db.all_tracks().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].path, "/m/a.mp3");
        assert_eq!(all[0].artist.as_deref(), Some("A"));
        assert_eq!(all[0].id, Some(id));
    }

    #[test]
    fn upsert_track_on_conflicting_path_updates_in_place() {
        let db = db();
        db.upsert_track(&track("/m/a.mp3", "Old", "Old", 1)).unwrap();
        let mut updated = track("/m/a.mp3", "New", "New", 2);
        updated.title = Some("New title".into());
        db.upsert_track(&updated).unwrap();

        let all = db.all_tracks().unwrap();
        assert_eq!(all.len(), 1, "same path must not duplicate");
        assert_eq!(all[0].artist.as_deref(), Some("New"));
        assert_eq!(all[0].title.as_deref(), Some("New title"));
        assert_eq!(all[0].track_num, Some(2));
    }

    #[test]
    fn all_tracks_is_ordered_by_artist_album_track_num() {
        let db = db();
        db.upsert_track(&track("/m/3.mp3", "B", "Z", 1)).unwrap();
        db.upsert_track(&track("/m/2.mp3", "A", "Y", 2)).unwrap();
        db.upsert_track(&track("/m/1.mp3", "A", "Y", 1)).unwrap();

        let paths: Vec<_> = db
            .all_tracks()
            .unwrap()
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert_eq!(paths, vec!["/m/1.mp3", "/m/2.mp3", "/m/3.mp3"]);
    }

    #[test]
    fn scrobble_queue_roundtrip() {
        let db = db();
        let tid = db.upsert_track(&track("/m/a.mp3", "A", "X", 1)).unwrap();
        db.queue_scrobble(tid, "1700000000").unwrap();
        db.queue_scrobble(tid, "1700000100").unwrap();

        let pending = db.pending_scrobbles().unwrap();
        assert_eq!(pending.len(), 2);
        let (queue_id, joined_track, played_at) = &pending[0];
        assert_eq!(joined_track.path, "/m/a.mp3");
        assert!(["1700000000", "1700000100"].contains(&played_at.as_str()));

        db.remove_scrobble(*queue_id).unwrap();
        assert_eq!(db.pending_scrobbles().unwrap().len(), 1);
    }

    #[test]
    fn settings_get_set_delete() {
        let db = db();
        assert_eq!(db.get_setting("language"), None);
        db.set_setting("language", "es").unwrap();
        assert_eq!(db.get_setting("language").as_deref(), Some("es"));
        // Upsert overwrites.
        db.set_setting("language", "en").unwrap();
        assert_eq!(db.get_setting("language").as_deref(), Some("en"));
        db.delete_setting("language").unwrap();
        assert_eq!(db.get_setting("language"), None);
    }

    #[test]
    fn remove_track_by_path_deletes_single_row() {
        let db = db();
        db.upsert_track(&track("/m/a.mp3", "A", "X", 1)).unwrap();
        db.upsert_track(&track("/m/b.mp3", "A", "X", 2)).unwrap();
        db.remove_track_by_path("/m/a.mp3").unwrap();
        let all = db.all_tracks().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].path, "/m/b.mp3");
    }

    #[test]
    fn remove_tracks_under_folder_counts_and_deletes_prefix() {
        let db = db();
        db.upsert_track(&track("/music/rock/a.mp3", "A", "X", 1))
            .unwrap();
        db.upsert_track(&track("/music/rock/b.mp3", "A", "X", 2))
            .unwrap();
        db.upsert_track(&track("/other/c.mp3", "C", "Y", 1)).unwrap();

        let removed = db.remove_tracks_under_folder("/music/rock").unwrap();
        assert_eq!(removed, 2);
        let all = db.all_tracks().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].path, "/other/c.mp3");
    }

    #[test]
    fn remove_missing_from_folder_purges_in_folder_and_nonexistent_ghosts() {
        let db = db();

        // A real file outside the library folder: still on disk, so a
        // single-folder rescan must NOT purge it (conservative, no data loss).
        let outside_real = std::env::temp_dir()
            .join(format!("audra_db_outside_{}.mp3", std::process::id()));
        std::fs::write(&outside_real, b"x").unwrap();
        let outside_real = outside_real.to_string_lossy().to_string();

        db.upsert_track(&track("/music/a/1.mp3", "A", "X", 1))
            .unwrap();
        db.upsert_track(&track("/music/b/2.mp3", "B", "Y", 1))
            .unwrap();
        db.upsert_track(&track(&outside_real, "C", "Z", 1)).unwrap();
        db.upsert_track(&track("/other/ghost.mp3", "D", "W", 1))
            .unwrap();

        // Scan of /music finds only a/1.mp3.
        let removed = db
            .remove_missing_from_folder("/music", &["/music/a/1.mp3".to_string()])
            .unwrap();
        // Purged: /music/b/2.mp3 (missing under folder) and
        // /other/ghost.mp3 (orphan whose file does not exist). Kept:
        // a/1.mp3 (found) and the out-of-folder file that still exists.
        assert_eq!(removed, 2);

        let mut paths: Vec<_> = db
            .all_tracks()
            .unwrap()
            .into_iter()
            .map(|t| t.path)
            .collect();
        paths.sort();
        let mut expected = vec!["/music/a/1.mp3".to_string(), outside_real.clone()];
        expected.sort();
        assert_eq!(paths, expected);

        let _ = std::fs::remove_file(&outside_real);
    }

    #[test]
    fn cover_blob_roundtrip_and_replace() {
        let db = db();
        assert_eq!(db.get_cover("Artist", "Album"), None);
        db.set_cover("Artist", "Album", &[1, 2, 3]).unwrap();
        assert_eq!(db.get_cover("Artist", "Album"), Some(vec![1, 2, 3]));
        // INSERT OR REPLACE on the (artist, album) primary key.
        db.set_cover("Artist", "Album", &[9, 9]).unwrap();
        assert_eq!(db.get_cover("Artist", "Album"), Some(vec![9, 9]));
    }
}
