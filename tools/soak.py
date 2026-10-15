"""Determinism soak test over every example scene.

For each scene: load it, press random keys for a few thousand ticks (seeded), record the run, then check that

  * replaying the recording gives the same final state hash,
  * a second engine with 1 worker thread and one with 8 worker threads end in exactly the same state,
  * no graph system recorded a runtime error.

    python -I tools/soak.py [--ticks 3000] [--seed 7]
"""
import glob
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(HERE, "..")
ticks = int(sys.argv[sys.argv.index("--ticks") + 1]) if "--ticks" in sys.argv else 3000
seed = int(sys.argv[sys.argv.index("--seed") + 1]) if "--seed" in sys.argv else 7
KEYS = ["w", "a", "s", "d", "space", "arrowleft", "arrowright", "arrowup", "arrowdown"]


def run(scene, threads):
    """Play the scene with a seeded random keyboard; returns (final hash, replay matches, errors, entities)."""
    rng = random.Random(seed)
    with Engine(root=ROOT, threads=threads) as eng:
        eng.load_scene(path=scene)
        eng.record_start()
        done = 0
        while done < ticks:
            eng.input(values={k: rng.random() < 0.35 for k in KEYS})
            n = rng.choice([1, 2, 4, 8])
            eng.step(ticks=n)
            done += n
        eng.record_stop()
        replay = eng.replay()
        errors = [(s["name"], s["last_error"]) for s in eng.systems()["graph"] if s.get("last_error")]
        d = eng.describe()
        return eng.hash()["hash"], replay["matches"], errors, d["entities"]


bad = 0
for scene in sorted(glob.glob(os.path.join(ROOT, "examples", "*.lunaris.json"))):
    rel = os.path.relpath(scene, ROOT).replace("\\", "/")
    h1, ok1, err1, n1 = run(rel, 1)
    h8, ok8, err8, n8 = run(rel, 8)
    good = h1 == h8 and ok1 and ok8 and not err1 and not err8
    bad += not good
    print(f"{'PASS' if good else 'FAIL'}  {rel:38} {ticks} ticks, {n1} entities: 1 thread {h1}, 8 threads {h8}, replay {'ok' if ok1 and ok8 else 'DIFFERENT'}" + (f", errors {err1 or err8}" if err1 or err8 else ""))
print("\nall scenes deterministic" if not bad else f"\n{bad} scene(s) failed")
sys.exit(1 if bad else 0)
