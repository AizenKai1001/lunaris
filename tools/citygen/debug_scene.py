"""Looks at a loaded scene: what the eye and weather entities do over a few hundred ticks (a debugging aid):

    python3 tools/citygen/debug_scene.py ROOT SCENE
"""
import json
import subprocess
import sys

proc = subprocess.Popen(["target/release/lunaris", "serve", "--root", sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
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
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


def rows(component):
    q = call("query", {"with": [component], "select": []})
    r = q.get("entities") or q.get("results") or q.get("matches") or []
    return [x.get("uid") if isinstance(x, dict) else x for x in r]


call("load_scene", {"path": sys.argv[2]})
probes, rain = rows("Probe"), rows("RainCloud")
print("probes", probes, "rain", rain)
for ticks in (1, 5, 30, 200):
    call("step", {"ticks": ticks})
    res = call("resources", {}).get("resources", {})
    print("tick +", ticks, {k: res.get(k) for k in ("interact_uid", "interact_kind", "eye_x", "eye_y", "eye_z", "covered", "rain")})
    print("   probes now", rows("Probe"), "rain now", rows("RainCloud"))
    for uid in rows("Probe") + [91]:
        c = call("get", {"uid": uid})
        print("   ", uid, str(c)[:260])
proc.stdin.close()
proc.wait(timeout=60)
