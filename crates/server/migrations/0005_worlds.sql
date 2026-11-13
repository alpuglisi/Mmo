-- Several worlds (shards) per server (crate::worlds). Accounts and alert
-- preferences stay global; every other table gets a `shard` column, the
-- configured world id, which scopes its rows to one world. Rows written
-- before there were several worlds belong to the default world, 'main'.
-- (`world_id`, where a table has it, still names one generated map of a
-- world and changes when `--fresh` starts the world over.)
--
-- Each column is added with a default so the existing rows get 'main',
-- then the default is dropped: every write names its world.

ALTER TABLE designs ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE designs ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE designs DROP CONSTRAINT designs_pkey, ADD PRIMARY KEY (shard, player);

ALTER TABLE daily_history ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE daily_history ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE daily_history DROP CONSTRAINT daily_history_pkey, ADD PRIMARY KEY (shard, world_id, day);
CREATE INDEX daily_history_shard_seq ON daily_history (shard, seq);

ALTER TABLE trades ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE trades ALTER COLUMN shard DROP DEFAULT;
DROP INDEX trades_players;
CREATE INDEX trades_players ON trades (shard, from_player, to_player);
CREATE INDEX trades_shard_id ON trades (shard, id);

ALTER TABLE snapshots ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE snapshots ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE snapshots DROP CONSTRAINT snapshots_pkey, ADD PRIMARY KEY (shard, tick);

ALTER TABLE terrains ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE terrains ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE terrains DROP CONSTRAINT terrains_pkey, ADD PRIMARY KEY (shard, world_id, hash);

ALTER TABLE balances ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE balances ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE balances DROP CONSTRAINT balances_pkey, ADD PRIMARY KEY (shard, player);

ALTER TABLE ledger ADD COLUMN shard TEXT NOT NULL DEFAULT 'main';
ALTER TABLE ledger ALTER COLUMN shard DROP DEFAULT;
ALTER TABLE ledger DROP CONSTRAINT ledger_pkey, ADD PRIMARY KEY (shard, world_id, seq);
DROP INDEX ledger_player;
CREATE INDEX ledger_player ON ledger (shard, player, id);
CREATE INDEX ledger_shard_id ON ledger (shard, id);
