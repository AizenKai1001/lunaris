"""The performance gate: run `lunaris bench` a few times, keep the best of each row, and fail when a row is slower than the stored baseline of the SAME machine by more than its
tolerance, or when the memory an entity costs grew. A speed claim of the engine only counts while a change that breaks it is caught.

    python3 tools/perf_gate.py                        compare with tools/perf_baseline.json (exit 1 on a regression)
    python3 tools/perf_gate.py --update               measure and write the baseline for this machine (do this when a change is meant to move the numbers, and say so in the commit)
    python3 tools/perf_gate.py --bin target/release/lunaris --runs 5 --entities 200000
    python3 tools/perf_gate.py --selftest             check the gate's own comparison logic (no benchmark is run)

The baseline holds one entry per machine, named by its processor and core count (timings are only comparable on the same machine, the clock and the memory of another one would make
every number a false alarm); on a machine with no entry only the memory an entity costs is checked (it does not depend on the machine) and the script says how to add one.
Timings are noisy (other jobs, the clock): the best of several runs is used, a failure is measured again (up to twice) before it counts, and a row fails only when it is more than its tolerance above the baseline (30% for one thread, 60% for
rows that use all cores, 100% for the ones that are mostly allocation and JSON text or too small to be steady).
"""
import argparse
import json
import os
import platform
import subprocess
import sys

BASELINE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "perf_baseline.json")
# what the benchmark rows are allowed to lose against the baseline, as a fraction (rows not listed use DEFAULT)
DEFAULT = 0.30
TOLERANCE = {
    "step": 0.60,
    "graph system": 0.60,
    "render (threads)": 0.60,
    "scene export": 1.00,
    "scene import": 1.00,
    "spawn": 0.60,
    # rows that are the difference of two timings, or a fraction of a millisecond: too small to be steady
    "graph interpreted": 1.00,
    "graph (1 thread)": 1.00,
    "query pass": 1.00,
    "probe idle": 3.00,      # 150 empty probe checks: a few microseconds
}
# the memory an entity costs may grow by this much (it is deterministic: a change that adds a byte per entity is a change that should be seen)
MEMORY_TOLERANCE = 0.03


def machine_name():
    cpu = platform.processor() or platform.machine()
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    return f"{cpu} x{os.cpu_count()}"


