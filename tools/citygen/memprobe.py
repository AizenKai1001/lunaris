"""Where does the memory go? Draw the same city scene several times and print the engine's resident memory after each step (run on a Linux test machine):

    python3 tools/citygen/memprobe.py ROOT SCENE

load -> step 150 -> render at the camera -> render again (same place) -> move 400 m -> step -> render -> render again.
"""
import json
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def rss_mb(pid):
    for line in open(f"/proc/{pid}/status"):
        if line.startswith("VmRSS"):
            return int(line.split()[1]) // 1024
    return -1


def breakdown(pid):
    out = {}
    for line in open(f"/proc/{pid}/status"):
        for k in ("RssAnon", "RssFile", "RssShmem"):
            if line.startswith(k):
                out[k] = int(line.split()[1]) // 1024
    return out


def main():
    root, scene = sys.argv[1], sys.argv[2]
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
        res = m.get("result") or {}
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    def say(what):
        print(f"{what:<44} rss {rss_mb(proc.pid):6d} MB  {breakdown(proc.pid)}", flush=True)

    call("load_scene", {"path": scene})
    say("loaded")
    keep = os.environ.get("KEEP")
    if keep:
        names = {7: "b0", 8: "b1", 9: "c2", 10: "in"}
        for uid, nm in names.items():
            if nm not in keep.split(","):
                call("despawn", {"uid": uid})
        print("kept", keep)
    qy = call("query", {"with": ["Camera"], "select": []})
    rows = qy.get("entities") or qy.get("results") or qy.get("matches") or []
    cam = rows[0].get("uid") if rows and isinstance(rows[0], dict) else rows[0]
    call("step", {"ticks": 150})
    say("stepped 150")
    q = os.environ.get("Q", "low")

    def draw(x, z, tag):
        eye = [float(v) for v in os.environ["EYE"].split(",")] if os.environ.get("EYE") else [x, 40.0, z]
        tgt = [float(v) for v in os.environ["TARGET"].split(",")] if os.environ.get("TARGET") else [x + 300, 20.0, z + 100]
        r = call("render", {"quality": q, "width": 320, "height": 180, "eye": eye, "target": tgt, "far": 9000.0, "path": "memprobe.png"})
        m = r.get("measured") or {}
        say(f"{tag} ({m.get('triangles')} triangles)")

    draw(0.0, 0.0, "render 1")
    draw(0.0, 0.0, "render 2 (same)")
    draw(0.0, 0.0, "render 3 (same)")
    call("set", {"uid": cam, "component": "Transform", "value": {"pos": [400.0, 40.0, 0.0]}})
    call("step", {"ticks": 150})
    say("moved 400 m, stepped")
    draw(400.0, 0.0, "render 4")
    draw(400.0, 0.0, "render 5 (same)")
    call("set", {"uid": cam, "component": "Transform", "value": {"pos": [3000.0, 40.0, 0.0]}})
    call("step", {"ticks": 700})
    say("moved 3 km, stepped 700 (trim)")
    draw(3000.0, 0.0, "render 6")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
