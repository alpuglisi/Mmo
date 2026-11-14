-- Native push (crate::webpush::native): the device tokens of the Android
-- (FCM) and iOS (APNs) apps. Global, like push_subscriptions: a device
-- belongs to an account, not to a world.

-- One row per (service, token); registering a token again replaces its row
-- (a shared device moves to the account that registered it last).
CREATE TABLE push_devices (
    service      TEXT NOT NULL,
    token        TEXT NOT NULL,
    player       BIGINT NOT NULL,
    label        TEXT,
    created_unix BIGINT NOT NULL,
    PRIMARY KEY (service, token)
);
CREATE INDEX push_devices_player ON push_devices (player, created_unix);
