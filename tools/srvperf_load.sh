#!/usr/bin/env bash
# The server load test of the survival scene, for the performance work (run on a Linux test machine through a shell with HL_DIR=scratch-lunaris-perf):
#   bash tools/srvperf_load.sh target/release/lunaris 200 40 [extra `lunaris server` arguments]
# The scene is read from ~/perf-ws/game/scenes/survival.lunaris.json (a copy of the game's; SCENE=path to use another, SAVE_EVERY=seconds to change `NetHost.save_every` in a copy of it).
# One live run at a time (flock /tmp/vb_live.lock, port 7961). Prints the server's own report lines and a summary (tools/srvperf_report.py).
LUN="$1"; BOTS="${2:-200}"; SECS="${3:-40}"; shift 3
PORT="${PORT:-7961}"; SCENE="${SCENE:-$HOME/perf-ws/game/scenes/survival.lunaris.json}"
LUN="$(readlink -f "$LUN")"; HERE="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${LOCK_HELD:-}" ]; then
  exec 9>/tmp/vb_live.lock
  flock 9
fi
W=$(mktemp -d /tmp/perf_load_XXXX); mkdir -p "$W/scenes"
if [ -n "${SAVE_EVERY:-}" ]; then
  python3 - "$SCENE" "$W/scenes/survival.lunaris.json" "$SAVE_EVERY" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
for e in d["entities"]:
    if "NetHost" in e["components"]:
        e["components"]["NetHost"]["save_every"] = float(sys.argv[3])
json.dump(d, open(sys.argv[2], "w"))
PY
else
  cp "$SCENE" "$W/scenes/survival.lunaris.json"
fi
cd "$W" || exit 1
uptime
"$LUN" server --scene scenes/survival.lunaris.json --players "$BOTS" --port "$PORT" --seconds $((SECS + 8)) --profile "$@" > server.log 2>&1 &
SP=$!
sleep 5
"$LUN" swarm "127.0.0.1:$PORT" --count "$BOTS" --seconds "$SECS" --map 300 --near 120 > swarm.log 2>&1
wait $SP
echo "--- swarm"; tail -2 swarm.log
python3 "$HERE/srvperf_report.py" server.log "$BOTS"
cd /tmp && rm -r "$W"
