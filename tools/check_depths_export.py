"""Proves that "Depths" runs from its exported folder alone: it exports the game with `lunaris export`, plays the exported copy in a hidden window (the player program that
was copied into the folder, started in the folder), taking the menu to level 1 with a scripted key press, and plays the workspace copy the same way. The two must end in the
same state hash (the same game, tick for tick), in level 1, with no scene error and an autosave written inside the folder.

    python3 tools/check_depths_export.py [path/to/lunaris]         (on Linux it needs a display for the hidden window: DISPLAY=:0 is tried when there is none)
"""
import json
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
EXE = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")
OUT = os.path.join(ROOT, "dist", "depths_check")
FRAMES = 260
ARGS = ["--hidden", "--frames", str(FRAMES), "--mute", "--press", "enter@45:4", "--report"]
env = dict(os.environ)
if os.name != "nt" and not env.get("DISPLAY"):
    env["DISPLAY"] = ":0"
failed = 0


def check(ok, what):
    global failed
    print(("PASS " if ok else "FAIL ") + what)
    failed += 0 if ok else 1


def play(cmd, cwd):
    p = subprocess.run(cmd + ARGS, cwd=cwd, env=env, capture_output=True, text=True, timeout=240)
    out = p.stdout + p.stderr
    m = re.search(r"state hash ([0-9a-f]{16})", out)
    r = re.search(r"^resources (\{.*\})$", out, re.M)
    return p.returncode, (m.group(1) if m else None), (json.loads(r.group(1)) if r else {}), out


shutil.rmtree(OUT, ignore_errors=True)
p = subprocess.run([EXE, "export", "examples/depths/boot.lunaris.json", "--out", "dist/depths_check", "--name", "depths"], cwd=ROOT, capture_output=True, text=True, timeout=120)
check(p.returncode == 0, "lunaris export boot.lunaris.json -> dist/depths_check")
manifest = json.load(open(os.path.join(OUT, "export.json"), encoding="utf-8"))
files = [f["path"] for f in manifest["files"]]
check(manifest["missing"] == [], f"nothing missing ({len(files)} files)")
for want in ("examples/depths/menu.lunaris.json", "examples/depths/level3.lunaris.json", "examples/depths/win.lunaris.json", "examples/depths/tiles_maze.png", "examples/depths/music_depths.wav",
             "examples/depths/lang/de.json", "examples/depths/npc.png", "examples/fonts/pixel.ttf"):
    check(want in files, f"the folder holds {want}")
check(not any(f.startswith("saves/") for f in files), "no saved games are exported")

player = os.path.join(OUT, "lunaris.exe" if os.name == "nt" else "lunaris")
shutil.rmtree(os.path.join(OUT, "saves"), ignore_errors=True)
code, exported, res, out = play([player, "play", "depths.lunaris.json"], OUT)
check(code == 0 and exported is not None, f"the exported player ran {FRAMES} frames in its own folder (exit {code})")
check(res.get("level") == 1, f"and the menu led to level 1 (level = {res.get('level')})")
check(res.get("scene.error", 0) == 0, "with no scene error")
check(res.get("game.saved", 0) > 0 and os.path.isfile(os.path.join(OUT, "saves", "slot1.json")), "and an autosave was written inside the folder")

shutil.rmtree(os.path.join(ROOT, "saves"), ignore_errors=True)
code, workspace, res2, out2 = play([EXE, "play", "examples/depths/boot.lunaris.json"], ROOT)
check(code == 0 and workspace is not None, "the workspace copy ran the same frames")
check(exported == workspace, f"the exported game and the workspace game end in the same state ({exported} and {workspace})")
shutil.rmtree(os.path.join(ROOT, "saves"), ignore_errors=True)
if failed:
    print(out[-1500:])
print(f"{failed} failed")
sys.exit(1 if failed else 0)
