//! Chat message (Wave 2.1 append-only; Wave 2.3 adds `edited_at` + deletion).

use serde::{Deserialize, Serialize};

/// One message in a room.
///
/// A "channel" is just a room with [`RoomKind::Channel`](crate::RoomKind) —
/// there is deliberately no separate channel table (rooms are already
/// typed).
///
/// **Timestamps:** `created_at` is RFC 3339 UTC with millisecond precision
/// (e.g. `2026-10-27T16:31:20.123Z`), the single timestamp format used
/// across the whole project (DB defaults use
/// `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`; see migrations V1/V3/V4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Stable DB row id — also serves as the pagination cursor.
    pub id: i64,
    /// Room (channel) the message lives in.
    pub room_id: i64,
    /// Author (`users.id`).
    pub sender_id: i64,
    /// Message text (trimmed by the API layer, 1..=4096 chars).
    pub text: String,
    /// Creation time, RFC 3339 UTC (millisecond precision) — see above.
    pub created_at: String,
    /// Last edit time, RFC 3339 UTC; `None` = never edited (the column is
    /// nullable in the DB). Serialized as JSON `null` (see the wire
    /// contract: `edited_at` is "ISO or null").
    #[serde(default)]
    pub edited_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Message {
        Message {
            id: 1,
            room_id: 2,
            sender_id: 3,
            text: "привет".into(),
            created_at: "2026-10-27T16:31:20.123Z".into(),
            edited_at: None,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let m = sample();
        let json = serde_json::to_string(&m).expect("serialize");
        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }
}
