//! SQLite access for Besedka (rusqlite, bundled) + refinery migrations.
//!
//! Wave 0 shape: one connection behind a `Mutex`. Single-process server, so a
//! writer-worker pool (like trackly) is overkill until Wave 2+ has real
//! concurrency pressure.

use std::path::Path;
use std::sync::Mutex;

use besedka_core::{Message, Role, Room, RoomKind, User};
use refinery::embed_migrations;
use rusqlite::{params, Connection};
use serde::Serialize;
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
    #[error("bad role in db: {0}")]
    BadRole(String),
    /// The requested row (room, user, membership) does not exist. Mapped to
    /// HTTP 404 by the app layer.
    #[error("not found")]
    NotFound,
    /// Access denied (e.g. posting/reading in a room the user is not a
    /// member of). Mapped to HTTP 403 by the app layer.
    #[error("forbidden")]
    Forbidden,
}

/// A SQLite database (connection + schema).
#[derive(Debug)]
pub struct Db {
    conn: Mutex<Connection>,
}

/// A user row exactly as stored — **includes the password hash**.
///
/// Use only inside the auth flow. To return a user to the API, map to the
/// public [`besedka_core::User`] (via `get_user`) so the hash is never
/// serialized.
#[derive(Debug, Clone)]
pub struct StoredUser {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: Role,
    pub created_at: String,
    /// Argon2 PHC string; `None` for AD/SSO accounts without a local password.
    pub password_hash: Option<String>,
}

