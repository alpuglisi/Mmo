-- Players' alert preferences (crate::protocol::AlertPrefs): the alert kinds
-- left out of their feed and the kinds that pop up as a toast, by wire name
-- ('death', 'market', ...). One row per player, upserted when they change.
CREATE TABLE alert_prefs (
    player     BIGINT PRIMARY KEY,
    hidden     TEXT[] NOT NULL DEFAULT '{}',
    toast      TEXT[] NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
