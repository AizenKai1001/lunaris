"""Builds Wyrdwood's first island, "Shore" (examples/wyrdwood/shore.lunaris.json), only by calling the engine's tools, the way an AI agent would. The pictures come from `tools/make_wyrdwood_world.py`,
the models are the ones of Caldera (`examples/caldera/models/`, made by `tools/make_caldera_models.py`) until Wyrdwood has its own, the rules are Lunaris Script (`tools/wyrdwood_scripts.py`).

    python -I tools/make_wyrdwood_world.py            (once: the island's pictures)
    python3 tools/build_wyrdwood.py      (needs target/release/lunaris)

What you can do on it: walk, run, jump, roll, swim in the sea and the pond (Space rises, C dives), climb the ladder of the lookout tower (walk into it), pull yourself up a ledge (jump against it), swing the
axe at trees and rocks (click) and pick up what falls. The engine does the swimming, the roll, the ladder and the ledge (`CharacterMotion`); the day turns (`TimeOfDay`).
"""
import json
import math
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import wyrdwood_scripts as S  # noqa: E402

BIN = "target/release/lunaris"
D = "examples/wyrdwood"
W = f"{D}/world"
M = "examples/caldera/models"
FONT = "examples/fonts/pixel.ttf"
OUT_SCENE = f"{D}/shore.lunaris.json"
WORLD = json.load(open(f"{W}/world.json"))
SEA = WORLD["sea"]
RELIEF = WORLD["relief"]
SIZE = WORLD["size"]

proc = None
n_requests = 0
failed = 0


