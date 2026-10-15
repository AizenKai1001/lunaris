"""Load a scene, optionally add entities and step, and render one picture with the `lunaris` binary: for looking at what a change to a renderer does.

    python3 tools/render_scene.py examples/vista.lunaris.json out.png [--add '{"PostFx":{"shafts":0.8}}']... [--steps N] [--hold a,w]
        [--quality high|ultra|reference|classic|...] [--size 960x540] [--eye x,y,z --target x,y,z --fov 60] [--bin target/release/lunaris]

`--add` takes a JSON object of components and may be given several times (each becomes one new entity). The picture is written to the given path
(relative to the workspace root, the folder this script is run from), so on a Linux test machine use it through `python3 tools/render_scene.py ...`.
"""
import argparse
import json
import subprocess
import sys

ap = argparse.ArgumentParser()
ap.add_argument("scene")
ap.add_argument("out")
ap.add_argument("--add", action="append", default=[])
ap.add_argument("--steps", type=int, default=0)
ap.add_argument("--hold", help="keys held down during --steps, e.g. a,w: a hero that walks to where the picture is taken")
ap.add_argument("--quality", default="high")
ap.add_argument("--size", default="960x540")
ap.add_argument("--eye")
ap.add_argument("--target")
ap.add_argument("--fov", type=float, default=60.0)
ap.add_argument("--prime", help="EYE;TARGET (x,y,z;x,y,z): render once from there first, so that motion blur has a previous picture")
ap.add_argument("--bin", default="target/release/lunaris")
a = ap.parse_args()

w, h = (int(v) for v in a.size.split("x"))
reqs = [{"name": "load_scene", "arguments": {"path": a.scene}}]
for comp in a.add:
    reqs.append({"name": "spawn", "arguments": {"components": json.loads(comp)}})
if a.hold:
    reqs.append({"name": "input", "arguments": {"values": {k: True for k in a.hold.split(",")}}})
if a.steps:
    reqs.append({"name": "step", "arguments": {"ticks": a.steps}})
if a.hold:
    reqs.append({"name": "input", "arguments": {"values": {k: False for k in a.hold.split(",")}}})
if a.prime:
    pe, pt = a.prime.split(";")
    reqs.append({"name": "render", "arguments": {"width": w, "height": h, "quality": a.quality, "image": False, "eye": [float(v) for v in pe.split(",")], "target": [float(v) for v in pt.split(",")], "fov": a.fov}})
render = {"width": w, "height": h, "quality": a.quality, "path": a.out, "image": False}
if a.eye:
    render["eye"] = [float(v) for v in a.eye.split(",")]
    render["target"] = [float(v) for v in (a.target or "0,0,0").split(",")]
    render["fov"] = a.fov
reqs.append({"name": "render", "arguments": render})
lines = [json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": r}) for i, r in enumerate(reqs)]
p = subprocess.run([a.bin, "serve", "--root", "."], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=240)
last = None
for line in p.stdout.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if "error" in m:
        print("error:", m["error"])
        sys.exit(1)
    for c in (m.get("result") or {}).get("content") or []:
        if c.get("type") == "text":
            last = c["text"]
if last:
    try:
        r = json.loads(last)
        print(r.get("renderer"), r.get("hash"), "warnings:", r.get("warnings"))
    except ValueError:
        print(last[:300])
print("wrote", a.out)
