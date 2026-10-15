"""A bot plays Moon Hopper through the tools: it runs right, jumps over gaps and walls it can see, and collects what it touches.

    python -I tools/play_moon_hopper.py            # play, print a summary, write target/renders/moon_hopper_*.png
    python -I tools/play_moon_hopper.py --check    # also exit non-zero unless it gets far, collects coins and the replay matches
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(HERE, "..")
OUT = os.path.join(ROOT, "target", "renders")
CHECK = "--check" in sys.argv
os.makedirs(OUT, exist_ok=True)


def save(eng, name):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(eng.render(width=960, height=540)["png"])


with Engine(root=ROOT) as eng:
    eng.load_scene(path="examples/moon_hopper.lunaris.json")
    hero = eng.query(with_=["Hero"], select=[])["entities"][0]["uid"]
    boxes = []  # (x0, x1, top) of every solid box
    for e in eng.query(with_=["Ground"], select=["Transform", "Collider"], limit=500)["entities"]:
        t, c = e["components"]["Transform"]["pos"], e["components"]["Collider"]["size"]
        boxes.append((t[0] - c[0] / 2, t[0] + c[0] / 2, t[1] + c[1] / 2))

    def ground_under(x, feet):
        """Highest solid top within reach below (or at) the given feet height at x."""
        tops = [top for (x0, x1, top) in boxes if x0 <= x <= x1 and top <= feet + 0.6]
        return max(tops) if tops else None

    save(eng, "moon_hopper_start.png")
    eng.record_start()
    best_x, ticks_stuck, decisions, hold = -99.0, 0, 0, 0
    while decisions < 900:
        h = eng.get(uid=hero, components=["Transform", "Hero", "Velocity"])["components"]
        x, y = h["Transform"]["pos"][0], h["Transform"]["pos"][1]
        grounded = h["Hero"]["grounded"] > 0.5
        feet = y - 0.5
        ahead = ground_under(x + 0.9, feet)
        gap = ahead is None or ahead < feet - 0.9           # no floor where we are about to step
        wall = any(x + 0.35 < x1 and x0 < x + 0.9 and feet + 0.05 < top < feet + 3.2 for (x0, x1, top) in boxes)  # something to climb
        if grounded and (gap or wall) and hold == 0:
            hold = 9 if gap else 3  # hold the key longer for a bigger jump (letting go early cuts the jump short)
        eng.input(values={"d": True, "space": hold > 0})
        hold = max(0, hold - 1)
        eng.step(ticks=4)
        decisions += 1
        if x > best_x + 0.01:
            best_x, ticks_stuck = x, 0
        else:
            ticks_stuck += 1
        if decisions == 120:
            save(eng, "moon_hopper_mid.png")
        if x > 17.5 or ticks_stuck > 150:
            break
    eng.input(values={"d": False, "space": False})
    eng.step(ticks=20)
    eng.record_stop()
    replay = eng.replay()
    save(eng, "moon_hopper_end.png")
    res = eng.resources()["resources"]
    h = eng.get(uid=hero, components=["Transform"])["components"]["Transform"]["pos"]
    print(f"hero reached x = {h[0]:.1f} (level ends at 20), coins {int(res.get('score', 0))} / 12, {decisions} decisions, tick {eng.describe()['tick']}")
    print(f"replay without the bot: {'identical' if replay['matches'] else 'DIFFERENT'}")
    if CHECK:
        assert h[0] > 12, f"the bot should get across most of the level, got x = {h[0]:.1f}"
        assert res.get("score", 0) >= 2, "the bot should pick up the coins on its way"
        assert replay["matches"], "the recording must replay to the same state"
        print("ok")
