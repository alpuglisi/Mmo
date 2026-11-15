#!/usr/bin/env bash
# Smoke-run the iOS app in a simulator against a local evo-server: boot a
# simulator, install the app, start Appium, and drive the app's WKWebView
# (e2e/ios-webview.mjs, the same scenario as the Android run): login screen,
# register over the Server field, found a lineage, map and state frames,
# then the home button for > 20 s and back with the socket recovered. The
# server's /api/metrics must also show the app's socket and state frames.
# Screenshots, the app's log, Appium's log, the server log, metrics.json and
# report.json go to $OUT.
#
# macOS with Xcode only. The app must be a Debug simulator build (its web
# view is inspectable) made for a plain-http server:
#   cd client && npm run build && EVO_CAP_CLEARTEXT=1 npx cap sync ios \
#     && scripts/ios-build-sim.sh            # → ios-out/sim/App.app
# and Appium 3 with the XCUITest driver must be installed:
#   npm install -g appium@3.9.0 && appium driver install xcuitest@12.22.7
#
#   client/e2e/ios-simulator.sh [path/to/App.app]
#
# Env:
#   APP              the .app (default ios-out/sim/App.app)
#   SIM_UDID         simulator to use (default: the first available device whose
#                    name matches SIM_DEVICE on the newest iOS runtime; booted
#                    here if it is not)
#   SIM_DEVICE       device name regex for that choice (default ^iPhone)
#   EVO_SERVER_BIN   evo-server binary (default target/release/evo-server)
#   EVO_PORT         server port (default 8080)
#   EVO_APP_SERVER   the server as the app reaches it (default http://127.0.0.1:$EVO_PORT:
#                    the simulator shares the Mac's loopback)
#   APPIUM           appium binary (default appium); APPIUM_PORT (default 4723)
#   OUT              output directory (default client/e2e/out/ios)
#   ATTEMPTS         scenario attempts, each on a fresh install (default 2)
#   BG_SECONDS       time in the background (default 25)
#   XCRUN            xcrun binary (default xcrun)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
client="$(cd "$here/.." && pwd)"
root="$(cd "$client/.." && pwd)"

APP="${1:-${APP:-$root/ios-out/sim/App.app}}"
XCRUN="${XCRUN:-xcrun}"
APP_ID="${EVO_APP_ID:-org.evolutionmmo.client}"
SERVER_BIN="${EVO_SERVER_BIN:-$root/target/release/evo-server}"
PORT="${EVO_PORT:-8080}"
APP_SERVER="${EVO_APP_SERVER:-http://127.0.0.1:$PORT}"
APPIUM="${APPIUM:-appium}"
APPIUM_PORT="${APPIUM_PORT:-4723}"
OUT="${OUT:-$client/e2e/out/ios}"
ATTEMPTS="${ATTEMPTS:-2}"
SIM_DEVICE="${SIM_DEVICE:-^iPhone}"
export XCRUN EVO_APP_ID="$APP_ID" EVO_APP_SERVER="$APP_SERVER" BG_SECONDS="${BG_SECONDS:-25}"
export APPIUM_URL="http://127.0.0.1:$APPIUM_PORT"

log() { echo "== $*"; }
die() { echo "ios-simulator: $*" >&2; exit 1; }
simctl() { "$XCRUN" simctl "$@"; }

# Run a command up to $1 times, 3 s apart.
retry() {
  local n=$1 i=1; shift
  until "$@"; do
    [ "$i" -ge "$n" ] && return 1
    echo "   (failed: $*; retry $i of $((n - 1)))"
    i=$((i + 1)); sleep 3
  done
}

[ -d "$APP" ] || die "no app at $APP (see the header of this script)"
[ -x "$SERVER_BIN" ] || die "no server binary at $SERVER_BIN (cargo build --release -p evo-server)"
command -v node >/dev/null || die "node is needed"
command -v "$XCRUN" >/dev/null || die "xcrun not found (macOS with Xcode only)"
command -v "$APPIUM" >/dev/null || die "appium not found (npm install -g appium && appium driver install xcuitest)"
mkdir -p "$OUT"

pids=()
booted_here=""
# shellcheck disable=SC2329  # run by the EXIT trap
cleanup() {
  for p in ${pids[@]+"${pids[@]}"}; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [ -n "$booted_here" ] && [ -z "${KEEP_SIM:-}" ]; then simctl shutdown "$booted_here" 2>/dev/null || true; fi
}
trap cleanup EXIT

