"""The state hash and the CPU reference frame hash of every example scene: the simulation and the reference renderer promise the same bits on every
machine and operating system, and `tools/golden_hashes.txt` is that promise written down.

    python3 tools/xplat_hashes.py [path/to/lunaris]            print the hashes of this machine
    python3 tools/xplat_hashes.py [path/to/lunaris] --check    compare them with tools/golden_hashes.txt (exit 1 and say which scenes differ)
    python3 tools/xplat_hashes.py [path/to/lunaris] --write    rewrite tools/golden_hashes.txt (after a change that is meant to alter behaviour)

Each line: scene, state hash after 600 ticks (seed 1), hash of a 320x180 reference picture after those ticks. The file was produced on Windows
(MSVC) and reproduced bit for bit on Linux (rustc 1.99, gcc/LLVM) on 2026-10-14; a machine that disagrees has found a portability bug.
"""
import glob
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
GOLDEN = os.path.join(ROOT, "tools", "golden_hashes.txt")
args = [a for a in sys.argv[1:] if not a.startswith("--")]
MODE = "check" if "--check" in sys.argv else "write" if "--write" in sys.argv else "print"
exe = args[0] if args else os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")


def hashes_of(rel):
    """Load a scene, step 600 ticks, take the state hash and a reference picture: four requests to one `serve` process over stdio."""
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "load_scene", "arguments": {"path": rel}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "step", "arguments": {"ticks": 600}}},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "hash", "arguments": {}}},
        {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "render", "arguments": {"width": 320, "height": 180, "quality": "reference"}}},
    ]
    p = subprocess.run([exe, "serve", "--root", ROOT, "--no-gpu"], input="\n".join(json.dumps(r) for r in reqs) + "\n", capture_output=True, text=True, cwd=ROOT, timeout=300)
    out = {}
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        for c in (m.get("result") or {}).get("content") or []:
            if c.get("type") == "text":
                try:
                    out[m["id"]] = json.loads(c["text"])
                except ValueError:
                    out[m["id"]] = {}
    return (out.get(3) or {}).get("hash"), (out.get(4) or {}).get("hash")


lines = []
# a game that asks for its saved games (`game.probe`: the title screen of examples/depths) has the files in its state: start from a workspace without any, and give them back after
saves = os.path.join(ROOT, "saves")
held = saves + ".held-by-xplat-hashes"
if os.path.isdir(saves):
    os.replace(saves, held)
try:
    # the examples, and the scenes of the games that keep theirs in a folder of their own (examples/depths: a menu, three levels, a game over and a win screen)
    for scene in sorted(glob.glob(os.path.join(ROOT, "examples", "*.lunaris.json")) + glob.glob(os.path.join(ROOT, "examples", "depths", "*.lunaris.json"))):
        rel = os.path.relpath(scene, ROOT).replace(os.sep, "/")
        state, frame = hashes_of(rel)
        lines.append(f"{rel:40} state {state}  frame {frame}")
finally:
    shutil.rmtree(saves, ignore_errors=True)
    if os.path.isdir(held):
        os.replace(held, saves)

text = "\n".join(lines) + "\n"
if MODE == "print":
    sys.stdout.write(text)
elif MODE == "write":
    with open(GOLDEN, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {GOLDEN}: {len(lines)} scenes")
else:
    with open(GOLDEN, encoding="utf-8") as f:
        want = f.read().replace("\r\n", "\n").splitlines()
    bad = [(w, g) for w, g in zip(want, lines) if w != g]
    if bad or len(want) != len(lines):
        for w, g in bad:
            print("expected:", w)
            print("     got:", g)
        print(f"{len(bad)} scene(s) differ ({len(want)} expected, {len(lines)} produced): fix the change, or if behaviour was meant to change run with --write")
        sys.exit(1)
    print(f"all {len(lines)} scenes reproduce their recorded state and picture hashes")