def start():
    global proc
    proc = subprocess.Popen([BIN, "serve", "--root", ".", "--no-gpu"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)


def call(name, args=None, quiet=False):
    """One tool call; returns the tool's own JSON answer (an empty dict, and a line on stdout, when it failed)."""
    global failed, n_requests
    n_requests += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n_requests, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n_requests:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        if quiet:
            return {}
        failed += 1
        print("request", n_requests, name, "failed:", json.dumps(m.get("error") or res)[:3000])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


def th(x, z):
    return call("terrain_height", {"x": x, "z": z}).get("height", 0.0)


def quat_y(deg):
    h = math.radians(deg) / 2
    return [0, math.sin(h), 0, math.cos(h)]


F = "f32"
COMPONENTS = {
    "Hero": [("state", F), ("t", F), ("cd", F), ("stamina", F), ("exhaust", F), ("regen", F), ("fx", F), ("fz", F), ("moving", F), ("sprint", F), ("air", F), ("swim", F), ("climb", F), ("rolling", F), ("run", F), ("sneak", F), ("tool", F), ("hurt", F)],
    "Choppable": [("kind", F), ("reach", F), ("shake", F)],
    "Pickup": [("kind", F), ("n", F), ("t", F)], "InvSlot": [("n", F)], "InvIcon": [("n", F)], "InvCount": [("n", F)], "InvPanel": [], "RecipeBtn": [("n", F)], "RecipeIcon": [("n", F)], "HudFood": [("n", F)], "ToastText": [], "Workbench": [], "Campfire": [], "HeldPart": [("item", F)],
    "Piece": [("kind", F), ("yaw", F), ("set", F)], "BuildAim": [], "BuildGhost": [], "TorchLight": [], "GhostPart": [("piece", F), ("ox", F), ("oy", F), ("oz", F), ("yaw", F)], "BuildText": [("n", F)],
    "HudHp": [], "HudSt": [], "HudItem": [], "Tree": [], "Prop": [], "SunTag": [],
}
LAYERS = "grass grass uv_scale=300 bump=0.02\nrock dirt uv_scale=240 tint=0.8,0.76,0.72 bump=0.06\nsand sand uv_scale=300 bump=0.01\nforest dirt uv_scale=200 tint=0.5,0.58,0.36 bump=0.03"
INPUT_MAP = "\n".join([
    "move_x = d, a:-1, gamepad.lx, arrowright, arrowleft:-1", "move_y = w, s:-1, gamepad.ly, arrowup, arrowdown:-1", "look_x = gamepad.rx", "look_y = gamepad.ry",
    "attack = mouse_left, j, gamepad.x, gamepad.rt", "roll = mouse_right, q, gamepad.b, gamepad.lt", "jump = space, gamepad.a", "dive = c, gamepad.rs", "sprint = shift, gamepad.ls, gamepad.lb", "sneak = control, gamepad.rs",
    "inventory = tab, gamepad.back", "hot1 = 1", "hot2 = 2", "hot3 = 3", "hot4 = 4", "hot5 = 5", "hot6 = 6", "hot7 = 7", "hot8 = 8",
    "interact = e, gamepad.y", "build_next = r, gamepad.rb", "pause = escape, gamepad.start", "deadzone = 0.15"])


def define_all():
    import wyrdwood_items as I
    for n, fields in COMPONENTS.items():
        call("define_component", {"name": n, "fields": [{"name": f, "type": t, "default": 0} for f, t in fields]})
    body = lambda key, n, extra=None: {"Pickup": {"kind": I.ID[key], "n": n}, **(extra or {})}  # noqa: E731
    drops = {"Collider": {"shape": 0, "size": [0.45, 0.45, 0.45]}, "RigidBody": {"mass": 1.0, "restitution": 0.25, "friction": 0.6}, "Velocity": {}, "AngularVelocity": {"damping": 0.5}}
    prefabs = {
        # what falls when a tree or a rock breaks: bodies that bounce and lie, picked up by walking over them
        "Log": ({**body("wood", 1), **drops, "MeshRenderer": {"mesh": "cylinder", "color": [0.42, 0.28, 0.16, 1], "roughness": 0.9}}, [0.22, 0.9, 0.22]),
        "Stone": ({**body("stone", 1), **drops, "MeshRenderer": {"mesh": "sphere", "color": [0.52, 0.52, 0.5, 1], "roughness": 0.95}}, [0.4, 0.3, 0.36]),
        "Flint": ({**body("flint", 1), **drops, "MeshRenderer": {"mesh": "sphere", "color": [0.25, 0.3, 0.36, 1], "roughness": 0.6}}, [0.26, 0.2, 0.3]),
        # what lies about on the ground to begin with: branches, stones, flint, berries, mushrooms (nothing to carry them away: no bodies)
        "Branch": (body("wood", 2, {"MeshRenderer": {"mesh": "cylinder", "color": [0.4, 0.28, 0.16, 1], "roughness": 0.95}}), [0.1, 0.7, 0.1]),
        "GroundStone": (body("stone", 1, {"MeshRenderer": {"mesh": "sphere", "color": [0.55, 0.55, 0.54, 1], "roughness": 0.95}}), [0.4, 0.28, 0.36]),
        "GroundFlint": (body("flint", 1, {"MeshRenderer": {"mesh": "sphere", "color": [0.27, 0.32, 0.38, 1], "roughness": 0.55}}), [0.24, 0.16, 0.28]),
        "BerryBush": (body("berries", 3, {"MeshRenderer": {"mesh": "sphere", "color": [0.22, 0.42, 0.2, 1], "roughness": 0.9}}), [0.9, 0.7, 0.9]),
        "MushroomPatch": (body("mushroom", 2, {"MeshRenderer": {"mesh": "sphere", "color": [0.82, 0.66, 0.5, 1], "roughness": 0.8}}), [0.3, 0.2, 0.3]),
    }
    for name, (comps, scale) in prefabs.items():
        call("define_prefab", {"name": name, "components": {**comps, "Transform": {"pos": [0, 0, 0], "scale": scale}}})
    import wyrdwood_build as B
    for name, d in B.prefabs().items():  # what the hammer builds
        call("define_prefab", {"name": name, **d})


def dress_world():
    spawn(1, "Environment", Environment={"sky": True, "sky_top": [0.17, 0.4, 0.82], "sky_horizon": [0.8, 0.86, 0.95], "ground": [0.3, 0.32, 0.3], "clouds": 0.5, "fog": 0.0008, "fog_color": [0.78, 0.84, 0.92],
                                         "bloom": 0.3, "bloom_threshold": 1.0, "ssao": 1.0, "exposure": 1.25, "vignette": 0.16, "shadows": True, "shadow_distance": 130, "saturation": 1.15, "ambient": 1.0})
    spawn(2, "Sun", Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "color": [1.0, 0.94, 0.84], "intensity": 3.0})
    spawn(3, "Time of day", TimeOfDay={"time": 8.0, "day_seconds": 600, "azimuth": 1.5, "sun_intensity": 3.1, "night_intensity": 0.25})
    spawn(5, "Input", InputMap={"bindings": INPUT_MAP})
    spawn(8, "Haze", HeightFog={"density": 0.008, "falloff": 0.06, "height": 14, "color": [0.8, 0.85, 0.92]})
    spawn(9, "Picture effects", PostFx={"shafts": 0.6, "contrast": 1.08})
    spawn(11, "Eyes", AutoExposure={"min": 0.6, "max": 1.45, "speed": 1.6, "target": 0.3})
    spawn(12, "Reflections", Ssr={"strength": 0.8, "max_distance": 36, "thickness": 0.6})
    spawn(13, "Lens", LensFlare={"intensity": 0.32, "ghosts": 3, "halo": 0.25})
    spawn(20, "Island", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": f"{W}/ground.png", "size": [SIZE, SIZE], "height": RELIEF, "step": 1, "color": [1, 1, 1, 1], "friction": 0.9},
          TerrainLayers={"splat": f"{W}/splat.png", "layers": LAYERS})
    spawn(21, "Water", Transform={"pos": [0, 0, 0]}, WaterBody={"heightmap": f"{W}/water.png", "size": [SIZE, SIZE], "relief": RELIEF, "color": [0.04, 0.3, 0.4, 0.86], "wave_height": 0.3, "wave_length": 10, "speed": 1.0})
    spawn(22, "Open sea", Transform={"pos": [0, SEA - 0.06, 0]}, Water={"size": [2600, 2600], "color": [0.04, 0.28, 0.4, 1.0], "wave_height": 0.3, "wave_length": 14, "speed": 0.9})
    # invisible walls far out at sea
    for i, (px, pz, sx, sz) in enumerate([(0, -156, 320, 4), (0, 156, 320, 4), (-156, 0, 4, 320), (156, 0, 4, 320)]):
        spawn(34 + i, f"Rough sea {i + 1}", Transform={"pos": [px, 20, pz], "scale": [sx, 60, sz]}, Collider={"shape": 1, "size": [1, 1, 1]})


