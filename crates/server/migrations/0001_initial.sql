-- Evolution MMO persistence (store::PgStore). Unsigned game values are
-- stored as BIGINT; nested game records (designs, legacy boards, lineage
-- counts) as JSONB in the same shape the file store writes.

-- Accounts: unique name and player id, argon2 PHC hash.
CREATE TABLE accounts (
    name          TEXT PRIMARY KEY,
    player        BIGINT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_unix  BIGINT NOT NULL
);

-- One founder design per player (re-founded into a new world on restart).
CREATE TABLE designs (
    player       BIGINT PRIMARY KEY,
    design       JSONB NOT NULL,
    founded_day  BIGINT NOT NULL,
    founded_unix BIGINT NOT NULL
);

-- One row per simulated day per world. A world replaying days after a
-- restart from an older saved game overwrites them (upsert on the key);
-- `seq` keeps the order rows were first written in.
CREATE TABLE daily_history (
    world_id        TEXT NOT NULL,
    day             BIGINT NOT NULL,
    seq             BIGSERIAL NOT NULL UNIQUE,
    year            BIGINT NOT NULL,
    season          TEXT NOT NULL,
    breeding_adults BIGINT NOT NULL,
    legacy          JSONB NOT NULL,
    lineages        JSONB NOT NULL,
    PRIMARY KEY (world_id, day)
);

-- Directed-mating agreements that reached an end.
CREATE TABLE trades (
    id          BIGSERIAL PRIMARY KEY,
    offer       BIGINT NOT NULL,
    from_player BIGINT NOT NULL,
    to_player   BIGINT NOT NULL,
    mine        BIGINT NOT NULL,
    partner     BIGINT NOT NULL,
    price       BIGINT,
    status      TEXT NOT NULL,
    conceived   BOOLEAN,
    day         BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX trades_players ON trades (from_player, to_player);

-- Saved games (crate::snapshot blobs); the newest few are kept.
CREATE TABLE snapshots (
    tick           BIGINT PRIMARY KEY,
    day            BIGINT NOT NULL,
    size_bytes     BIGINT NOT NULL,
    -- World snapshot format version from the blob's header (NULL if the
    -- header does not parse).
    format_version INTEGER,
    saved_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    data           BYTEA NOT NULL
);
