-- Login sessions kept over a restart (crate::accounts): the SHA-256 of the
-- token (hex, never the token itself), its player and wall-clock times in
-- milliseconds. Saved at a graceful stop, read at start; a logout deletes
-- its row; rows past expires_unix_ms are ignored and dropped at the next
-- save.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    player BIGINT NOT NULL,
    name TEXT NOT NULL,
    created_unix_ms BIGINT NOT NULL,
    last_seen_unix_ms BIGINT NOT NULL,
    expires_unix_ms BIGINT NOT NULL
);
CREATE INDEX sessions_expires ON sessions (expires_unix_ms);