def run_bench(binary, entities, ticks):
    out = subprocess.run([binary, "bench", "--entities", str(entities), "--ticks", str(ticks), "--json"], check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def best_of(runs):
    """The smallest time of each row over the runs, and the memory an entity cost (the same in every run, so the last)."""
    rows = {}
    for r in runs:
        for row in r["results"]:
            rows[row["name"]] = min(rows.get(row["name"], float("inf")), row["ms"])
    return rows, runs[-1].get("bytes_per_entity")


def compare(baseline, rows, bytes_per_entity):
    """What is worse than the baseline: a list of (what, now, was, allowed) lines; empty when the gate passes."""
    bad = []
    for name, was in baseline.get("rows", {}).items():
        now = rows.get(name)
        if now is None:
            bad.append((f"{name}: row missing from the benchmark", None, was, None))
            continue
        tol = TOLERANCE.get(name, DEFAULT)
        if now > was * (1.0 + tol):
            bad.append((f"{name}: {now:.3f} ms against {was:.3f} ms ({(now / was - 1) * 100:+.0f}%, at most +{tol * 100:.0f}% allowed)", now, was, tol))
    was_mem = baseline.get("bytes_per_entity")
    if was_mem and bytes_per_entity and bytes_per_entity > was_mem * (1.0 + MEMORY_TOLERANCE):
        bad.append((f"memory an entity costs: {bytes_per_entity:.0f} bytes against {was_mem:.0f} ({(bytes_per_entity / was_mem - 1) * 100:+.1f}%, at most +{MEMORY_TOLERANCE * 100:.0f}% allowed)", bytes_per_entity, was_mem, MEMORY_TOLERANCE))
    return bad


def selftest():
    base = {"rows": {"state hash": 15.0, "step": 1.0}, "bytes_per_entity": 100.0}
    assert compare(base, {"state hash": 15.5, "step": 1.2}, 101.0) == [], "small changes pass"
    assert compare(base, {"state hash": 19.4, "step": 1.0}, 100.0) == [], "29% slower is inside the 30% of one thread"
    bad = compare(base, {"state hash": 20.0, "step": 1.0}, 100.0)
    assert len(bad) == 1 and "state hash" in bad[0][0], bad
    assert compare(base, {"state hash": 15.0, "step": 1.55}, 100.0) == [], "rows that use every core are allowed 60%"
    assert len(compare(base, {"state hash": 15.0, "step": 1.7}, 100.0)) == 1
    bad = compare(base, {"state hash": 15.0, "step": 1.0}, 104.0)
    assert len(bad) == 1 and "memory" in bad[0][0], bad
    assert compare(base, {"state hash": 15.0}, 100.0)[0][0].startswith("step: row missing"), "a row that vanished is a failure, not a pass"
    assert compare({"rows": {}}, {}, None) == [], "an empty baseline gates nothing"
    rows, mem = best_of([{"results": [{"name": "a", "ms": 3.0}], "bytes_per_entity": 99}, {"results": [{"name": "a", "ms": 2.0}], "bytes_per_entity": 99}])
    assert rows == {"a": 2.0} and mem == 99
    print("perf_gate selftest: ok")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", default=os.path.join("target", "release", "lunaris.exe" if os.name == "nt" else "lunaris"))
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--entities", type=int, default=200000)
    ap.add_argument("--ticks", type=int, default=20)
    ap.add_argument("--update", action="store_true", help="write the baseline for this machine")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return 0
    runs = [run_bench(a.bin, a.entities, a.ticks) for _ in range(max(1, a.runs))]
    rows, mem = best_of(runs)
    machine = machine_name()
    try:
        with open(BASELINE, encoding="utf-8") as f:
            store = json.load(f)
    except (OSError, ValueError):
        store = {"entities": a.entities, "machines": {}}
    if a.update:
        store["entities"] = a.entities
        store.setdefault("machines", {})[machine] = {"rows": {k: round(v, 3) for k, v in rows.items()}, "bytes_per_entity": mem}
        with open(BASELINE, "w", encoding="utf-8", newline="\n") as f:
            json.dump(store, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"baseline for {machine} written to {BASELINE} ({len(rows)} rows, best of {len(runs)} runs)")
        return 0
    if store.get("entities") != a.entities:
        print(f"the baseline was measured with {store.get('entities')} entities, this run uses {a.entities}: nothing to compare")
        return 2
    entry = store.get("machines", {}).get(machine)
    if entry is not None:
        bad = compare(entry, rows, mem)
        # a slow round on a machine that has other work (a soak test, a build) is not a regression: measure again, up to twice, and judge on the best of everything
        for again in range(2):
            if not bad:
                break
            print(f"slower than the baseline ({len(bad)} row(s)): measuring again ({again + 1} of 2) before calling it a regression")
            runs += [run_bench(a.bin, a.entities, a.ticks) for _ in range(max(1, a.runs))]
            rows, mem = best_of(runs)
            bad = compare(entry, rows, mem)
    if entry is None:
        # no timings for this machine: the memory of an entity does not depend on it, so that is still checked
        known = next(iter(store.get("machines", {}).values()), {})
        bad = compare({"bytes_per_entity": known.get("bytes_per_entity")}, rows, mem)
        print(f"no baseline for this machine ({machine}): only the memory an entity costs is checked; run with --update to add one")
    for line, *_ in bad:
        print("SLOWER  " + line)
    print(f"perf gate: {'FAILED' if bad else 'ok'} on {machine} (best of {len(runs)} runs, {a.entities} entities)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
