//! SQLite access for Besedka (rusqlite, bundled) + refinery migrations.
//!
//! Wave 0 shape: one connection behind a `Mutex`. Single-process server, so a
//! writer-worker pool (like trackly) is overkill until Wave 2+ has real
//! concurrency pressure.

use std::path::Path;
use std::sync::Mutex;

use besedka_core::Room;
use refinery::embed_migrations;
use rusqlite::Connection;
use thiserror::Error;

// Embed workspace-root migrations (`migrations/`) at compile time.
// Path is relative to this crate's `Cargo.toml` (trackly uses the same trick).
embed_migrations!("../../migrations");

#[derive(Debug, Error)]
pub enum DbError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migrations failed: {0}")]
    Migrations(String),
    #[error("bad room kind in db: {0}")]
    BadKind(String),
}

/// A SQLite database (connection + schema).
#[derive(Debug)]
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (or create) the database file and run pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&mut conn)?;
        tracing::info!(db = %path.display(), "sqlite ready");
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// In-memory database with the same schema (for tests).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let mut conn = Connection::open_in_memory()?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn migrate(conn: &mut Connection) -> Result<(), DbError> {
        migrations::runner()
            .run(conn)
            .map(|_report| ())
            .map_err(|e| DbError::Migrations(e.to_string()))
    }

    /// All rooms, ordered by `sort_order` then `id`.
    pub fn list_rooms(&self) -> Result<Vec<Room>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, kind, title, created_at FROM rooms ORDER BY sort_order, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut rooms = Vec::new();
        for row in rows {
            let (id, kind, title, created_at) = row?;
            rooms.push(Room {
                id,
                kind: kind.parse().map_err(|k: String| DbError::BadKind(k))?,
                title,
                created_at,
            });
        }
        Ok(rooms)
    }

    /// Create a room; returns the new `id`.
    pub fn create_room(&self, kind: besedka_core::RoomKind, title: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO rooms (kind, title) VALUES (?1, ?2)",
            (kind.as_str(), title),
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Number of users (sanity check / future admin).
    pub fn user_count(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        Ok(conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use besedka_core::RoomKind;

    #[test]
    fn migrations_create_expected_tables() {
        let db = Db::open_in_memory().expect("open in-memory db");
        let conn = db.conn.lock().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(tables.contains(&"users".to_string()), "tables: {tables:?}");
        assert!(tables.contains(&"rooms".to_string()), "tables: {tables:?}");
    }

    #[test]
    fn rooms_start_empty() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.list_rooms().unwrap().is_empty());
        assert_eq!(db.user_count().unwrap(), 0);
    }

    #[test]
    fn create_and_list_room() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_room(RoomKind::General, "Новости").unwrap();
        assert!(id > 0);
        let rooms = db.list_rooms().unwrap();
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].id, id);
        assert_eq!(rooms[0].kind, RoomKind::General);
        assert_eq!(rooms[0].title, "Новости");
    }

    #[test]
    fn file_db_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/dir/besedka.db");
        {
            let db = Db::open(&path).expect("open file db");
            db.create_room(RoomKind::Room, "Работа").unwrap();
        }
        let db = Db::open(&path).expect("reopen file db");
        assert_eq!(db.list_rooms().unwrap().len(), 1);
    }
}