/// A room row enriched with membership state for ONE viewer (Wave 1.3):
/// `joined` is whether that user is a member, `member_count` is the total
/// number of members of the room.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomView {
    pub id: i64,
    pub kind: RoomKind,
    pub title: String,
    pub created_at: String,
    pub joined: bool,
    pub member_count: i64,
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory database with the same schema (for tests).
    pub fn open_in_memory() -> Result<Self, DbError> {
        let mut conn = Connection::open_in_memory()?;
        Self::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
        let mut stmt =
            conn.prepare("SELECT id, kind, title, created_at FROM rooms ORDER BY sort_order, id")?;
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

    /// Catalog of all users: `(id, display_name)`; `display_name` falls
    /// back to `username` when unset. Sorted by name — for sender names
    /// in the UI (Wave 2.2).
    pub fn list_users(&self) -> Result<Vec<(i64, String)>, DbError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(display_name, username) FROM users ORDER BY display_name",
        )?;
        let out = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// Insert a user; returns the new `id`. `password_hash` may be `None`
    /// (AD/SSO accounts). `role` defaults to `Role::User` at the DB level if
    /// you omit it, but we always pass it explicitly.
    pub fn create_user(
        &self,
        username: &str,
        display_name: Option<&str>,
        role: Role,
        password_hash: Option<&str>,
    ) -> Result<i64, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO users (username, display_name, role, password_hash)
             VALUES (?1, ?2, ?3, ?4)",
            (username, display_name, role.as_str(), password_hash),
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Look up a user by username (case-insensitive — `username` is
    /// `COLLATE NOCASE`). `None` if the account does not exist.
    pub fn find_user_by_username(&self, username: &str) -> Result<Option<StoredUser>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, username, display_name, role, created_at, password_hash
             FROM users WHERE username = ?1",
        )?;
        let mut rows = stmt.query_map([username], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;
        match rows.next() {
            Some(row) => {
                let (id, username, display_name, role, created_at, password_hash) = row?;
                Ok(Some(StoredUser {
                    id,
                    username,
                    display_name,
                    role: role.parse().map_err(|e: String| DbError::BadRole(e))?,
                    created_at,
                    password_hash,
                }))
            }
            None => Ok(None),
        }
    }

    /// Overwrite a user's display name. Used by AD/SSO enrichment to keep the
    /// ФИО in sync with the directory. Returns the number of rows updated (0 if
    /// the id does not exist).
    pub fn update_user_display_name(&self, id: i64, display_name: &str) -> Result<usize, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        Ok(conn.execute(
            "UPDATE users SET display_name = ?2 WHERE id = ?1",
            (id, display_name),
        )?)
    }

    /// Fetch the public view of a user (no password hash) by id.
    pub fn get_user(&self, id: i64) -> Result<Option<User>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, username, display_name, role, created_at FROM users WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        match rows.next() {
            Some(row) => {
                let (id, username, display_name, role, created_at) = row?;
                Ok(Some(User {
                    id,
                    username,
                    display_name,
                    role: role.parse().map_err(|e: String| DbError::BadRole(e))?,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    // ----- Room membership (Wave 1.3) -----

    /// All rooms in sort order with the membership state of `user_id`
    /// (`None` = anonymous viewer: `joined` is false for every room).
    pub fn list_rooms_for_user(&self, user_id: Option<i64>) -> Result<Vec<RoomView>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT r.id, r.kind, r.title, r.created_at,
                    (SELECT COUNT(*) FROM room_members rm WHERE rm.room_id = r.id),
                    EXISTS (SELECT 1 FROM room_members rm
                            WHERE rm.room_id = r.id AND rm.user_id = ?1)
             FROM rooms r
             ORDER BY r.sort_order, r.id",
        )?;
        let rows = stmt.query_map(params![user_id], room_row)?;
        let mut rooms = Vec::new();
        for row in rows {
            rooms.push(to_room_view(row?)?);
        }
        Ok(rooms)
    }

    /// One room with the membership state of `user_id`; `None` if the room
    /// does not exist.
    pub fn room_view(&self, room_id: i64, user_id: i64) -> Result<Option<RoomView>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT r.id, r.kind, r.title, r.created_at,
                    (SELECT COUNT(*) FROM room_members rm WHERE rm.room_id = r.id),
                    EXISTS (SELECT 1 FROM room_members rm
                            WHERE rm.room_id = r.id AND rm.user_id = ?2)
             FROM rooms r WHERE r.id = ?1",
        )?;
        let mut rows = stmt.query_map(params![room_id, user_id], room_row)?;
        match rows.next() {
            Some(row) => Ok(Some(to_room_view(row?)?)),
            None => Ok(None),
        }
    }

    /// Join a room (idempotent — a member re-joining is a no-op).
    /// Returns `true` if the user was just added, `false` if already a member.
    pub fn join_room(&self, room_id: i64, user_id: i64) -> Result<bool, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO room_members (room_id, user_id) VALUES (?1, ?2)",
            (room_id, user_id),
        )?;
        Ok(inserted > 0)
    }

    /// Leave a room. Returns `true` if the user was removed, `false` if they
    /// were not a member (caller 404s).
    pub fn leave_room(&self, room_id: i64, user_id: i64) -> Result<bool, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let removed = conn.execute(
            "DELETE FROM room_members WHERE room_id = ?1 AND user_id = ?2",
            (room_id, user_id),
        )?;
        Ok(removed > 0)
    }

    /// One room by id; `None` if it does not exist (app maps to 404).
    pub fn get_room(&self, room_id: i64) -> Result<Option<Room>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt =
            conn.prepare("SELECT id, kind, title, created_at FROM rooms WHERE id = ?1")?;
        let mut rows = stmt.query_map([room_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        match rows.next() {
            Some(row) => {
                let (id, kind, title, created_at) = row?;
                Ok(Some(Room {
                    id,
                    kind: kind.parse().map_err(|k: String| DbError::BadKind(k))?,
                    title,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Is `user_id` a member of `room_id`? (membership gate for posting/reading)
    pub fn is_room_member(&self, room_id: i64, user_id: i64) -> Result<bool, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        Ok(conn
            .query_row(
                "SELECT 1 FROM room_members WHERE room_id = ?1 AND user_id = ?2",
                (room_id, user_id),
                |_| Ok(()),
            )
            .is_ok())
    }

    // ----- Messages (Wave 2.1, append-only) -----

    /// Append a message to a room; returns the new `id`.
    ///
    /// Foreign keys enforce room/user existence (both `ON DELETE CASCADE`),
    /// so the app checks 404/membership for friendlier errors.
    pub fn create_message(&self, room_id: i64, sender_id: i64, text: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO messages (room_id, sender_id, text) VALUES (?1, ?2, ?3)",
            (room_id, sender_id, text),
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// One message by id; `None` if it does not exist.
    pub fn get_message(&self, id: i64) -> Result<Option<Message>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, room_id, sender_id, text, created_at, edited_at FROM messages WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], message_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Cursor pagination over a room's messages (chat-scroll semantics):
    ///
    /// - `before_id = None` → the LATEST `limit` messages of the room;
    /// - `before_id = Some(id)` → the `limit` messages immediately BEFORE
    ///   `id` (i.e. older ones, `id` itself excluded).
    ///
    /// The returned page is always in ASCENDING order (oldest → newest),
    /// ready to render top-to-bottom.
    ///
    /// `limit` is expected to be 1..=200 (the app layer enforces this with
    /// HTTP 400 and defaults to 50); the DB clamps defensively
    /// (0 → 1, >200 → 200).
    pub fn list_messages(
        &self,
        room_id: i64,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<Message>, DbError> {
        let limit = limit.clamp(1, 200);
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, room_id, sender_id, text, created_at, edited_at
             FROM messages
             WHERE room_id = ?1 AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![room_id, before_id, limit as i64], message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut page = rows;
        page.reverse(); // DESC (fetch) → ASC (render oldest → newest)
        Ok(page)
    }

    /// Edit a message's text and stamp `edited_at`. The timestamp is
    /// generated in SQL with the SAME `strftime` expression as the
    /// `created_at` column default (`%Y-%m-%dT%H:%M:%fZ` — RFC 3339 UTC with
    /// milliseconds), so both fields always share one byte-identical format.
    ///
    /// Only the "does the row exist?" check is enforced here — "is the
    /// author allowed?" is the app layer's job (HTTP 403).
    ///
    /// Wave 2.4: the author may edit their own message any number of times;
    /// `edited_at` always reflects the LAST edit.
    pub fn update_message_text(&self, id: i64, text: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let updated = conn.execute(
            "UPDATE messages
             SET text = ?2,
                 edited_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            (id, text),
        )?;
        if updated == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Delete a message row (hard delete). `NotFound` when the id does not
    /// exist.
    pub fn delete_message(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let deleted = conn.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        if deleted == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    // ----- Wave 2.3: read cursors (read receipts) -----

    /// The newest message id in the room, or `None` when the room has no
    /// messages.
    pub fn latest_message_id(&self, room_id: i64) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.query_row(
            "SELECT MAX(id) FROM messages WHERE room_id = ?1",
            [room_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(DbError::from)
    }

    /// The user's current read cursor for the room (`None` = never read).
    pub fn get_read_cursor(&self, room_id: i64, user_id: i64) -> Result<Option<i64>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        match conn.query_row(
            "SELECT last_read_id FROM room_read_cursors WHERE room_id = ?1 AND user_id = ?2",
            (room_id, user_id),
            |row| row.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set (or bump) the user's read cursor. UPSERT with `MAX`: the stored
    /// cursor never moves backwards, so an older poll arriving late can't
    /// roll the receipt state back.
    pub fn set_read_cursor(
        &self,
        room_id: i64,
        user_id: i64,
        last_read_id: i64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO room_read_cursors (room_id, user_id, last_read_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (room_id, user_id) DO UPDATE SET
                 last_read_id = MAX(last_read_id, excluded.last_read_id)",
            (room_id, user_id, last_read_id),
        )?;
        Ok(())
    }

    /// Who has read a given message: room members whose cursor is at or past
    /// the message id, as `(user_id, display_name)` pairs sorted by name
    /// (`display_name` falls back to `username`, same as the user catalog).
    pub fn message_readers(
        &self,
        room_id: i64,
        message_id: i64,
    ) -> Result<Vec<(i64, String)>, DbError> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT u.id, COALESCE(u.display_name, u.username)
             FROM room_read_cursors rc
             JOIN room_members rm ON rm.room_id = rc.room_id AND rm.user_id = rc.user_id
             JOIN users u ON u.id = rc.user_id
             WHERE rc.room_id = ?1 AND rc.last_read_id >= ?2
             ORDER BY COALESCE(u.display_name, u.username)",
        )?;
        let rows = stmt
            .query_map((room_id, message_id), |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// Message query row.
fn message_row(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        room_id: row.get(1)?,
        sender_id: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
        // Wave 2.4: nullable column; NULL = never edited.
        edited_at: row.get::<_, Option<String>>(5)?,
    })
}

/// Room-view query row (kind still a string — parsed in [`to_room_view`]).
fn room_row(row: &rusqlite::Row) -> rusqlite::Result<(i64, String, String, String, i64, bool)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

/// Parse the kind string and build the public view.
fn to_room_view(
    (id, kind, title, created_at, member_count, joined): (i64, String, String, String, i64, bool),
) -> Result<RoomView, DbError> {
    Ok(RoomView {
        id,
        kind: kind.parse().map_err(DbError::BadKind)?,
        title,
        created_at,
        member_count,
        joined,
    })
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

    #[test]
    fn create_and_find_user() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .create_user(
                "alice",
                Some("Alice"),
                Role::User,
                Some("$argon2id$v=19$m=65536$fake"),
            )
            .unwrap();
        assert!(id > 0);

        let stored = db
            .find_user_by_username("alice")
            .unwrap()
            .expect("alice exists");
        assert_eq!(stored.id, id);
        assert_eq!(stored.username, "alice");
        assert_eq!(stored.role, Role::User);
        assert_eq!(stored.display_name.as_deref(), Some("Alice"));
        assert!(stored.password_hash.is_some());

        // Lookup is case-insensitive (username is COLLATE NOCASE).
        let upper = db
            .find_user_by_username("ALICE")
            .unwrap()
            .expect("ALICE == alice");
        assert_eq!(upper.id, id);
    }

    #[test]
    fn get_user_returns_public_view_without_hash() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_user("bob", None, Role::Manager, None).unwrap();
        let user = db.get_user(id).unwrap().expect("bob exists");
        assert_eq!(user.username, "bob");
        assert_eq!(user.role, Role::Manager);
        assert!(user.display_name.is_none());
        // StoredUser carries the hash slot; User does not.
        let stored = db.find_user_by_username("bob").unwrap().unwrap();
        assert!(stored.password_hash.is_none());
    }

    #[test]
    fn duplicate_username_rejected_case_insensitive() {
        let db = Db::open_in_memory().unwrap();
        db.create_user("carol", None, Role::User, None).unwrap();
        // Exact duplicate and case-variant duplicate both hit UNIQUE + NOCASE.
        assert!(db.create_user("carol", None, Role::User, None).is_err());
        assert!(db.create_user("CAROL", None, Role::User, None).is_err());
        assert_eq!(db.user_count().unwrap(), 1);
    }

    #[test]
    fn update_display_name_updates_only_that_user() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_user("alice", None, Role::User, None).unwrap();
        let b = db
            .create_user("bob", Some("Bob"), Role::User, None)
            .unwrap();

        assert_eq!(db.update_user_display_name(a, "Алиса А.").unwrap(), 1);
        assert_eq!(
            db.get_user(a).unwrap().unwrap().display_name.as_deref(),
            Some("Алиса А.")
        );
        // Neighbour untouched.
        assert_eq!(
            db.get_user(b).unwrap().unwrap().display_name.as_deref(),
            Some("Bob")
        );
        // Unknown id updates nothing (no error, no phantom row).
        assert_eq!(db.update_user_display_name(999, "Некто").unwrap(), 0);
    }

    #[test]
    fn list_users_returns_all_with_name_fallback_sorted() {
        let db = Db::open_in_memory().unwrap();
        let a = db
            .create_user("alice", Some("Alice"), Role::User, None)
            .unwrap();
        let b = db.create_user("bob", None, Role::User, None).unwrap(); // no display_name
        let c = db
            .create_user("anna", Some("Анна"), Role::Admin, None)
            .unwrap();

        let users = db.list_users().unwrap();
        assert_eq!(users.len(), 3);
        // Contains self + others.
        let ids: Vec<i64> = users.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&a) && ids.contains(&b) && ids.contains(&c));
        // Null display_name falls back to username.
        let bob = users.iter().find(|(id, _)| *id == b).unwrap();
        assert_eq!(bob.1, "bob");
        // Alice keeps her display name.
        let alice = users.iter().find(|(id, _)| *id == a).unwrap();
        assert_eq!(alice.1, "Alice");
    }

    #[test]
    fn list_users_empty_when_no_users() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.list_users().unwrap().is_empty());
    }

    #[test]
    fn find_missing_user_is_none_not_err() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.find_user_by_username("ghost").unwrap().is_none());
    }

    #[test]
    fn join_is_idempotent_and_leave_removes() {
        let db = Db::open_in_memory().unwrap();
        let user = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#all").unwrap();

        // First join adds, second join is a no-op.
        assert!(db.join_room(room, user).unwrap(), "first join adds");
        assert!(!db.join_room(room, user).unwrap(), "second join is a no-op");

        // Leave removes once; a second leave is a no-op.
        assert!(db.leave_room(room, user).unwrap(), "leave removes");
        assert!(
            !db.leave_room(room, user).unwrap(),
            "leave again is a no-op"
        );
    }

    #[test]
    fn room_view_carries_joined_and_member_count() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let bob = db.create_user("bob", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#all").unwrap();

        // Nobody joined yet.
        let view = db.room_view(room, alice).unwrap().unwrap();
        assert!(!view.joined);
        assert_eq!(view.member_count, 0);

        db.join_room(room, alice).unwrap();
        db.join_room(room, bob).unwrap();

        // Alice sees herself joined with two members; Bob the same.
        let alice_view = db.room_view(room, alice).unwrap().unwrap();
        assert!(alice_view.joined);
        assert_eq!(alice_view.member_count, 2);

        let bob_view = db.room_view(room, bob).unwrap().unwrap();
        assert!(bob_view.joined);
        assert_eq!(bob_view.member_count, 2);

        // After Alice leaves, Bob alone remains.
        db.leave_room(room, alice).unwrap();
        let bob_view = db.room_view(room, bob).unwrap().unwrap();
        assert!(bob_view.joined);
        assert_eq!(bob_view.member_count, 1);
    }

    #[test]
    fn list_rooms_for_user_flags_only_their_rooms() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let one = db.create_room(RoomKind::General, "Общий").unwrap();
        db.create_room(RoomKind::Channel, "#news").unwrap();
        db.join_room(one, alice).unwrap();

        let views = db.list_rooms_for_user(Some(alice)).unwrap();
        assert_eq!(views.len(), 2);
        let joined: Vec<bool> = views.iter().map(|v| v.joined).collect();
        assert_eq!(joined, vec![true, false]);
        assert_eq!(views[0].member_count, 1);
        assert_eq!(views[1].member_count, 0);

        // Anonymous viewer sees no joined rooms.
        let anon = db.list_rooms_for_user(None).unwrap();
        assert!(anon.iter().all(|v| !v.joined));
    }

    // ----- Wave 2.1: messages -----

    #[test]
    fn messages_table_exists_with_expected_indexes() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            tables.contains(&"messages".to_string()),
            "tables: {tables:?}"
        );

        let indexes: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'messages'",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            indexes.contains(&"idx_messages_room_id".to_string()),
            "indexes: {indexes:?}"
        );
        assert!(
            indexes.contains(&"idx_messages_sender".to_string()),
            "indexes: {indexes:?}"
        );
    }

    #[test]
    fn create_message_then_read_back() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        let id = db.create_message(room, alice, "привет").unwrap();
        assert!(id > 0);

        let msg = db.get_message(id).unwrap().expect("message exists");
        assert_eq!(msg.id, id);
        assert_eq!(msg.room_id, room);
        assert_eq!(msg.sender_id, alice);
        assert_eq!(msg.text, "привет");
        // RFC 3339 UTC (millisecond precision) default from the migration.
        assert!(
            msg.created_at.ends_with('Z') && msg.created_at.contains('T'),
            "created_at looks RFC 3339: {}",
            msg.created_at
        );

        // Unknown id → None, not an error.
        assert!(db.get_message(999_999).unwrap().is_none());
    }

    #[test]
    fn update_message_text_sets_text_and_edited_at() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        let id = db.create_message(room, alice, "первое").unwrap();
        // Fresh messages are never edited: NULL in the DB → None in the struct.
        assert!(db.get_message(id).unwrap().unwrap().edited_at.is_none());

        db.update_message_text(id, "второе").unwrap();
        let msg = db.get_message(id).unwrap().expect("still exists");
        assert_eq!(msg.text, "второе");
        // Timestamp shape: RFC 3339 UTC with milliseconds
        // (e.g. "2026-10-31T12:00:00.123Z", 24 chars) — same as created_at.
        let edited = msg.edited_at.expect("edited_at must be stamped");
        assert_eq!(edited.len(), 24, "unexpected edited_at shape: {edited}");
        assert!(
            edited.ends_with('Z') && edited.as_bytes()[10] == b'T',
            "unexpected edited_at shape: {edited}"
        );

        // Re-edit: text updates again, edited_at stays stamped.
        db.update_message_text(id, "третье").unwrap();
        let msg = db.get_message(id).unwrap().unwrap();
        assert_eq!(msg.text, "третье");
        assert!(msg.edited_at.is_some());

        // Unknown id → NotFound (the app layer maps it to HTTP 404).
        assert!(db.update_message_text(999_999, "x").is_err());
    }

    #[test]
    fn delete_message_removes_the_row() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        let id = db.create_message(room, alice, "удали меня").unwrap();
        assert!(db.get_message(id).unwrap().is_some());

        db.delete_message(id).unwrap();
        assert!(
            db.get_message(id).unwrap().is_none(),
            "deleted message must be gone"
        );
        assert_eq!(db.list_messages(room, 50, None).unwrap().len(), 0);

        // Double delete → NotFound, not a panic.
        assert!(db.delete_message(id).is_err());
    }

    // ----- Wave 2.3: read cursors (read receipts) -----

    #[test]
    fn latest_message_id_none_for_empty_room_some_for_posted() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        assert_eq!(
            db.latest_message_id(room).unwrap(),
            None,
            "empty room → None"
        );

        let id = db.create_message(room, alice, "first").unwrap();
        assert_eq!(db.latest_message_id(room).unwrap(), Some(id));
    }

    #[test]
    fn set_read_cursor_inserts_then_bumps_never_regresses() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let bob = db.create_user("bob", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();
        db.join_room(room, alice).unwrap();
        db.join_room(room, bob).unwrap();

        // No cursor yet → None.
        assert_eq!(db.get_read_cursor(room, alice).unwrap(), None);

        // First set inserts.
        db.set_read_cursor(room, alice, 5).unwrap();
        assert_eq!(db.get_read_cursor(room, alice).unwrap(), Some(5));

        // Bigger value wins…
        db.set_read_cursor(room, alice, 9).unwrap();
        assert_eq!(db.get_read_cursor(room, alice).unwrap(), Some(9));

        // …smaller value does NOT regress the cursor.
        db.set_read_cursor(room, alice, 3).unwrap();
        assert_eq!(
            db.get_read_cursor(room, alice).unwrap(),
            Some(9),
            "cursors only move forward"
        );

        // Bob's cursor is independent.
        assert_eq!(db.get_read_cursor(room, bob).unwrap(), None);
        db.set_read_cursor(room, bob, 2).unwrap();
        assert_eq!(db.get_read_cursor(room, bob).unwrap(), Some(2));
    }

    #[test]
    fn message_readers_lists_members_past_the_message_sorted_by_name() {
        let db = Db::open_in_memory().unwrap();
        let anna = db
            .create_user("anna", Some("Anna"), Role::User, None)
            .unwrap();
        let bob = db
            .create_user("bob", Some("Bob"), Role::User, None)
            .unwrap();
        let carol = db
            .create_user("carol", Some("Carol"), Role::User, None)
            .unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();
        db.join_room(room, anna).unwrap();
        db.join_room(room, bob).unwrap();
        db.join_room(room, carol).unwrap();

        let m1 = db.create_message(room, anna, "hello").unwrap();
        let m2 = db.create_message(room, anna, "world").unwrap();

        // Nobody read yet → no readers.
        assert!(db.message_readers(room, m1).unwrap().is_empty());

        // Bob read up to m1: sees m1, not m2.
        db.set_read_cursor(room, bob, m1).unwrap();
        let readers = db.message_readers(room, m1).unwrap();
        assert_eq!(readers, vec![(bob, "Bob".to_string())]);
        assert!(db.message_readers(room, m2).unwrap().is_empty());

        // Carol read up to m2 → she has read BOTH m1 and m2. Bob only read
        // up to m1, so he appears for m1 but not for m2.
        db.set_read_cursor(room, carol, m2).unwrap();
        assert_eq!(
            db.message_readers(room, m2).unwrap(),
            vec![(carol, "Carol".to_string())]
        );
        assert_eq!(
            db.message_readers(room, m1).unwrap(),
            vec![(bob, "Bob".to_string()), (carol, "Carol".to_string())]
        );

        // A non-member who somehow has a cursor is excluded by the join.
        let outsider = db.create_user("outsider", None, Role::User, None).unwrap();
        db.set_read_cursor(room, outsider, 999).unwrap();
        assert_eq!(db.message_readers(room, m2).unwrap().len(), 1);
    }

    #[test]
    fn list_messages_pagination_and_cursor() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        let mut ids = Vec::new();
        for i in 1..=7u32 {
            ids.push(db.create_message(room, alice, &format!("msg {i}")).unwrap());
        }

        // Default page: latest 5, ascending (oldest → newest).
        let page = db.list_messages(room, 5, None).unwrap();
        assert_eq!(
            page.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[2], ids[3], ids[4], ids[5], ids[6]]
        );

        // Cursor: 2 messages before ids[2] → [ids[0], ids[1]], ascending.
        let page = db.list_messages(room, 2, Some(ids[2])).unwrap();
        assert_eq!(
            page.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[0], ids[1]]
        );

        // Cursor at the oldest message → empty page.
        assert!(db.list_messages(room, 10, Some(ids[0])).unwrap().is_empty());

        // Unknown cursor id → behaves like "no cursor" (latest `limit`).
        let page = db.list_messages(room, 3, Some(999_999)).unwrap();
        assert_eq!(
            page.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[4], ids[5], ids[6]]
        );

        // A cursor pointing past all ids → latest `limit`.
        let page = db.list_messages(room, 2, Some(ids[6] + 1)).unwrap();
        assert_eq!(
            page.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[5], ids[6]]
        );

        // Boundary: limit 1 → only the newest message.
        let page = db.list_messages(room, 1, None).unwrap();
        assert_eq!(page.iter().map(|m| m.id).collect::<Vec<_>>(), vec![ids[6]]);

        // Boundary: limit 200 (max) → all 7.
        assert_eq!(db.list_messages(room, 200, None).unwrap().len(), 7);

        // Defensive clamp: 0 → 1, 9999 → 200 (all 7 still fit).
        assert_eq!(db.list_messages(room, 0, None).unwrap().len(), 1);
        assert_eq!(db.list_messages(room, 9999, None).unwrap().len(), 7);
    }

    #[test]
    fn list_messages_scoped_to_room() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room_a = db.create_room(RoomKind::Room, "A").unwrap();
        let room_b = db.create_room(RoomKind::Channel, "#b").unwrap();

        db.create_message(room_a, alice, "в A").unwrap();
        db.create_message(room_b, alice, "в B").unwrap();
        db.create_message(room_b, alice, "ещё в B").unwrap();

        let only_a = db.list_messages(room_a, 200, None).unwrap();
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].text, "в A");

        let only_b = db.list_messages(room_b, 200, None).unwrap();
        assert_eq!(only_b.len(), 2);
    }

    #[test]
    fn is_room_member_reflects_join_leave() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();

        assert!(!db.is_room_member(room, alice).unwrap());
        db.join_room(room, alice).unwrap();
        assert!(db.is_room_member(room, alice).unwrap());
        db.leave_room(room, alice).unwrap();
        assert!(!db.is_room_member(room, alice).unwrap());
    }

    #[test]
    fn deleting_room_cascades_to_messages() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();
        let id = db.create_message(room, alice, "сгинет").unwrap();

        // Гвардия — в блоке: дропнута ПЕРЕД вызовом Db-методов (иначе self-deadlock).
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM rooms WHERE id = ?1", [room])
                .unwrap();
        }
        assert!(
            db.get_message(id).unwrap().is_none(),
            "FK CASCADE removed it"
        );
        assert!(db.list_messages(room, 200, None).unwrap().is_empty());
    }

    #[test]
    fn deleting_user_cascades_to_messages() {
        let db = Db::open_in_memory().unwrap();
        let alice = db.create_user("alice", None, Role::User, None).unwrap();
        let room = db.create_room(RoomKind::Channel, "#dev").unwrap();
        let id = db.create_message(room, alice, "сгинет").unwrap();

        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM users WHERE id = ?1", [alice])
                .unwrap();
        }
        assert!(
            db.get_message(id).unwrap().is_none(),
            "FK CASCADE removed it"
        );
    }

    #[test]
    fn empty_room_lists_no_messages() {
        let db = Db::open_in_memory().unwrap();
        let room = db.create_room(RoomKind::Channel, "#empty").unwrap();
        assert!(db.list_messages(room, 50, None).unwrap().is_empty());
        assert!(db.list_messages(room, 50, Some(1)).unwrap().is_empty());
    }
}
