"""Run every check the project has, one after another, and say what passed.

    python -I tools/check_all.py            (about two minutes; needs cargo, Python and Node 22+)
    python -I tools/check_all.py --quick    (skips the editor script and the long determinism soak)
    python -I tools/check_all.py --low      (builds and tests at below normal priority with four jobs: for when somebody is using the computer)

It builds the release binary first (the end-to-end scripts use it). Exit code 0 only if everything passed.
"""
import glob
import os
import subprocess
import sys
import time

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
QUICK = "--quick" in sys.argv
EXE = os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")

# stop a lunaris.exe left over from an earlier run: Windows will not overwrite a running binary
if os.name == "nt":
    # only programs started from this project's target folder: other copies (the MCP server of an AI session runs lunaris.exe from its own
    # folder) are somebody's tools and stay
    target = os.path.abspath(os.path.join(ROOT, "target")).replace("'", "''")
    ps = ("Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith('" + target + "', 'OrdinalIgnoreCase') -and "
          "@('lunaris.exe','lunaris-editor.exe','lunaris-player.exe') -contains $_.Name } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }")
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True)

steps = [
    ("build (release)", ["cargo", "build", "--release", "--offline"]),
    ("Rust tests (every crate, the fuzzers, the plugin and playtest integration tests)", ["cargo", "test", "--offline", "--workspace"]),
    ("MCP end to end", ["node", "tools/mcp_smoke.mjs"]),
    ("Python client tests", [sys.executable, "-I", "clients/python/test_lunaris.py"]),
    ("JavaScript client tests", ["node", "--test", "clients/js/test.mjs"]),
]
steps += [("playtest: " + os.path.basename(f), [EXE, "verify", f]) for f in sorted(glob.glob(os.path.join(ROOT, "examples", "tests", "*.test.json")))]
steps += [
    ("documentation: every mentioned file exists", [sys.executable, "-I", "tools/check_docs.py"]),
    ("bot plays Orb Dash and the run replays without it", [sys.executable, "-I", "tools/play_orb_dash.py", "--check"]),
    ("bot plays Moon Hopper and the run replays without it", [sys.executable, "-I", "tools/play_moon_hopper.py", "--check"]),
    ("the tutorial script (Coin Chase)", [sys.executable, "-I", "tools/tutorial_coin_chase.py"]),
    ("every example reproduces its recorded state and reference-picture hash (the same on Windows and Linux)", [sys.executable, "-I", "tools/xplat_hashes.py", "--check"]),
]
if not QUICK:
    steps.append(("every example: 1 thread = 8 threads = replay", [sys.executable, "-I", "tools/soak.py"]))

