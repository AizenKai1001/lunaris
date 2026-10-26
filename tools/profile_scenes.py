"""Where does a tick go in each example scene? Runs the `profile` tool on every scene of examples/ (no rendering) and prints the cost of a tick and its three biggest stages.

    python3 tools/profile_scenes.py [path/to/lunaris] [--ticks N] [--top K]

The machine should be quiet: these are wall-clock times of a few hundred ticks.
"""
import glob
import json
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
args = [a for a in sys.argv[1:] if not a.startswith("--")]
ticks = 120
top = 3
if "--ticks" in sys.argv:
    ticks = int(sys.argv[sys.argv.index("--ticks") + 1])
    args = [a for a in args if a != str(ticks)]
if "--top" in sys.argv:
    top = int(sys.argv[sys.argv.index("--top") + 1])
    args = [a for a in args if a != str(top)]
exe = args[0] if args else os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")

scenes = sorted(glob.glob(os.path.join(ROOT, "examples", "**", "*.lunaris.json"), recursive=True))
rows = []
for path in scenes:
    rel = os.path.relpath(path, ROOT).replace("\\", "/")
    try:
        out = subprocess.run([exe, "call", "profile", json.dumps({"ticks": ticks}), "--scene", rel, "--root", ROOT], capture_output=True, text=True, timeout=120, cwd=ROOT)
        j = json.loads(out.stdout)
    except Exception as e:  # a scene that does not load, or a timeout
        rows.append((rel, None, str(e)[:60]))
        continue
    total = j.get("avg_tick_ms") or j.get("tick_ms") or j.get("total_ms_per_tick")
    stages = j.get("stages", [])
    if total is None:
        total = sum(s.get("avg_ms", 0.0) for s in stages if not s["stage"].startswith("system ") and not s["stage"].startswith("graph system "))
    best = sorted(stages, key=lambda s: -s.get("avg_ms", 0.0))[:top]
    rows.append((rel, total, ", ".join(f"{s['stage']} {s['avg_ms']:.3f}" for s in best)))

for rel, total, text in sorted(rows, key=lambda r: -(r[1] or 0)):
    print(f"{rel:48} {'' if total is None else f'{total:8.3f} ms':>11}  {text}")