def lookout(u, x, z):
    """A tower of timber with a ladder up its east side and a deck on top: something to climb. Built from boxes standing on the ground at (x, z)."""
    g = th(x, z)
    wood, dark = [0.46, 0.32, 0.2, 1], [0.34, 0.23, 0.14, 1]
    H = 5.0
    for sx, sz in ((-1.7, -1.7), (1.7, -1.7), (-1.7, 1.7), (1.7, 1.7)):
        spawn(u(), "Post", Prop={}, Transform={"pos": [x + sx, g + H / 2 - 0.4, z + sz], "scale": [0.34, H + 0.8, 0.34]}, MeshRenderer={"mesh": "cube", "color": wood, "roughness": 0.9}, Collider={"shape": 1, "size": [1, 1, 1]})
    spawn(u(), "Deck", Prop={}, Transform={"pos": [x, g + H + 0.1, z], "scale": [4.2, 0.24, 4.2]}, MeshRenderer={"mesh": "cube", "color": dark, "roughness": 0.9}, Collider={"shape": 1, "size": [1, 1, 1]})
    for k, (px, pz, sx, sz) in enumerate(((0, -2.0, 4.2, 0.14), (0, 2.0, 4.2, 0.14), (-2.0, 0, 0.14, 4.2))):
        spawn(u(), f"Rail {k + 1}", Prop={}, Transform={"pos": [x + px, g + H + 0.8, z + pz], "scale": [sx, 0.9, sz]}, MeshRenderer={"mesh": "cube", "color": wood, "roughness": 0.9}, Collider={"shape": 1, "size": [1, 1, 1]})
    # the ladder on the east face, 3.2 m beyond the posts' middle: its front is +x (turned 90 degrees about y)
    lx = x + 1.95
    spawn(u(), "Ladder", Prop={}, Transform={"pos": [lx, g, z], "rot": quat_y(90)}, Climbable={"height": H + 0.2, "width": 0.9, "reach": 0.9})
    for sz in (-0.38, 0.38):
        spawn(u(), "Ladder rail", Prop={}, Transform={"pos": [lx + 0.06, g + (H + 0.2) / 2, z + sz], "scale": [0.08, H + 0.2, 0.08]}, MeshRenderer={"mesh": "cube", "color": dark, "roughness": 0.9})
    for r in range(int((H + 0.2) / 0.4)):
        spawn(u(), "Rung", Prop={}, Transform={"pos": [lx + 0.06, g + 0.35 + r * 0.4, z], "scale": [0.06, 0.06, 0.8]}, MeshRenderer={"mesh": "cube", "color": wood, "roughness": 0.9})
    # a crate to jump up onto: a ledge a little too high to step over
    spawn(u(), "Crate", Prop={}, Transform={"pos": [x - 4.5, th(x - 4.5, z + 3.5) + 0.65, z + 3.5], "scale": [1.3, 1.3, 1.3]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.22, 1], "roughness": 0.85}, Collider={"shape": 1, "size": [1, 1, 1]})
    return g


