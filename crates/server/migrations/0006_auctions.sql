-- How each marketplace auction ended (crate::store::AuctionRecord). Open
-- auctions and their highest bids live in the saved game with the other
-- listings; the bids' escrow and refunds are ledger rows (kind 'escrow' /
-- 'refund' with the listing and no offer). A world replaying days after a
-- restart from an older saved game may close the same auction again and
-- overwrites its row (upsert on the key); `id` keeps the order rows were
-- first written in.
CREATE TABLE auctions (
    world_id        TEXT NOT NULL,
    listing         BIGINT NOT NULL,
    id              BIGSERIAL NOT NULL UNIQUE,
    seller          BIGINT NOT NULL,
    organism        BIGINT NOT NULL,
    reserve         BIGINT NOT NULL,
    deadline_tick   BIGINT NOT NULL,
    bids            INTEGER NOT NULL,
    high_bid        BIGINT,
    winner          BIGINT,
    winner_organism BIGINT,
    -- 'sold', 'unsold', 'failed', 'ended' or 'withdrawn'.
    outcome         TEXT NOT NULL,
    -- The agreement the winning bid became ('sold').
    offer           BIGINT,
    day             BIGINT NOT NULL,
    closed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (world_id, listing)
);
CREATE INDEX auctions_seller ON auctions (seller, id);
