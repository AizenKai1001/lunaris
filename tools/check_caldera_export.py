"""Proves that "Caldera" runs from its exported folder alone: it exports the game with `lunaris export`, plays the exported copy as a hidden self-test (the player program that was copied into the
folder, started in the folder: no window, so no display is needed), taking the title to the island with a scripted Enter press, and plays the workspace copy the same way. The two must end in the same
state hash (the same game, tick for tick), on the island, with no scene error and the music still playing.

    python3 tools/check_caldera_export.py [path/to/lunaris]         (a hidden self-test opens no window: it runs without a display, over ssh or in CI)
"""
import json
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
EXE = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")
OUT = os.path.join(ROOT, "dist", "caldera_check")
FRAMES = 420
ARGS = ["--hidden", "--frames", str(FRAMES), "--mute", "--press", "enter@70:4,w@200:90", "--report"]
env = dict(os.environ)
failed = 0


def check(ok, what):
    global failed
    print(("PASS " if ok else "FAIL ") + what)
    failed += 0 if ok else 1


def play(cmd, cwd):
    p = subprocess.run(cmd + ARGS, cwd=cwd, env=env, capture_output=True, text=True, timeout=280)
    out = p.stdout + p.stderr
    m = re.search(r"state hash ([0-9a-f]{16})", out)
    r = re.search(r"^resources (\{.*\})$", out, re.M)
    return p.returncode, (m.group(1) if m else None), (json.loads(r.group(1)) if r else {}), out


shutil.rmtree(OUT, ignore_errors=True)
p = subprocess.run([EXE, "export", "examples/caldera/boot.lunaris.json", "--out", "dist/caldera_check", "--name", "caldera"], cwd=ROOT, capture_output=True, text=True, timeout=240)
check(p.returncode == 0, "lunaris export boot.lunaris.json -> dist/caldera_check")
manifest = json.load(open(os.path.join(OUT, "export.json"), encoding="utf-8"))
files = [f["path"] for f in manifest["files"]]
total = sum(f.get("size", 0) for f in manifest["files"])
check(manifest["missing"] == [], f"nothing missing ({len(files)} files, {total // 1024} KB)")
for want in ("examples/caldera/title.lunaris.json", "examples/caldera/island.lunaris.json", "examples/caldera/gameover.lunaris.json", "examples/caldera/win.lunaris.json", "examples/caldera/world/ground.png",
             "examples/caldera/world/splat.png", "examples/caldera/world/water.png", "examples/caldera/models/hero.glb", "examples/caldera/models/golem.glb", "examples/caldera/music_night.wav",
             "examples/caldera/amb_fire.wav", "examples/caldera/lang/de.json", "examples/caldera/lang/en.json", "examples/fonts/pixel.ttf"):
    check(want in files, f"the folder holds {want}")
check(not any(f.startswith("saves/") for f in files), "no saved games are exported")

player = os.path.join(OUT, "lunaris.exe" if os.name == "nt" else "lunaris")
shutil.rmtree(os.path.join(OUT, "saves"), ignore_errors=True)
code, exported, res, out = play([player, "play", "caldera.lunaris.json"], OUT)
check(code == 0 and exported is not None, f"the exported player ran {FRAMES} frames in its own folder (exit {code})")
check(res.get("music") == 2, f"and the title led to the island by day (music = {res.get('music')})")
check(res.get("scene.error", 0) == 0, "with no scene error")
check(res.get("hero.z", 0) != 0, "and the hero walked (the key W was pressed)")

shutil.rmtree(os.path.join(ROOT, "saves"), ignore_errors=True)
code, workspace, res2, out2 = play([EXE, "play", "examples/caldera/boot.lunaris.json"], ROOT)
check(code == 0 and workspace is not None, "the workspace copy ran the same frames")
check(exported == workspace, f"the exported game and the workspace game end in the same state ({exported} and {workspace})")
shutil.rmtree(os.path.join(ROOT, "saves"), ignore_errors=True)
if failed:
    print(out[-1500:])
print(f"{failed} failed")
sys.exit(1 if failed else 0)
