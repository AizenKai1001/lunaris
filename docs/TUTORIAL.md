# Your first game in fifteen minutes

You will build **Coin Chase**: roll a blue ball around a field to collect ten spinning coins, with a camera that follows the ball and a score on screen. You make it the way an AI agent would, by calling the engine's tools from a script; then you play it, test it, and open it in the editor. Everything below is the file `tools/tutorial_coin_chase.py`, which you can run as it is.

You need the built engine (`cargo build --release`) and Python 3.10+ (nothing to install). Any other language works the same way: the tools are JSON in, JSON out (`docs/EMBEDDING.md`).

```python
import math, os, sys
sys.path.insert(0, "clients/python")           # the client is one file
from lunaris import Engine

with Engine(root=".") as eng:                   # starts `lunaris serve` and talks to it
    eng.reset(seed=1)
```

Every tool is a method on `eng` (`eng.tools` lists them). If a call is wrong, you get an error with a stable `code`, a `path` into your arguments and a `hint` that says how to fix it: read the hint, fix, retry.

## 1. Components: your own data

A world is entities (just numbers) made of **components** (plain data with named fields). Standard ones exist (`Transform`, `MeshRenderer`, `Light`, `Camera`, `Collider`, ...); define your own for anything specific to your game:

```python
    eng.define_component(name="Hero", doc="The thing the player moves.", fields=[{"name": "speed", "type": "f32", "default": 6}])
    eng.define_component(name="Coin", doc="Something to collect.", fields=[])      # a marker: no data, just a label
```

## 2. The world

```python
    def spawn(name, **components):
        return eng.spawn(components={"Name": {"value": name}, **components})["uid"]

    spawn("Floor", Transform={"scale": [40, 1, 40]}, MeshRenderer={"mesh": "plane", "color": [0.25, 0.4, 0.3, 1]})
    spawn("Sun", Transform={"rot": [-0.5, 0.2, 0, 0.84]}, Light={"kind": 0, "intensity": 1.1})
    hero = spawn("Hero", Transform={"pos": [0, 0.5, 0]}, Velocity={}, Hero={"speed": 6},
                 Collider={"shape": 0}, RigidBody={"gravity_scale": 0},
                 MeshRenderer={"mesh": "sphere", "color": [0.3, 0.55, 1.0, 1], "shininess": 80})
```

Units are metres, y is up, rotations are quaternions `[x, y, z, w]`. The hero has a sphere `Collider` (`shape` 0; 1 is a box) and a `RigidBody` that ignores gravity.

**Attach the camera to the hero** and it follows without any code. `Parent` names the entity to follow; `LocalTransform` is where the camera sits relative to it (11 up, 9 back, tilted down about 50 degrees):

```python
    spawn("Camera", Transform={}, Parent={"target": hero}, LocalTransform={"pos": [0, 11, 9], "rot": [-0.4226, 0, 0, 0.9063]}, Camera={"fov_y": 55})
```

Ten coins in a ring. They are **triggers** (touching them reports a contact but does not push anything), and they spin because the engine turns angular velocity into rotation, no system needed. `emissive` makes them glow:

```python
    for i in range(10):
        a = i * 2 * math.pi / 10
        spawn(f"Coin {i + 1}", Transform={"pos": [round(4 * math.cos(a), 2), 0.6, round(4 * math.sin(a), 2)], "scale": [0.5, 0.5, 0.5]},
              Coin={}, Velocity={"ang": [0, 3, 0]},
              Collider={"shape": 0, "trigger": True}, MeshRenderer={"mesh": "sphere", "color": [1.0, 0.8, 0.2, 1], "emissive": 0.4})
```

A sound and a score on screen. `Text` is drawn over the picture by the engine; `{score}` is replaced by the world resource of that name, live:

```python
    spawn("Sfx coin", Sound={"name": "coin", "wave": 1, "freq": 880, "freq_end": 1320, "duration": 0.12, "volume": 0.2})
    spawn("Score", Text={"value": "Coins {score} / 10", "pos": [0.02, 0.03], "size": 0.06})
    eng.set_resource(name="score", value=0)
```

## 3. The rules: graph systems

Behaviour is **data too**: a graph system is a JSON description of what to do for every entity that has some components, every tick, deterministically, with no compiler (`docs/GRAPH.md` has the whole language; the editor draws it as nodes).

