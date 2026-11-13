-- Finished ranked seasons (crate::league). One row per world and season;
-- a rollover replayed after a restart overwrites its row (upsert on the
-- key), and `seq` keeps the order rows were first written in.
CREATE TABLE seasons (
    world_id   TEXT NOT NULL,
    season     BIGINT NOT NULL,
    seq        BIGSERIAL NOT NULL UNIQUE,
    first_day  BIGINT NOT NULL,
    last_day   BIGINT NOT NULL,
    ended_unix BIGINT NOT NULL,
    -- league::SeasonHead (winners, summary, ...) and the full standings.
    head       JSONB NOT NULL,
    standings  JSONB NOT NULL,
    PRIMARY KEY (world_id, season)
);

-- Every ranked player's place in each category, for per-player history.
-- Rewritten with its season.
CREATE TABLE season_placements (
    world_id TEXT NOT NULL,
    season   BIGINT NOT NULL,
    player   BIGINT NOT NULL,
    category TEXT NOT NULL,
    rank     BIGINT NOT NULL,
    value    DOUBLE PRECISION NOT NULL,
    -- Players ranked in the category.
    ranked   BIGINT NOT NULL,
    PRIMARY KEY (world_id, season, player, category),
    FOREIGN KEY (world_id, season) REFERENCES seasons (world_id, season) ON DELETE CASCADE
);
CREATE INDEX season_placements_player ON season_placements (player);
