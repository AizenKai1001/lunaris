"""What does the wallhack defence cost? (docs/NETWORK_SCALE.md, "Wallhacks")  One game server on a town of closed buildings (`tools/build_occlusion_town.py`) and a swarm of bots over real UDP on this machine, once
with `NetHost.occlusion` off and once for each safe radius asked for; prints the server's tick and where it goes (`--profile`), the rays, and how many entities each bot knows about.

    python3 tools/occlusion_bench.py [path/to/lunaris] [--bots 200] [--seconds 40] [--radius 16] [--radius 24] [--lots 30] [--pitch 50] [--crowd] [--port 7861]

`--crowd` puts every player in one 120 m square (and the bots keep to it) instead of spreading them over the whole town. Run it from the repository root; the server's save and the scene go in a temporary folder.
"""
import os
import re
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
args = sys.argv[1:]


def opt(name, default, many=False):
    vals = [args[i + 1] for i, a in enumerate(args) if a == "--" + name and i + 1 < len(args)]
    if many:
        return vals
    return type(default)(vals[-1]) if vals else default


exe = next((a for a in args if not a.startswith("--") and not a.replace(".", "").isdigit() and os.path.exists(a)), os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris"))
exe = os.path.abspath(exe)
bots, seconds, lots, pitch, port = opt("bots", 200), opt("seconds", 40.0), opt("lots", 30), opt("pitch", 50.0), opt("port", 7861)
radii = [float(r) for r in opt("radius", "", many=True)] or [16.0]
crowd = "--crowd" in args
area = 60.0 if crowd else 0.0
half_map = 60.0 if crowd else lots * pitch / 2


def run(radius):
    work = tempfile.mkdtemp(prefix="occ_bench_")
    scene = "town.lunaris.json"
    gen = [sys.executable, "-I", os.path.join(ROOT, "tools", "build_occlusion_town.py"), os.path.join(work, scene), "--lots", str(lots), "--pitch", str(pitch), "--occlusion", str(radius), "--spawns", str(bots)]
    if crowd:
        gen += ["--spawn-area", str(area)]
    print(subprocess.run(gen, capture_output=True, text=True, check=True).stdout.strip())
    server = subprocess.Popen([exe, "server", "--scene", scene, "--players", str(bots + 20), "--port", str(port), "--seconds", str(seconds + 8), "--profile"], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    time.sleep(3)
    swarm = subprocess.run([exe, "swarm", f"127.0.0.1:{port}", "--count", str(bots), "--seconds", str(seconds), "--map", str(half_map), "--fire", "0"], capture_output=True, text=True, timeout=seconds + 120)
    out, _ = server.communicate(timeout=120)
    return out, swarm.stdout


def mean(xs):
    return sum(xs) / len(xs) if xs else float("nan")


def report(label, server_out, swarm_out):
    lines = server_out.splitlines()
    tick, phases, occ, knows, gathered = [], {}, [], [], ""
    for i, line in enumerate(lines):
        m = re.search(rf"players\s+(\d+)\s+ticks ([\d.]+)/s\s+tick (\d+) us avg (\d+) us max", line)
        if m and int(m.group(1)) >= bots * 0.98:
            tick.append((int(m.group(3)), int(m.group(4))))
            if i + 1 < len(lines) and "us a tick:" in lines[i + 1]:
                for name, v in re.findall(r"(\w+) (\d+)", lines[i + 1].split("us a tick:")[1]):
                    phases.setdefault(name, []).append(int(v))
            if i + 2 < len(lines) and "occlusion a tick:" in lines[i + 2]:
                occ.append([float(v) for v in re.findall(r"(\d+\.?\d*)", lines[i + 2].split("occlusion a tick:")[1])[:5]])
                gathered = lines[i + 2].split("(")[-1].rstrip(")")
    for line in swarm_out.splitlines():
        m = re.search(r"knows (\d+) entities", line)
        if m and re.search(rf"connected\s+{bots}/", line):
            knows.append(int(m.group(1)))
    if not tick:
        print(f"{label}: no full-strength server lines; server said:\n" + server_out[-1500:])
        return
    phase_text = "  ".join(f"{k} {mean(v):.0f}" for k, v in phases.items())
    occ_text = ""
    if occ:
        cols = list(zip(*occ))
        occ_text = f"   per tick: pairs {mean(cols[0]):.0f} rays {mean(cols[1]):.0f} still {mean(cols[2]):.0f} hidden {mean(cols[3]):.0f} unaffordable {mean(cols[4]):.1f}   ({gathered})"
    print(f"{label}: server tick {mean([t[0] for t in tick]):.0f} us avg (longest in 5 s: {max(t[1] for t in tick)} us) over {len(tick)} reports; each bot knows {mean(knows):.1f} entities\n    us a tick: {phase_text}{occ_text}")


for r in [0.0] + radii:
    out, swarm_out = run(r)
    report(f"safe radius {r:g} m" if r else "occlusion off", out, swarm_out)
    port += 1
