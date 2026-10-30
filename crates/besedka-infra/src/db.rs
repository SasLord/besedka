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
            "SELECT id, room_id, sender_id, text, created_at FROM messages WHERE id = ?1",
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
            "SELECT id, room_id, sender_id, text, created_at
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
}

/// Message query row.
fn message_row(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        room_id: row.get(1)?,
        sender_id: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
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
