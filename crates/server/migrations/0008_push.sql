-- Web Push (crate::webpush). All global, like accounts: a subscription
-- belongs to an account, not to a world, and its notifications name the
-- world they come from.

-- Browsers' push subscriptions (PushSubscription.toJSON()), one row per
-- endpoint; subscribing an endpoint again replaces its row.
CREATE TABLE push_subscriptions (
    endpoint     TEXT PRIMARY KEY,
    player       BIGINT NOT NULL,
    p256dh       TEXT NOT NULL,
    auth         TEXT NOT NULL,
    label        TEXT,
    created_unix BIGINT NOT NULL
);
CREATE INDEX push_subscriptions_player ON push_subscriptions (player, created_unix);

-- The push kinds a player switched off, by wire name ('mating', 'death',
-- ...). One row per player, upserted when they change.
CREATE TABLE push_prefs (
    player     BIGINT PRIMARY KEY,
    off        TEXT[] NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Server secrets generated on first start (the VAPID private key), shared
-- by every server process on the database: the first one written wins.
CREATE TABLE server_secrets (
    name       TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
