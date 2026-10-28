//! Besedka domain — pure types and logic, no I/O.
//!
//! Wave 0 ships the minimal set of domain types the API and UI are built on:
//! rooms (`Room`, `RoomKind`) and roles (`Role`). Wave 1.1 adds the user
//! model (`User`) and password primitives (`password`). Wave 2.1 adds
//! messages (`Message`, append-only; edit/delete come in Wave 2.3).
//! Later waves add voting and the AD-identity model (see
//! `CORP_CHAT_PLAN.md` §5).

pub mod ad;
pub mod message;
pub mod password;
pub mod role;
pub mod room;
pub mod user;

pub use ad::{AdClient, AdDirectory, AuthOutcome, DirectoryError, DirectoryResult};
pub use message::Message;
pub use password::{hash, verify};
pub use role::Role;
pub use room::{Room, RoomKind};
pub use user::User;
