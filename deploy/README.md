# Deploying a public server

One machine, Docker Compose, three containers:

- **evo**: `evo-server` (release build) serving the API, the WebSocket and
  the built web client, from the repository's `Dockerfile`. Runs as uid
  10001, not root; its health check polls `GET /api/metrics`.
- **postgres**: PostgreSQL 16 holding everything: accounts, every world's
  saved games and terrain, history, economy, push subscriptions and the
  generated VAPID key. Data in the `pgdata` volume.
- **caddy**: HTTPS on ports 80/443 with a certificate from Let's Encrypt,
  renewed automatically, proxying everything (WebSocket included) to evo.

Only Caddy publishes ports. evo-server is started with `--trusted-proxy`
set to Caddy's fixed address, so its per-client rate limits see the real
client address from `X-Forwarded-For` (without it every player would share
one login and read limit: the proxy's).

## Sizing

From the measurements in the main README ([Several worlds:
capacity](../README.md#several-worlds-capacity)), per world (512², 500
active players, 30–40k organisms, 1 s ticks):

| Per world | Budget |
| --- | --- |
| CPU | 0.3 cores on average, in bursts: at most **1.5 worlds per core** |
| Memory | **0.5 GB**, plus 0.1 GB for the process and some for PostgreSQL |
| Egress | **1.5 MB/s** (12 Mbit/s); 100 Mbit/s carries about 8 worlds |
| Disk | ~100 MB of saved games (3 kept, 15–35 MB each at 512²) plus daily history; leave room for dumps |

So one world fits a 1–2 vCPU / 2 GB VPS; 2 vCPU / 4 GB holds 3 worlds; 4
vCPU / 8 GB holds 6. The default world is 128² and costs much less than the
512² figures. Login bursts are bounded by `EVO_HASH_CONCURRENCY` in `.env`
(default: half the cores).

## First start

On a Linux host with Docker Engine and the compose plugin, with DNS for
your domain pointing at it and ports 80 and 443 open:

```sh
git clone <this repository> evo && cd evo/deploy
cp .env.example .env
$EDITOR .env          # EVO_DOMAIN, ACME_EMAIL, POSTGRES_PASSWORD (openssl rand -hex 24), EVO_VAPID_SUBJECT
docker compose up -d --build
docker compose ps     # evo turns "healthy" once every world has loaded
docker compose logs -f evo caddy
```

Open `https://<EVO_DOMAIN>/`. The first build takes a while (Rust release
build and the client bundle); later ones reuse the build cache.

Server flags without an environment variable (world size and seed, season
length, offline protection, `--lod`, …; see [Server](../README.md#server))
go into the `command:` list of the `evo` service in `docker-compose.yml`.
Worlds go into `EVO_WORLDS` ([Several worlds](../README.md#several-worlds)).
A saved world must be started again with the seed, size and calendar it was
made with; the server refuses to start otherwise.

## Push notifications

- **Web Push** works without setup: on first start the server generates a
  VAPID key and keeps it in PostgreSQL. Set `EVO_VAPID_SUBJECT` to a real
  `mailto:` or `https:` contact. To bring your own key, set
  `EVO_VAPID_PRIVATE_KEY`. Losing the key unsubscribes every browser, so
  keep backups ([Push notifications](../README.md#push-notifications)).
- **Android (FCM) and iOS (APNs)** are off until configured. Get the key
  files as described in [Native push in the apps](../README.md#native-push-in-the-apps)
  (*Setting it up*), copy them into `deploy/secrets/` (git-ignored, mounted
  read-only at `/run/secrets/evo`), make them readable by the container
  user (`sudo chown 10001 secrets/* && sudo chmod 400 secrets/*`), and set
  `EVO_FCM_SERVICE_ACCOUNT=/run/secrets/evo/<file>.json` and/or
  `EVO_APNS_KEY=/run/secrets/evo/AuthKey_<id>.p8`, `EVO_APNS_KEY_ID`,
  `EVO_APNS_TEAM_ID` in `.env`, then `docker compose up -d`.

## Upgrades

```sh
cd evo && git pull
docker compose -f deploy/docker-compose.yml exec -T postgres pg_dump -U evo -Fc evo > deploy/backups/pre-upgrade-$(date +%F).dump
docker compose -f deploy/docker-compose.yml up -d --build
docker compose -f deploy/docker-compose.yml logs -f evo
```

`up -d --build` rebuilds the image and replaces the container: Docker sends
SIGTERM, the server stops serving, saves every world and exits (the compose
file allows 2 minutes), and the new one starts. **Database migrations run by
themselves** when the server connects (`crates/server/migrations/`,
embedded in the binary, applied by sqlx's migrator under a lock); there is
no separate step. They only go forward, hence the dump first.

After the start, look for `restored the world from tick …` for each world.
A line `skipping the saved game of tick …` means a save didn't load; if no
save loads, the server **starts a new world** and its daily saves soon
replace the old ones. That happens when rolling back to an image older than
the saves (newer save formats are refused; older ones down to version 10
load). If you see it, `docker compose stop evo` at once and go back to the
newer image, or restore the pre-upgrade dump together with the image it
belongs to.

## Backups and restore

With PostgreSQL **everything is in the database**, the world saves included
(`snapshots` and `terrains` tables, see
[Persistence](../README.md#persistence)), so one dump is a full backup. The
server saves each world once per simulated day and at shutdown; a dump
taken while it runs is consistent (a crash restore replays from the newest
save). Daily, from cron, and copy the files off the machine:

```sh
cd /path/to/evo/deploy
docker compose exec -T postgres pg_dump -U evo -Fc evo > backups/evo-$(date +%F).dump
find backups -name 'evo-*.dump' -mtime +14 -delete
```

Restore (onto the same or a new machine with the same `.env`):

```sh
docker compose up -d postgres
docker compose stop evo
docker compose exec -T postgres pg_restore -U evo -d evo --clean --if-exists < backups/evo-2026-11-14.dump
docker compose up -d
```

The `caddy_data` volume holds the certificates; it need not be backed up
(Caddy gets new ones), but keep it across upgrades (`docker compose down`
keeps volumes; `down -v` deletes them, **the database included**).

Without PostgreSQL (`docker run` of the image with no `DATABASE_URL`) the
file store lives in `/var/lib/evo/data` in the container's volume: per-world
saves in `worlds/<id>/snapshots/`, terrain in `worlds/<id>/terrain/`, the
rest beside them. Back it up by stopping the container and archiving the
volume.

## Logs and monitoring

- `docker compose logs -f evo` (or `caddy`, `postgres`); the compose file
  caps each container's logs at 5 × 20 MB. More detail: `RUST_LOG=debug`
  (or `evo_server=debug`) in `.env`, then `docker compose up -d`.
- `https://<EVO_DOMAIN>/api/metrics?world=<id>` reports a world's tick
  tick and state timings, connections, players, egress, saves, password
  hashing, process
  CPU and memory; `/api/worlds` lists worlds with population and players.
- Caddy writes no access log on purpose: WebSocket URLs carry the session
  token ([Security notes](../README.md#security-notes)). If you add one,
  filter the `token` query parameter out.

## Testing the stack locally

`scripts/deploy-smoke.sh` runs this compose file against `localhost` (Caddy
issues a certificate from its own CA) and checks health, the client page,
the redirect, a registration, a WebSocket upgrade through Caddy, that rate
limits stay per client behind Caddy, and that an account survives a restart
in PostgreSQL. CI runs it on every push:

```sh
docker build -t evo-server:smoke .
EVO_IMAGE=evo-server:smoke scripts/deploy-smoke.sh
```
