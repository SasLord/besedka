//! Besedka domain — pure types and logic, no I/O.
//!
//! Wave 0 ships the minimal set of domain types the API and UI are built on:
//! rooms (`Room`, `RoomKind`) and roles (`Role`). Later waves add channels,
//! messages, voting and the AD-identity model (see `CORP_CHAT_PLAN.md` §5).

pub mod room;
pub mod role;

pub use room::{Room, RoomKind};
pub use role::Role;
