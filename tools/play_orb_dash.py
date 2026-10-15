"""A bot plays Orb Dash through the same tools a human's keyboard uses, then proves the run replays without it.

    python -I tools/play_orb_dash.py            # play, print a summary, write target/renders/orb_dash_*.png
    python -I tools/play_orb_dash.py --check    # also exit non-zero unless the bot wins and the replay matches

It reads the world with `query`, "presses keys" with `input`, advances with `step`, looks with `render`.
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
    frame = eng.render(width=640, height=360)
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(frame["png"])


with Engine(root=ROOT) as eng:
    eng.load_scene(path="examples/orb_dash.lunaris.json")
    player = eng.query(with_=["Player"], select=[])["entities"][0]["uid"]
    save(eng, "orb_dash_start.png")
    eng.record_start()
    decisions = 0
    while decisions < 400:
        res = eng.resources()["resources"]
        orbs = eng.query(with_=["Orb"], select=["Transform"], limit=100)["entities"]
        if not orbs or res.get("time_left", 1) <= 0:
            break
        px, pz = res["player.x"] if "player.x" in res else 0.0, res["player.z"] if "player.z" in res else 0.0
        # steer towards the nearest orb
        target = min((o["components"]["Transform"]["pos"] for o in orbs), key=lambda p: (p[0] - px) ** 2 + (p[2] - pz) ** 2)
        dx, dz = target[0] - px, target[2] - pz
        eng.input(values={"d": dx > 0.2, "a": dx < -0.2, "s": dz > 0.2, "w": dz < -0.2})
        eng.step(ticks=5)
        decisions += 1
        if decisions == 60:
            save(eng, "orb_dash_mid.png")
    eng.input(values={"d": False, "a": False, "s": False, "w": False})
    eng.step(ticks=30)  # let the banner and camera settle
    rec = eng.record_stop()
    res = eng.resources()["resources"]
    replay = eng.replay()
    save(eng, "orb_dash_end.png")
    score, left = int(res.get("score", 0)), res.get("time_left", 0)
    print(f"score {score}/12, {left:.2f} s left on the clock, {decisions} decisions, {eng.describe()['tick']} ticks")
    print(f"replay without the bot: {'identical' if replay['matches'] else 'DIFFERENT'} ({rec.get('entries', '?')} recorded commands)")
    if CHECK:
        assert score == 12, f"the bot should collect every orb, got {score}"
        assert left > 0, "the bot should beat the clock"
        assert replay["matches"], "the recording must replay to the same state"
        print("ok")
