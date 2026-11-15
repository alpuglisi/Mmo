-- Retention of the daily history (crate::history): how much of a row its
-- age has removed (0 whole, 1 lineages with nothing to report dropped,
-- 2 gene means dropped too). Compaction thins a row only past its level;
-- an upsert (a replayed day) writes a whole row again.
ALTER TABLE daily_history ADD COLUMN thinned SMALLINT NOT NULL DEFAULT 0;
