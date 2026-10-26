"""Where a tick of a streamed world goes (the `profile` tool of the engine, run on a Linux test machine): python3 tools/realmap/profile_world.py ROOT SCENE [TICKS] [STEPS_BEFORE]"""
import json
import subprocess
import sys

BIN = "target/release/lunaris"
root, scene = sys.argv[1], sys.argv[2]
ticks = int(sys.argv[3]) if len(sys.argv) > 3 else 240
before = int(sys.argv[4]) if len(sys.argv) > 4 else 200
proc = subprocess.Popen([BIN, "serve", "--root", root], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n = [0]


def call(name, args=None):
    n[0] += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n[0], "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + chr(10))
    proc.stdin.flush()
    while True:
        m = json.loads(proc.stdout.readline())
        if m.get("id") == n[0]:
            break
    for c in (m.get("result") or {}).get("content") or []:
        if c.get("type") == "text":
            return c["text"]
    return ""


call("load_scene", {"path": scene})
call("step", {"ticks": before})
print(call("profile", {"ticks": ticks})[:6000])
proc.stdin.close()
proc.wait(timeout=60)