def scatter_trees():
    def tree(model, size, hp, kind=1):
        return {"Tree": {}, "Choppable": {"kind": kind, "reach": 0.4}, "HitPoints": {"value": hp, "max": hp, "regen": 0}, "MeshRenderer": {"mesh": f"{M}/{model}.glb", "roughness": 0.9}, "Collider": {"shape": 1, "size": size}}
    calls = [
        (tree("pine", [0.7, 3.4, 0.7], 4), dict(count=330, layer="forest", layer_min=0.55, slope=[0, 26], height=[SEA + 3, 40], min_distance=5.0, scale=[0.85, 1.4], seed=11)),
        (tree("broadleaf", [0.8, 3.4, 0.8], 5), dict(count=220, layer="grass", layer_min=0.8, slope=[0, 20], height=[SEA + 3, 22], min_distance=6.5, scale=[0.8, 1.25], seed=12)),
        (tree("palm", [0.5, 3.0, 0.5], 3), dict(count=60, layer="sand", layer_min=0.7, slope=[0, 24], height=[SEA + 0.8, SEA + 4.5], min_distance=7.0, scale=[0.85, 1.3], seed=13)),
    ]
    for n in (1, 2, 3):
        calls.append((tree(f"rock{n}", [1.8, 1.4, 1.8], 5, kind=2), dict(count=[40, 34, 30][n - 1], layer="rock", layer_min=0.6, slope=[0, 50], min_distance=4.0, scale=[0.7, 2.2], seed=20 + n, yaw=True, sink=0.1)))
    calls.append((tree("rock2", [1.8, 1.4, 1.8], 4, kind=2), dict(count=24, layer="grass", layer_min=0.9, slope=[0, 25], height=[SEA + 3, 24], min_distance=14.0, scale=[0.5, 1.1], seed=30, yaw=True, sink=0.2)))
    placed = 0
    for comps, args in calls:
        placed += call("scatter", {"components": comps, **args}).get("placed", 0)
    print(f"  scatter: {placed} trees and rocks")
    return placed


