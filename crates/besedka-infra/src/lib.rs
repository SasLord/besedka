//! Besedka infrastructure — I/O adapters (SQLite now; AD/Kerberos, WS, files later).

pub mod db;

pub use db::{Db, DbError, StoredUser};
