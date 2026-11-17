-- Player ids for new accounts, handed out by the database so that several
-- server processes sharing it never give two accounts the same id
-- (crate::accounts). Starts after the highest id in use.
CREATE SEQUENCE player_ids;
SELECT setval('player_ids', COALESCE((SELECT MAX(player) FROM accounts), 0) + 1, false);