def clear_around(spots):
    """Take away the scattered trees and rocks standing on the places of the game. `spots`: (x, z, radius)."""
    gone, offset = [], 0
    while True:
        q = call("query", {"with": ["Tree"], "select": ["Transform"], "limit": 500, "offset": offset})
        ents = q.get("entities") or []
        for e in ents:
            pos = ((e.get("components") or e).get("Transform") or {}).get("pos")
            if pos and any((pos[0] - sx) ** 2 + (pos[2] - sz) ** 2 < r * r for sx, sz, r in spots):
                gone.append(e["uid"])
        offset += len(ents)
        if len(ents) < 500:
            break
    for uid in gone:
        call("despawn", {"uid": uid})
    print(f"  {len(gone)} trees and rocks taken off the places of the game")


def hud():
    spawn(1002, "Health bar", HudHp={}, HudItem={}, Ui={"pos": [0.04, 0.04], "size": [0.2, 0.028], "color": [0.86, 0.3, 0.2, 1], "back_color": [0.2, 0.05, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [1, 0.9, 0.8, 0.7], "layer": 3})
    spawn(1004, "Stamina bar", HudSt={}, HudItem={}, Ui={"pos": [0.04, 0.078], "size": [0.2, 0.018], "color": [0.3, 0.78, 0.35, 1], "back_color": [0.05, 0.12, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [0.9, 1, 0.9, 0.6], "layer": 3})
    spawn(1006, "Counts", HudItem={}, Text={"value": "Wood {wood}    Stone {stone}", "pos": [0.04, 0.115], "size": 0.034, "align": 0, "color": [1, 0.94, 0.8, 1], "shadow": True}, TextFont={"file": FONT})
    spawn(1007, "Help", HudItem={}, Text={"value": "WASD walk   Shift run   Space jump / rise   C dive   RMB or Q roll   LMB swing the axe   walk into the ladder to climb", "pos": [0.04, 0.95], "size": 0.024, "align": 0, "color": [1, 1, 1, 0.75], "shadow": True},
          TextFont={"file": FONT})


def main():
    os.makedirs(D, exist_ok=True)
    start()
    call("reset")
    define_all()
    dress_world()
    # the hero, its camera, the lookout
    u_counter = [6000]

    def u():
        u_counter[0] += 1
        return u_counter[0] - 1

    land = WORLD["landing"]
    sx, sz = land["x"] - 3.0, land["z"]
    sy = th(sx, sz)
    tower_x, tower_z = land["x"] - 22.0, land["z"] - 14.0
    lookout(u, tower_x, tower_z)
    scatter_trees()
    clear_around([(sx, sz, 7.0), (tower_x, tower_z, 9.0), (tower_x - 4.5, tower_z + 3.5, 3.0)])
    spawn(7, "Camera", Transform={"pos": [sx, sy + 3.5, sz + 6.5]}, Camera={"fov_y": 62, "near": 0.15, "far": 1100},
          CameraRig={"target": 100, "mode": 1, "look_at": True, "look_offset": [0, 1.55, 0], "follow_speed": 16, "yaw": 1.2, "pitch": 0.3, "distance": 6.6, "shake_decay": 1.8, "shake_amount": 0.5}, CameraCollide={"radius": 0.4, "min_distance": 1.3})
    spawn(100, "Hero", Hero={"stamina": 100, "fx": -1, "fz": 0}, Transform={"pos": [sx, sy + 0.05, sz]}, CharacterBody={"radius": 0.38, "height": 1.8, "step_height": 0.55, "max_slope": 52},
          CharacterMotion={"swim_speed": 2.6, "roll_speed": 8.5, "roll_time": 0.62}, HitPoints={"value": 100, "max": 100, "regen": 0.8, "regen_delay": 9},
          MeshRenderer={"mesh": f"{M}/hero.glb", "roughness": 0.8}, Animator={"clip": "idle", "looping": True}, StateMachine={"machine": S.HERO_MACHINE})
    hud()
    # the rules
    res = call("define_script", {"source": S.ALL.replace("@SEA@", f"{SEA:.3f}")})
    print("define_script:", json.dumps(res)[:400])
    call("check")
    out = call("save_scene", {"path": OUT_SCENE})
    print("saved", OUT_SCENE, out if out else "")
    print("failed requests:", failed)
    proc.stdin.close()
    proc.wait(timeout=120)


if __name__ == "__main__":
    main()
