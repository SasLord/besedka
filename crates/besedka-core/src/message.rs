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

/// A read receipt: which user read a message, plus that user's display name.
///
/// Wave 2.3: the readers endpoint builds one of these per member whose read
/// cursor is at or past the message. The wire format keeps the field names
/// `id`/`display_name` (mapped in the app layer), so this struct is the
/// domain representation of a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadReceipt {
    /// The user who read the message.
    pub user_id: i64,
    /// That user's display name (joined from the users table).
    pub display_name: String,
}

impl Message {
    /// Whether the user `actor_id` may edit this message.
    ///
    /// Only the author (`sender_id`) may edit their own message — everyone
    /// else, including room members, is denied. This is the single source of
    /// truth for the edit policy; the infra/app layers call it instead of
    /// duplicating the check.
    pub fn can_edit_by(&self, actor_id: i64) -> bool {
        self.sender_id == actor_id
    }

    /// Whether the user `actor_id` may delete this message.
    ///
    /// Only the author (`sender_id`) may delete their own message. Single
    /// source of truth for the delete policy.
    pub fn can_delete_by(&self, actor_id: i64) -> bool {
        self.sender_id == actor_id
    }
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

    #[test]
    fn read_receipt_serde_roundtrip() {
        let r = ReadReceipt {
            user_id: 7,
            display_name: "Анна".into(),
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: ReadReceipt = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn can_edit_by_author_only() {
        let m = sample(); // sender_id = 3
        assert!(m.can_edit_by(3), "the author may edit");
        assert!(!m.can_edit_by(4), "a stranger may not edit");
        assert!(!m.can_edit_by(1), "another member may not edit");
    }

    #[test]
    fn can_delete_by_author_only() {
        let m = sample(); // sender_id = 3
        assert!(m.can_delete_by(3), "the author may delete");
        assert!(!m.can_delete_by(4), "a stranger may not delete");
        assert!(!m.can_delete_by(1), "another member may not delete");
    }
}