```python
    def axis(plus, minus):                          # +1 while a plus key is held, -1 for a minus key
        held = lambda keys: {"max": [{"res": f"input.{k}"} for k in keys]}
        return {"sub": [held(plus), held(minus)]}

    eng.define_system(name="hero_move", each=["Hero", "Velocity"], do=[
        {"set": "Velocity.lin[0]", "to": {"mul": [{"get": "Hero.speed"}, axis(["d", "arrowright"], ["a", "arrowleft"])]}},
        {"set": "Velocity.lin[2]", "to": {"mul": [{"get": "Hero.speed"}, axis(["s", "arrowdown"], ["w", "arrowup"])]}},
    ])
    eng.define_system(name="coin_pickup", on_contact={"a": ["Hero"], "b": ["Coin"]}, do=[
        {"res_add": "score", "by": 1},
        {"emit": "coin"},                           # a named event: the Sound entity above plays it
        {"despawn": "b"},                           # the coin disappears
    ])
```

Keys arrive as **resources** (`input.d`, `input.arrowleft`, ...): numbers in the world that systems read with `{"res": "..."}`. `on_contact` runs once for every pair of touching entities where `a` has the first list of components and `b` the second.

## 4. Look, check, save

```python
    report = eng.check()                            # the usual mistakes: no camera, a missing texture, a resource nothing sets
    assert report["ok"], report
    frame = eng.render(width=480, height=270)       # frame["png"] holds the picture
    open("target/coin-chase.png", "wb").write(frame["png"])
    eng.save_scene(path="examples/coin_chase.lunaris.json")
```

`save_scene` writes everything you did as a text file you can read, diff and edit. Open `target/coin-chase.png`: a blue ball in a ring of gold coins.

## 5. Play it

The keyboard of a person and the "keys" of a program are the same thing: `input` sets keys, `step` advances time.

```python
    eng.input(values={"d": True})
    eng.step(ticks=60)                              # one second at 60 ticks per second
    eng.input(values={"d": False})
    print(eng.resources()["resources"]["score"])    # 1: the ball ran through the first coin
```

For a person: `lunaris play examples/coin_chase.lunaris.json` (a native window; WASD or the arrows) or `lunaris edit examples/coin_chase.lunaris.json` (the Lunaris Editor, a Windows application: press Play, click things, edit their fields in the inspector; drag one entity onto another in the hierarchy to attach it, e.g. hang more things on the ball).

## 6. Prove it works: a playtest as data

`examples/tests/coin_chase.test.json` states what must be true: ten coins, no check errors, the camera hangs 11 above the hero, and after holding D for a second the score is at least 1, a coin is gone and the camera has followed. Run it:

```
target/release/lunaris verify examples/tests/coin_chase.test.json
```

Every expectation prints PASS or FAIL with what was seen and what was wanted, and the run is also recorded and replayed to check it reproduces exactly. Because the simulation is deterministic, a test that passes once passes every time, on any machine. An agent can run the same spec through the `verify` tool without touching your world (`docs/GAMES.md`).

## Where to go next

- **Look at a bigger game**: `examples/orb_dash.lunaris.json` (3D, timer, win and lose banners, sound) and `examples/moon_hopper.lunaris.json` (2D pixel-art platformer with a tile map and sprite animation), both built by scripts like this one (`tools/build_*.py`) and played by bots (`tools/play_*.py`).
- **Make the ball fall and bounce**: remove `"gravity_scale": 0`, give the floor a `Collider` (`{"shape": 1, "size": [1, 1, 1]}`: a collider's size is multiplied by the `Transform` scale, so this is 40 by 1 by 40) and move the floor down to `y = -0.5` so its top is at 0; physics does the rest (`docs/DESIGN.md`).
- **More behaviour**: `docs/GRAPH.md` lists every operation (`atan2`, `pow`, `smoothstep`, `select`, `rand`, `spawn`, ...). A score that grows a coin's size, enemies that chase `player.x` / `player.z`, a timer: all of it is JSON.
- **Faster, or in another language**: `docs/PLUGINS.md` (your own compiled code inside the tick) and `docs/EMBEDDING.md` (C#, C, Python in-process).
- **An AI that plays**: `docs/MIND.md` lets a language model drive a body through the same tools.
