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
                let norm_p: std::path::PathBuf = std::path::Path::new(&normalize_path(p)).to_path_buf();
                norm_p.starts_with(&norm_folder)
                    && !existing_set.contains(&normalize_path(p))
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
