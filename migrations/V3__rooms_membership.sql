-- Besedka V3: room membership (Wave 1.3)
-- room_members links users to the rooms they have joined. The UNIQUE pair
-- makes re-joining idempotent (INSERT OR IGNORE is a no-op), and the FKs
-- keep the table in sync with rooms/users.

CREATE TABLE room_members (
    room_id   INTEGER NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (room_id, user_id)
);

CREATE INDEX idx_room_members_user ON room_members (user_id);
