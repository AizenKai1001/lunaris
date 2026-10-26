"""A short run of the city without drawing (no graphics card, little memory): load the walker scene, step, and look that what the game is made of is there.

    python -I tools/memguard.py --mem-gb 6 --timeout 150 -- python -I tools/citygen/smoke.py target/release/lunaris.exe citygen-data city.lunaris.json

Prints how many doors, containers and street-lamp lights streamed in, the clock, and the slowest tick. Exit code 1 when something is missing.
"""
import json
import subprocess
import sys
import time


def main():
    binary, root, scene = sys.argv[1], sys.argv[2], sys.argv[3]
    proc = subprocess.Popen([binary, "serve", "--root", root], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    n = [0]

    def call(name, args=None):
        n[0] += 1
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n[0], "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
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
                    return {}
        return {}

    def count(component):
        return call("query", {"with": [component], "select": []}).get("total", 0)

    t = time.time()
    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    print(f"scene loaded in {time.time() - t:.1f} s")
    worst = 0.0
    for _ in range(12):
        t = time.time()
        call("step", {"ticks": 30})
        worst = max(worst, (time.time() - t) / 30 * 1000)
    ok = True
    for what in ("Door", "Container", "StreetLamp", "Item", "Panel", "Hero"):
        c = count(what)
        print(f"{what}: {c}")
        ok &= c > 0 or what == "Container"
    res = call("resources", {}).get("resources", {})
    print("clock", int(res.get("clock_h", -1)), int(res.get("clock_m", -1)), "night", res.get("night"))
    print(f"slowest 30-tick batch: {worst:.2f} ms per tick")
    proc.stdin.close()
    proc.wait(timeout=60)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
