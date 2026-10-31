-- Besedka V5: message editing + read receipts (Wave 2.3)
--
-- `edited_at` is NULL while a message is unedited; the API sets it to an
-- RFC 3339 UTC timestamp the moment the author edits their own message, so
-- the UI can render an "(edited)" marker.
-- Deletion in this wave is a hard DELETE of the row — no `deleted_by` column.
--
-- `room_read_cursors` powers read receipts: one row per (room, user) with the
-- id of the newest message that user has read. A message is "read by" a
-- member when their cursor is >= the message id (cursors only move forward).

ALTER TABLE messages ADD COLUMN edited_at TEXT;

CREATE TABLE room_read_cursors (
    room_id INTEGER NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    last_read_id INTEGER NOT NULL,
    PRIMARY KEY (room_id, user_id)
);
