-- The static terrain of each world (evo_sim::terrain::Terrain::encode:
-- biomes, relief, rivers, ground cover), stored once instead of in every
-- saved game. World snapshots from format version 5 on name their terrain
-- by content hash; a saved game's world id and that hash are the key.
-- Rows are written once and never change (same key, same content).
CREATE TABLE terrains (
    world_id   TEXT NOT NULL,
    -- Content hash as 16 lowercase hex digits (FNV-1a 64 of the terrain).
    hash       TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    saved_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    data       BYTEA NOT NULL,
    PRIMARY KEY (world_id, hash)
);

-- The terrain each saved game names (NULL for saved games that carry their
-- own: world snapshot format version 4 and earlier).
ALTER TABLE snapshots ADD COLUMN terrain_hash TEXT;
