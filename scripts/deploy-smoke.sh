#!/usr/bin/env bash
# Smoke-test the deployment in deploy/: start evo-server, PostgreSQL and
# Caddy (HTTPS for `localhost` from Caddy's own CA) from an already built
# image, then check the health endpoint, the client page, the http → https
# redirect, a registration, a WebSocket upgrade through the proxy and that
# the account survives a restart (PostgreSQL). Tears everything down after.
#
#   docker build -t evo-server:smoke .
#   EVO_IMAGE=evo-server:smoke scripts/deploy-smoke.sh
#
# Needs docker with compose v2, curl, and ports 80 and 443 free.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="${EVO_IMAGE:-evo-server:smoke}"
project="evo-smoke"
envfile="$(mktemp)"
cat >"$envfile" <<EOF
EVO_IMAGE=$image
EVO_DOMAIN=localhost
ACME_EMAIL=ci@example.org
POSTGRES_PASSWORD=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
EVO_VAPID_SUBJECT=mailto:ci@example.org
EOF
compose() {
    docker compose -p "$project" --env-file "$envfile" -f "$root/deploy/docker-compose.yml" "$@"
}
cleanup() {
    status=$?
    if [ "$status" -ne 0 ]; then
        echo "--- smoke test failed; container logs:"
        compose logs --no-color --tail 200 || true
    fi
    if [ -n "${other:-}" ]; then docker rm -fv "$other" >/dev/null 2>&1 || true; fi
    compose down -v --remove-orphans >/dev/null 2>&1 || true
    rm -f "$envfile"
    exit "$status"
}
trap cleanup EXIT

base="https://localhost"
# Caddy's local CA is not in the system trust store: -k.
get() { curl -fsSk --retry 10 --retry-delay 2 --retry-all-errors "$@"; }

echo "--- starting the stack ($image)"
compose up -d --no-build --wait --wait-timeout 300

echo "--- health endpoint"
get "$base/api/metrics" | grep -q '"world"'

echo "--- client page"
get "$base/" | grep -qi '<!doctype html'
get "$base/sw.js" -o /dev/null

echo "--- http redirects to https"
code="$(curl -s -o /dev/null -w '%{http_code}' http://localhost/)"
[ "$code" = 308 ] || { echo "http://localhost/ answered $code, not 308"; exit 1; }

echo "--- register"
name="smoke$RANDOM"
creds="{\"name\":\"$name\",\"password\":\"smoke test password\"}"
token="$(get -X POST -H 'content-type: application/json' -d "$creds" "$base/api/register" \
    | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')"
[ -n "$token" ] || { echo "no token from /api/register"; exit 1; }

echo "--- WebSocket upgrade through the proxy"
# curl only reads the 101 and then waits on the open socket until
# --max-time; the status line is all this needs.
code="$(curl -sk --http1.1 -o /dev/null -w '%{http_code}' --max-time 3 \
    -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
    -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "$base/ws?token=$token" || true)"
[ "$code" = 101 ] || { echo "/ws answered $code, not 101"; exit 1; }

echo "--- rate limits are per client behind the proxy (--trusted-proxy)"
# Use up this client's read limit (a burst of 60, then 10 a second), then
# read 20 times in a row from another address (a container on the proxy's
# network): all must get through. With one shared limit only the few
# tokens refilled meanwhile would.
proxy_ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' "$(compose ps -q caddy)" | awk '{print $1}')"
other="$(docker run -d --rm --network "${project}_web" --entrypoint sleep "$image" 300)"
limited=""
for _ in $(seq 1 300); do
    code="$(curl -sk -o /dev/null -w '%{http_code}' "$base/api/species")"
    if [ "$code" = 429 ]; then limited=1; break; fi
done
[ -n "$limited" ] || { echo "300 reads of /api/species were never rate-limited"; exit 1; }
# shellcheck disable=SC2016 # expanded by the container's shell
codes="$(docker exec "$other" sh -c 'for i in $(seq 1 20); do
    curl -sk -o /dev/null -w "%{http_code} " --resolve "localhost:443:$1" https://localhost/api/species; done' sh "$proxy_ip")"
docker rm -fv "$other" >/dev/null
ok="$(echo "$codes" | tr ' ' '\n' | grep -cx 200 || true)"
[ "$ok" = 20 ] || { echo "a second client got: $codes(not 20 × 200: limits are not per client)"; exit 1; }

echo "--- restart: the account is still there (PostgreSQL)"
compose restart evo >/dev/null
compose up -d --no-build --wait --wait-timeout 300
get -X POST -H 'content-type: application/json' -d "$creds" "$base/api/login" | grep -q '"token"'
rows="$(compose exec -T postgres psql -U evo -d evo -tAc "select count(*) from accounts where name = '$name'")"
[ "$rows" = 1 ] || { echo "the account is not in PostgreSQL (rows: $rows)"; exit 1; }

echo "--- image runs as a non-root user"
user="$(docker inspect -f '{{.Config.User}}' "$image")"
[ -n "$user" ] && [ "$user" != root ] && [ "$user" != 0 ] || { echo "image user is '${user}'"; exit 1; }

echo "deploy smoke test passed"
