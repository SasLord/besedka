-- Besedka V4: messages (Wave 2.1)
-- Append-only in 2.1: no `edited_at` / `deleted_by` yet (those arrive in Wave 2.3).
-- A "channel" is a room of kind 'channel' — deliberately no separate table.
-- Timestamps: RFC 3339 UTC with millisecond precision, same default as V1/V3.

CREATE TABLE messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id    INTEGER NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    sender_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text       TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Pagination: (room_id, id) — "latest N in a room" / "page before cursor" reads.
CREATE INDEX idx_messages_room_id ON messages (room_id, id);

-- "My messages" lookups (search, audit — future waves).
CREATE INDEX idx_messages_sender ON messages (sender_id);