if [ -z "${SIM_UDID:-}" ]; then
  # The newest iOS runtime's first available device matching SIM_DEVICE.
  # shellcheck disable=SC2016  # JavaScript, not shell
  SIM_UDID="$(simctl list -j devices available | SIM_DEVICE="$SIM_DEVICE" node -e '
    let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
      const re = new RegExp(process.env.SIM_DEVICE);
      const ver = (k) => (k.match(/iOS-(\d+)-(\d+)/) || [0, 0, 0]).slice(1).map(Number);
      const runtimes = Object.keys(JSON.parse(s).devices).filter((k) => /SimRuntime\.iOS-/.test(k))
        .sort((a, b) => { const [x, y] = [ver(a), ver(b)]; return y[0] - x[0] || y[1] - x[1]; });
      const devices = JSON.parse(s).devices;
      for (const r of runtimes) {
        const d = devices[r].find((d) => d.isAvailable !== false && re.test(d.name));
        if (d) { console.log(d.udid); console.error(`   ${d.name} (${r.replace(/.*SimRuntime\./, "")}) ${d.udid} [${d.state}]`); return; }
      }
      process.exit(1);
    });')" || die "no available simulator matching $SIM_DEVICE (xcrun simctl list devices available)"
fi
export SIM_UDID

log "booting simulator $SIM_UDID"
if ! simctl list devices | grep -q "$SIM_UDID) (Booted)"; then
  simctl boot "$SIM_UDID"
  booted_here="$SIM_UDID"
fi
simctl bootstatus "$SIM_UDID" -b >/dev/null

log "starting evo-server on 127.0.0.1:$PORT (app uses $APP_SERVER)"
"$SERVER_BIN" --fast --memory --size 64 --no-rate-limits --bind "127.0.0.1:$PORT" >"$OUT/server.log" 2>&1 &
pids+=($!)
for i in $(seq 1 100); do
  curl -sf "http://127.0.0.1:$PORT/api/world" >/dev/null && break
  [ "$i" = 100 ] && { cat "$OUT/server.log"; die "server did not come up"; }
  sleep 0.2
done

log "starting Appium on 127.0.0.1:$APPIUM_PORT"
"$APPIUM" --address 127.0.0.1 --port "$APPIUM_PORT" --log-no-colors --log-timestamp >"$OUT/appium.log" 2>&1 &
pids+=($!)
for i in $(seq 1 150); do
  curl -sf "$APPIUM_URL/status" >/dev/null && break
  [ "$i" = 150 ] && { tail -50 "$OUT/appium.log"; die "Appium did not come up"; }
  sleep 0.2
done

# The app's own log lines (Capacitor prints the WebView console there too).
simctl spawn "$SIM_UDID" log stream --style compact --level debug --predicate 'process == "App"' >"$OUT/app.log" 2>&1 &
pids+=($!)

status=1
for attempt in $(seq 1 "$ATTEMPTS"); do
  log "attempt $attempt of $ATTEMPTS"
  shots="$OUT/attempt-$attempt"
  mkdir -p "$shots"
  # A clean start: no saved server, session or tour progress.
  simctl terminate "$SIM_UDID" "$APP_ID" 2>/dev/null || true
  simctl uninstall "$SIM_UDID" "$APP_ID" 2>/dev/null || true
  retry 3 simctl install "$SIM_UDID" "$APP"
  if SCREENSHOT_DIR="$shots" node "$client/e2e/ios-webview.mjs"; then
    status=0
    break
  fi
  simctl io "$SIM_UDID" screenshot --type=png "$shots/after-failure.png" >/dev/null 2>&1 || true
done

# The server's side: the app's socket opened and state frames went out to it.
# shellcheck disable=SC2016  # JavaScript, not shell
if curl -sf "http://127.0.0.1:$PORT/api/metrics" -o "$OUT/metrics.json"; then
  if ! node -e '
    const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    console.log(`   server: ws_opened ${m.ws_opened}, ws_closed ${m.ws_closed}, states_out ${m.states_out}, players_online ${m.players_online}`);
    process.exit(m.ws_opened >= 1 && m.states_out >= 1 ? 0 : 1);
  ' "$OUT/metrics.json"; then
    echo "   the server saw no app socket or sent no state frames"
    status=1
  fi
else
  echo "   (could not read /api/metrics)"
  status=1
fi

if [ "$status" = 0 ]; then log "ios smoke passed (artifacts in $OUT)"; else log "ios smoke FAILED (artifacts in $OUT)"; fi
exit "$status"
