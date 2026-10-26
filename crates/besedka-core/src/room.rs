//! Rooms — the core chat container (draft model — CORP_CHAT_PLAN.md §5).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// What kind of room it is. Wire format — `snake_case` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomKind {
    /// General room for everyone (e.g. «Новости»).
    General,
    /// Public room.
    Room,
    /// Channel with a posting policy.
    Channel,
    /// Direct (1-to-1) conversation.
    Direct,
}

impl RoomKind {
    /// Stable wire/DB string form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Room => "room",
            Self::Channel => "channel",
            Self::Direct => "direct",
        }
    }
}

impl fmt::Display for RoomKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RoomKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "general" => Ok(Self::General),
            "room" => Ok(Self::Room),
            "channel" => Ok(Self::Channel),
            "direct" => Ok(Self::Direct),
            other => Err(format!("unknown room kind: {other}")),
        }
    }
}

/// A chat room. The server is the single source of truth; the client only caches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: i64,
    pub kind: RoomKind,
    pub title: String,
    /// RFC 3339 UTC timestamp (second precision) — uniform API format.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_serde_roundtrip() {
        let room = Room {
            id: 7,
            kind: RoomKind::Channel,
            title: "Новости".into(),
            created_at: "2026-10-26T10:00:00Z".into(),
        };
        let json = serde_json::to_string(&room).unwrap();
        let back: Room = serde_json::from_str(&json).unwrap();
        assert_eq!(room, back);
        assert_eq!(json, r#"{"id":7,"kind":"channel","title":"Новости","created_at":"2026-10-26T10:00:00Z"}"#);
    }

    #[test]
    fn kind_parses_and_displays() {
        for kind in [RoomKind::General, RoomKind::Room, RoomKind::Channel, RoomKind::Direct] {
            assert_eq!(kind.as_str().parse::<RoomKind>().unwrap(), kind);
        }
        assert!("bogus".parse::<RoomKind>().is_err());
    }
}
