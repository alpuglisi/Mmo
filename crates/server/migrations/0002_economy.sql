-- The renown economy (crate::economy): balances, the ledger, and how the
-- escrowed price of each finished trade was settled.

-- One balance per player, upserted on every change.
CREATE TABLE balances (
    player      BIGINT PRIMARY KEY,
    balance     BIGINT NOT NULL,
    updated_day BIGINT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Every balance change. A world replaying days after a restart from an
-- older saved game writes the same sequence numbers again and overwrites
-- them (upsert on the key); `id` keeps the order rows were first written in.
CREATE TABLE ledger (
    world_id TEXT NOT NULL,
    seq      BIGINT NOT NULL,
    id       BIGSERIAL NOT NULL UNIQUE,
    player   BIGINT NOT NULL,
    tick     BIGINT NOT NULL,
    day      BIGINT NOT NULL,
    delta    BIGINT NOT NULL,
    balance  BIGINT NOT NULL,
    kind     TEXT NOT NULL,
    text     TEXT NOT NULL,
    offer    BIGINT,
    listing  BIGINT,
    PRIMARY KEY (world_id, seq)
);
CREATE INDEX ledger_player ON ledger (player, id);

-- Trades bought from a marketplace listing, and the escrow's fate
-- ('paid' or 'refunded'; NULL when nothing was held).
ALTER TABLE trades ADD COLUMN listing BIGINT;
ALTER TABLE trades ADD COLUMN settlement TEXT;
