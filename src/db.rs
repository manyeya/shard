//! SQLite: every download and the settings, so the list and the queue survive a restart.
//! Byte-level resume state lives next to each download in `<file>.resume` (see `engine`).

use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub struct Db(Connection);

/// A download as stored. `headers` is "Name: value" lines (cookies, referrer, user agent).
pub struct Saved {
    pub id: i64,
    pub url: String,
    pub headers: String,
    pub dir: PathBuf,
    pub name: Option<String>,
    pub total: u64,
    pub done: u64,
    pub status: String,
    pub resumable: Option<bool>,
    pub error: Option<String>,
    pub added: i64,
    /// Place in the queue: lower runs first.
    pub position: i64,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let c = Connection::open(path)?;
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS downloads (
                 id INTEGER PRIMARY KEY,
                 url TEXT NOT NULL,
                 headers TEXT NOT NULL DEFAULT '',
                 dir TEXT NOT NULL,
                 name TEXT,
                 total INTEGER NOT NULL DEFAULT 0,
                 done INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL,
                 resumable INTEGER,
                 error TEXT,
                 added INTEGER NOT NULL,
                 position INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        // Databases from before queue order: number them in the order they were added.
        if c.prepare("SELECT position FROM downloads LIMIT 0").is_err() {
            c.execute_batch("ALTER TABLE downloads ADD COLUMN position INTEGER NOT NULL DEFAULT 0; UPDATE downloads SET position = id;")?;
        }
        Ok(Db(c))
    }

    pub fn all(&self) -> rusqlite::Result<Vec<Saved>> {
        let mut q = self.0.prepare(
            "SELECT id, url, headers, dir, name, total, done, status, resumable, error, added, position FROM downloads ORDER BY position, id",
        )?;
        q.query_map([], |r| {
            Ok(Saved {
                id: r.get(0)?,
                url: r.get(1)?,
                headers: r.get(2)?,
                dir: PathBuf::from(r.get::<_, String>(3)?),
                name: r.get(4)?,
                total: r.get::<_, i64>(5)? as u64, // SQLite integers are signed
                done: r.get::<_, i64>(6)? as u64,
                status: r.get(7)?,
                resumable: r.get(8)?,
                error: r.get(9)?,
                added: r.get(10)?,
                position: r.get(11)?,
            })
        })?
        .collect()
    }

    /// Where a download sits in the queue.
    pub fn position(&self, id: i64) -> rusqlite::Result<i64> {
        self.0.query_row("SELECT position FROM downloads WHERE id = ?1", [id], |r| r.get(0))
    }

    /// Insert a new download, at the end of the queue; returns its id.
    pub fn insert(&self, url: &str, headers: &str, dir: &Path, status: &str, added: i64) -> rusqlite::Result<i64> {
        self.0.execute(
            "INSERT INTO downloads (url, headers, dir, status, added, position)
             VALUES (?1, ?2, ?3, ?4, ?5, (SELECT COALESCE(MAX(position), 0) + 1 FROM downloads))",
            params![url, headers, dir.to_string_lossy(), status, added],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    pub fn update(&self, s: &Saved) -> rusqlite::Result<()> {
        self.0.execute(
            "UPDATE downloads SET headers = ?2, name = ?3, total = ?4, done = ?5, status = ?6, resumable = ?7, error = ?8,
             url = ?9, dir = ?10, position = ?11 WHERE id = ?1",
            params![
                s.id,
                s.headers,
                s.name,
                s.total as i64,
                s.done as i64,
                s.status,
                s.resumable,
                s.error,
                s.url,
                s.dir.to_string_lossy(),
                s.position
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        self.0.execute("DELETE FROM downloads WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.0.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.0.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            [key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Db;

    #[test]
    fn an_old_database_gets_queue_positions() {
        let dir = crate::engine::tests::scratch("migrate");
        let path = dir.join("old.db");
        let old = rusqlite::Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE downloads (id INTEGER PRIMARY KEY, url TEXT NOT NULL, headers TEXT NOT NULL DEFAULT '', dir TEXT NOT NULL,
             name TEXT, total INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL, resumable INTEGER,
             error TEXT, added INTEGER NOT NULL);
             INSERT INTO downloads (url, dir, status, added) VALUES ('http://a/1', '/tmp', 'done', 1), ('http://a/2', '/tmp', 'queued', 2);",
        )
        .unwrap();
        drop(old);
        let db = Db::open(&path).unwrap();
        let positions: Vec<i64> = db.all().unwrap().iter().map(|s| s.position).collect();
        assert_eq!(positions, [1, 2]);
        let id = db.insert("http://a/3", "", std::path::Path::new("/tmp"), "queued", 3).unwrap();
        assert_eq!(db.position(id).unwrap(), 3, "new ones go to the end");
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
