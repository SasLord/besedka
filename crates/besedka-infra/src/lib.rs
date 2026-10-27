//! Besedka infrastructure — I/O adapters: SQLite (rusqlite + refinery) and
//! AD/Kerberos (LDAP bind, Kerberos keytab + SPNEGO accept, mocks). WS and
//! files arrive in later waves.

pub mod ad;
pub mod db;

pub use db::{Db, DbError, RoomView, StoredUser};
