"""Write examples/first_person.lunaris.json: a first-person walk round a small hall, to show `MouseLook` (docs/GAMEPLAY.md). The camera is a child of the character and
looks with the mouse (`input.mouse_dx` / `mouse_dy`) or the right stick of a pad; the body turns with the yaw; a graph system walks "ahead" and "to the right" along the
`forward` and `right` that `MouseLook` writes; the resource `cursor.lock` makes `lunaris play` hide the pointer and hold it in the window.

    python3 tools/build_first_person.py      (it needs the `lunaris` binary: target/release/lunaris)
"""
import json
import subprocess

BIN = "target/release/lunaris"
reqs = []


def call(name, args):
    reqs.append({"name": name, "arguments": args})


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


call("define_component", {"name": "Hero", "fields": []})
call("define_component", {"name": "Prop", "fields": []})

spawn(1, "Environment", Environment={"sky": True, "sky_top": [0.25, 0.4, 0.75], "sky_horizon": [0.75, 0.82, 0.92], "ambient": 0.9, "ssao": 1.0, "bloom": 0.2, "fog": 0.004})
spawn(2, "Sun", Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "intensity": 2.6})
spawn(3, "Lamp", Transform={"pos": [0, 4.2, 0]}, Light={"kind": 1, "color": [1.0, 0.85, 0.6], "intensity": 3.0, "range": 12})
spawn(4, "Floor", Transform={"pos": [0, -0.5, 0], "scale": [26, 1, 26]}, MeshRenderer={"mesh": "cube", "texture": "stone", "uv_scale": [12, 12], "color": [0.62, 0.62, 0.66, 1], "roughness": 0.9},
      Collider={"shape": 1, "size": [26, 1, 26]})
for i, (pos, size) in enumerate([([0, 2.5, -13.25], [26, 5, 0.5]), ([0, 2.5, 13.25], [26, 5, 0.5]), ([-13.25, 2.5, 0], [0.5, 5, 26]), ([13.25, 2.5, 0], [0.5, 5, 26])]):
    spawn(5 + i, f"Wall {i + 1}", Transform={"pos": pos, "scale": size}, MeshRenderer={"mesh": "cube", "texture": "bricks", "uv_scale": [10, 2] if size[0] > size[2] else [2, 10], "color": [0.85, 0.75, 0.7, 1]},
          Collider={"shape": 1, "size": size})
for i, (x, z) in enumerate([(-6, -5), (6, -5), (-6, 4), (6, 4)]):
    spawn(10 + i, f"Pillar {i + 1}", Transform={"pos": [x, 2.0, z], "scale": [1.2, 4, 1.2]}, MeshRenderer={"mesh": "cube", "texture": "stone", "color": [0.78, 0.8, 0.86, 1]},
          Collider={"shape": 1, "size": [1.2, 4, 1.2]})
colors = [[0.9, 0.3, 0.25, 1], [0.25, 0.65, 0.9, 1], [0.95, 0.8, 0.25, 1], [0.35, 0.8, 0.45, 1]]
for i, (x, z) in enumerate([(-2.5, -8), (0, -9.5), (2.5, -8), (0, -6)]):
    spawn(20 + i, f"Crate {i + 1}", Prop={}, Transform={"pos": [x, 0.5, z]}, MeshRenderer={"mesh": "cube", "color": colors[i], "roughness": 0.5}, Collider={"shape": 1, "size": [1, 1, 1]})
spawn(30, "Ball", Prop={}, Transform={"pos": [0, 1.2, -2.5], "scale": [1.6, 1.6, 1.6]}, MeshRenderer={"mesh": "sphere", "color": [0.9, 0.9, 0.95, 1], "metallic": 0.9, "roughness": 0.15}, Collider={"shape": 0, "size": [1.6, 1.6, 1.6]})

# the walker and its eyes
spawn(40, "Player", Hero={}, Transform={"pos": [0, 0, 9]}, CharacterBody={"radius": 0.35, "height": 1.8})
spawn(41, "Eyes", Parent={"target": 40}, LocalTransform={"pos": [0, 1.62, 0]}, Transform={"pos": [0, 1.62, 9]}, Camera={"fov_y": 72}, MouseLook={"sensitivity": 0.0022})
spawn(42, "Input", InputMap={"bindings": "move_x = d, a:-1, gamepad.lx\nmove_y = w, s:-1, gamepad.ly\njump = space, gamepad.a\ndeadzone = 0.15"})
spawn(43, "Crosshair", Text={"value": "+", "pos": [0.5, 0.47], "size": 0.05, "align": 1, "color": [1, 1, 1, 0.85], "shadow": True})
spawn(44, "Hint", Text={"value": "WASD walk   mouse look   Space jump   Esc quit", "pos": [0.5, 0.94], "size": 0.03, "align": 1, "color": [1, 1, 1, 0.8], "shadow": True})

SCRIPT = """
/// Which way is ahead and to the right on the ground, from the look.
system look_axes each MouseLook order 9 {
    $look.fx = MouseLook.forward[0];
    $look.fz = MouseLook.forward[2];
    $look.rx = MouseLook.right[0];
    $look.rz = MouseLook.right[2];
}

/// The keys (or the pad) walk the body along them; Space jumps.
system walk each Hero, CharacterBody order 10 {
    CharacterBody.walk[0] = ($look.rx * $action.move_x + $look.fx * $action.move_y) * 5;
    CharacterBody.walk[1] = ($look.rz * $action.move_x + $look.fz * $action.move_y) * 5;
    if $action.jump.pressed > 0.5 {
        CharacterBody.jump = 5.5;
    }
}
"""
call("define_script", {"source": SCRIPT})
call("set_resource", {"name": "cursor.lock", "value": 1})
call("set_resource", {"name": "look.fz", "value": -1})
call("set_resource", {"name": "look.rx", "value": 1})
call("check", {})
call("save_scene", {"path": "examples/first_person.lunaris.json"})

lines = [json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": r}) for i, r in enumerate(reqs)]
p = subprocess.run([BIN, "serve", "--root", ".", "--no-gpu"], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=240)
bad = 0
for line in p.stdout.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        bad += 1
        print("request", m.get("id"), reqs[m["id"] - 1]["name"], "failed:", json.dumps(m.get("error") or res)[:400])
    elif reqs[m["id"] - 1]["name"] == "check":
        for c in res.get("content") or []:
            if c.get("type") == "text":
                print("check:", c["text"][:600])
print(f"wrote examples/first_person.lunaris.json ({len(reqs)} requests, {bad} failed)")
