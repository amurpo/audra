use anyhow::Result;
use rusqlite::{Connection, params};
use crate::library::Track;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
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
}
