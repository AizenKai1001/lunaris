"""Summarise the report lines of `lunaris server --profile` (tools/srvperf_load.sh): the windows with the players in, medians of the tick and the phases."""
import re
import statistics
import sys

log = open(sys.argv[1], errors="replace").read().splitlines()
bots = int(sys.argv[2])
rows = []
cur = None
for line in log:
    m = re.match(r"\s*(\d+)s\s+players\s+(\d+)\s+ticks ([\d.]+)/s\s+tick (\d+) us avg (\d+) us max.*simulation (\d+) us, replication (\d+) us", line)
    if m:
        cur = {"t": int(m[1]), "players": int(m[2]), "hz": float(m[3]), "avg": int(m[4]), "max": int(m[5]), "sim": int(m[6]), "repl": int(m[7]), "phase": {}, "extra": []}
        rows.append(cur)
        continue
    if cur is None:
        continue
    if line.strip().startswith("us a tick:"):
        for n, v in re.findall(r"(\w+) (\d+)", line.split(":", 1)[1]):
            cur["phase"][n] = int(v)
    elif "tick us by the clock" in line:
        m = re.search(r"clock: p50 (\d+) p90 (\d+) p99 (\d+) max (\d+)\s+by the processor: p50 (\d+) p99 (\d+) max (\d+)\s+ticks over 33 ms: (\d+) of (\d+)", line)
        if m:
            cur["clock"] = [int(x) for x in m.groups()]
    elif "slowest tick" in line or "occlusion a tick" in line or "saves so far" in line:
        cur["extra"].append(line.strip())
sim = False
for line in log:
    if line.startswith("simulation:"):
        sim = True
    if sim:
        print(line)
full = [r for r in rows if r["players"] >= bots * 0.95]
for r in rows:
    c = r.get("clock")
    print(f"  {r['t']:3d}s players {r['players']:3d} tick avg {r['avg']:6d} max {r['max']:6d}" + (f" p50 {c[0]} p99 {c[2]} cpu p50 {c[4]} cpu max {c[6]} over33 {c[7]}/{c[8]}" if c else "") + f"  occlude {r['phase'].get('occlude', 0)} game {r['phase'].get('game', 0)} send {r['phase'].get('send', 0)}")
    for e in r["extra"]:
        print("       " + e[:400])
if not full:
    print("no window with the players in")
    sys.exit(0)
med = lambda k: statistics.median(r[k] for r in full)
ph = lambda k: statistics.median(r["phase"].get(k, 0) for r in full)
p99 = [r["clock"][2] for r in full if "clock" in r]
over = sum(r["clock"][7] for r in full if "clock" in r)
tot = sum(r["clock"][8] for r in full if "clock" in r)
print(f"SUMMARY ({len(full)} windows with {bots} players): tick avg median {med('avg'):.0f} us, max of the windows {max(r['max'] for r in full)} us (median of window max {med('max'):.0f}), p99 median {statistics.median(p99) if p99 else 0:.0f} us, ticks over 33 ms {over}/{tot}")
print("  phases median (us a tick): " + " ".join(f"{k} {ph(k):.0f}" for k in ("recv", "game", "grids", "gather", "rank", "write", "send", "occlude")) + f"   game: simulation {med('sim'):.0f} replication {med('repl'):.0f}")
