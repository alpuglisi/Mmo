#!/usr/bin/env bash
# Smoke-run the Android app on an emulator (or a device) against a local
# evo-server: install the APK, launch it, and drive it through its WebView
# (e2e/android-webview.mjs): login screen, register over the Server field,
# found a lineage, map and state frames, then background > 20 s and resume
# with the socket recovered. Screenshots, logcat, the server log and
# report.json go to $OUT.
#
# The APK must be a debug build (WebView debugging on) made for a plain-http
# server:
#   cd client && npm run build && EVO_CAP_CLEARTEXT=1 npx cap sync android \
#     && (cd android && ./gradlew assembleDebug)
# and an emulator must be booted (or a device attached), e.g.:
#   emulator -avd Pixel_API_34 -no-window -gpu swiftshader_indirect -no-snapshot-save &
#
#   client/e2e/android-emulator.sh [path/to/app-debug.apk]
#
# Env:
#   APK              the APK (default client/android/app/build/outputs/apk/debug/app-debug.apk)
#   ADB              adb binary (default adb); ANDROID_SERIAL picks the device
#   EVO_SERVER_BIN   evo-server binary (default target/release/evo-server)
#   EVO_PORT         server port on the host (default 8080)
#   EVO_APP_SERVER   the server as the app reaches it (default http://10.0.2.2:$EVO_PORT,
#                    the emulator's alias for the host's loopback; for a device
#                    on the LAN use http://<host LAN IP>:$EVO_PORT, which also
#                    needs EVO_BIND=0.0.0.0)
#   EVO_BIND         address the server binds (default 127.0.0.1)
#   OUT              output directory (default client/e2e/out/android)
#   ATTEMPTS         scenario attempts, each from a clean install state (default 2)
#   BG_SECONDS       time in the background (default 25)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
client="$(cd "$here/.." && pwd)"
root="$(cd "$client/.." && pwd)"

APK="${1:-${APK:-$client/android/app/build/outputs/apk/debug/app-debug.apk}}"
ADB="${ADB:-adb}"
APP_ID="${EVO_APP_ID:-org.evolutionmmo.client}"
SERVER_BIN="${EVO_SERVER_BIN:-$root/target/release/evo-server}"
PORT="${EVO_PORT:-8080}"
BIND="${EVO_BIND:-127.0.0.1}"
APP_SERVER="${EVO_APP_SERVER:-http://10.0.2.2:$PORT}"
OUT="${OUT:-$client/e2e/out/android}"
ATTEMPTS="${ATTEMPTS:-2}"
export ADB EVO_APP_ID="$APP_ID" EVO_APP_SERVER="$APP_SERVER" BG_SECONDS="${BG_SECONDS:-25}"

log() { echo "== $*"; }
die() { echo "android-emulator: $*" >&2; exit 1; }

# Run a command up to $1 times, 3 s apart.
retry() {
  local n=$1 i=1; shift
  until "$@"; do
    [ "$i" -ge "$n" ] && return 1
    echo "   (failed: $*; retry $i of $((n - 1)))"
    i=$((i + 1)); sleep 3
  done
}

[ -f "$APK" ] || die "no APK at $APK (see the header of this script)"
[ -x "$SERVER_BIN" ] || die "no server binary at $SERVER_BIN (cargo build --release -p evo-server)"
command -v node >/dev/null || die "node is needed"
command -v "$ADB" >/dev/null || die "adb not found (set ADB or put platform-tools on PATH)"
mkdir -p "$OUT"

pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup EXIT

log "waiting for the device"
"$ADB" wait-for-device
for i in $(seq 1 150); do
  [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ] && break
  [ "$i" = 150 ] && die "device did not finish booting"
  sleep 2
done
"$ADB" shell getprop ro.build.version.release | tr -d '\r' | sed 's/^/   Android /'
# Animations off and the screen kept on: fewer timing surprises.
for s in window_animation_scale transition_animation_scale animator_duration_scale; do
  "$ADB" shell settings put global "$s" 0 || true
done
"$ADB" shell svc power stayon true || true
"$ADB" shell input keyevent KEYCODE_WAKEUP || true
"$ADB" shell wm dismiss-keyguard || true

log "starting evo-server on $BIND:$PORT (app uses $APP_SERVER)"
"$SERVER_BIN" --fast --memory --size 64 --no-rate-limits --bind "$BIND:$PORT" >"$OUT/server.log" 2>&1 &
pids+=($!)
for i in $(seq 1 100); do
  curl -sf "http://127.0.0.1:$PORT/api/world" >/dev/null && break
  [ "$i" = 100 ] && { cat "$OUT/server.log"; die "server did not come up"; }
  sleep 0.2
done

log "installing $APK"
retry 3 "$ADB" install -r -t "$APK"

"$ADB" logcat -c || true
"$ADB" logcat -v threadtime >"$OUT/logcat.txt" 2>&1 &
pids+=($!)

status=1
for attempt in $(seq 1 "$ATTEMPTS"); do
  log "attempt $attempt of $ATTEMPTS"
  shots="$OUT/attempt-$attempt"
  mkdir -p "$shots"
  # A clean start: no saved server, session or tour progress.
  "$ADB" shell am force-stop "$APP_ID" || true
  "$ADB" shell pm clear "$APP_ID" >/dev/null || true
  retry 3 "$ADB" shell am start -W -n "$APP_ID/.MainActivity"
  if SCREENSHOT_DIR="$shots" node "$client/e2e/android-webview.mjs"; then
    status=0
    break
  fi
  "$ADB" exec-out screencap -p >"$shots/after-failure.png" 2>/dev/null || true
done

# The app's own lines (WebView console messages are tagged Capacitor/Console).
pid_filter="Capacitor|chromium|$APP_ID|AndroidRuntime|FATAL"
grep -E "$pid_filter" "$OUT/logcat.txt" >"$OUT/logcat-app.txt" || true
if [ "$status" = 0 ]; then log "android smoke passed (artifacts in $OUT)"; else log "android smoke FAILED (artifacts in $OUT)"; fi
exit "$status"