if os.name == "nt":
    # the native window, hidden: the GPU renderer and the reference renderer must see the same simulation (same state hash)
    def player_hash(extra):
        p = subprocess.run([EXE, "play", "examples/orb_dash.lunaris.json", "--hidden", "--frames", "90", "--hold", "d", "--mute", "--report"] + extra, cwd=ROOT, capture_output=True, text=True)
        if p.returncode != 0:
            raise RuntimeError(p.stdout + p.stderr)
        return [l for l in p.stdout.splitlines() if l.startswith("frames ")][0]

    def _player_step():
        a, b = player_hash([]), player_hash(["--cpu"])
        if a != b:
            raise RuntimeError("GPU run: " + a + " / CPU run: " + b)
    steps.append(("native player self-test, GPU and CPU renderers agree on the simulation", _player_step))

    def _gpu_render_step():
        """The render tool on the GPU (high quality) and on the CPU reference both give a picture of the same entities."""
        import json
        def render(*extra):
            p = subprocess.run([EXE, "call", "render", json.dumps({"width": 320, "height": 180, "image": False}), "--scene", "examples/vista.lunaris.json"] + list(extra), cwd=ROOT, capture_output=True, text=True)
            if p.returncode != 0:
                raise RuntimeError(p.stdout + p.stderr)
            return json.loads(p.stdout)
        gpu, cpu = render(), render("--no-gpu")
        if cpu["renderer"] != "cpu-reference" or not cpu["deterministic"]:
            raise RuntimeError("--no-gpu did not give the reference renderer: " + cpu["renderer"])
        if gpu["renderer"].startswith("gpu"):
            missing = {v["uid"] for v in cpu["visible"] if v["pixels"] >= 40} - {v["uid"] for v in gpu["visible"]}
            if missing:
                raise RuntimeError("the GPU picture misses entities: " + str(sorted(missing)))
    steps.append(("render tool: GPU (high quality) and CPU reference draw the same entities", _gpu_render_step))

    def _editor_window_step():
        """The Lunaris Editor opens (off screen, muted), draws a scene with the GPU renderer, and saves a picture of its window."""
        exe = os.path.join(ROOT, "target", "release", "lunaris-editor.exe")
        shot = os.path.join(ROOT, "target", "check_editor_window.png")
        if os.path.exists(shot):
            os.remove(shot)
        p = subprocess.run([exe, "examples/echo_tour.lunaris.json", "--root", ".", "--mute", "--select", "Radio", "--shot", shot, "--shot-delay", "2"], cwd=ROOT, capture_output=True, text=True, timeout=120)
        if p.returncode != 0 or not os.path.exists(shot):
            raise RuntimeError(p.stdout + p.stderr)
        # a window that drew nothing, or one colour, compresses to almost nothing
        if os.path.getsize(shot) < 150_000:
            raise RuntimeError("the picture of the editor window is nearly empty: " + str(os.path.getsize(shot)) + " bytes")
    steps.append(("Lunaris Editor (the Windows application) opens, draws a scene and saves a picture of its window", _editor_window_step))

    def _package_step():
        """A game built by the editor runs on its own, from its own folder, and plays exactly as it does in the repository."""
        import shutil
        out = os.path.join(ROOT, "target", "check_game")
        shutil.rmtree(out, ignore_errors=True)
        editor = os.path.join(ROOT, "target", "release", "lunaris-editor.exe")
        exe = os.path.join(out, "OrbDash", "OrbDash.exe")
        p = subprocess.run([editor, "examples/orb_dash.lunaris.json", "--root", ".", "--build", exe, "--mute"], cwd=ROOT, capture_output=True, text=True, timeout=120)
        if p.returncode != 0 or not os.path.exists(exe):
            raise RuntimeError(p.stdout + p.stderr)
        args = ["--hidden", "--frames", "90", "--hold", "d", "--mute", "--report"]
        game = subprocess.run([exe] + args, cwd=os.path.dirname(exe), capture_output=True, text=True, timeout=120)
        ref = subprocess.run([EXE, "play", "examples/orb_dash.lunaris.json"] + args, cwd=ROOT, capture_output=True, text=True, timeout=120)
        pick = lambda r: [l for l in r.stdout.splitlines() if l.startswith("frames ")]
        if game.returncode != 0 or not pick(game) or pick(game) != pick(ref):
            raise RuntimeError("built game: " + str(pick(game)) + " " + game.stderr + " / repository: " + str(pick(ref)))
    steps.append(("a game built by the editor runs from its own folder and ends in the same state as in the repository", _package_step))

results = []
for name, cmd in steps:
    if callable(cmd):
        t0 = time.time()
        try:
            cmd()
            ok, detail = True, ""
        except Exception as e:  # noqa: BLE001
            ok, detail = False, str(e)
        results.append((name, ok, time.time() - t0))
        print(f"{'PASS' if ok else 'FAIL'}  {name}  ({time.time() - t0:.0f} s)", flush=True)
        if not ok:
            print("      " + "\n      ".join(detail.strip().splitlines()[-15:]))
        continue
    t0 = time.time()
    if os.name == "nt" and cmd[0] == "cargo":
        # a test that loops and allocates once took the whole PC down (2026-10-14): cargo and everything it starts share a memory cap
        cmd = [sys.executable, "-I", os.path.join(ROOT, "tools", "memguard.py"), "--mem-gb", "16", "--timeout", "1500"] + (["--low"] if "--low" in sys.argv else []) + ["--"] + cmd
    env = dict(os.environ, CARGO_BUILD_JOBS="4") if "--low" in sys.argv else None
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace", env=env)
    ok = p.returncode == 0
    results.append((name, ok, time.time() - t0))
    print(f"{'PASS' if ok else 'FAIL'}  {name}  ({time.time() - t0:.0f} s)", flush=True)
    if not ok:
        print("      " + "\n      ".join((p.stdout + p.stderr).strip().splitlines()[-15:]))

failed = [r for r in results if not r[1]]
print(f"\n{len(results) - len(failed)} of {len(results)} checks passed in {sum(r[2] for r in results):.0f} s")
sys.exit(1 if failed else 0)
