"""Hollowlight, the water park: a prototype of the scale (examples/hollowlight/park.lunaris.json), built only by calling the engine's tools. A hall 900 m by 600 m under a roof 70 m up, a main basin 520 m by 280 m of
perfectly still water, slide towers 60 m tall, columns, girders, deck chairs and lifeguard chairs to show the size of a person against it. Surfaces use the CC0 Poly Haven sets of `tools/fetch_polyhaven.py` with `PbrMaps`.

    python3 tools/build_hollowlight_park.py      (needs target/release/lunaris and the textures)
"""
import json
import math
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
D = "examples/hollowlight"
T = f"{D}/textures"
OUT = sys.argv[1] if len(sys.argv) > 1 else f"{D}/park.lunaris.json"
NO_SAVE = OUT == "-"
SHOTS = sys.argv[2] if len(sys.argv) > 2 else "park"

proc = subprocess.Popen([BIN, "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n_req = 0
failed = 0


def call(name, args=None):
    global n_req, failed
    n_req += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n_req, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n_req:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        failed += 1
        print("request", n_req, name, "failed:", json.dumps(m.get("error") or res)[:800])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


uid = [100]


def spawn(name, **c):
    u = uid[0]
    uid[0] += 1
    call("spawn", {"uid": u, "components": {"Name": {"value": name}, **c}})
    return u


def tex(tid, scale, strength=1.0, color=(1, 1, 1), rough=None, ao=1.0):
    """The MeshRenderer texture fields and the PbrMaps of a Poly Haven set, as two dicts."""
    mr = {"texture": f"{T}/{tid}/{tid}_diff.jpg", "uv_scale": list(scale), "color": [*color, 1.0], "roughness": 0.6 if rough is None else rough}
    pbr = {"normal": f"{T}/{tid}/{tid}_nor_dx.jpg", "arm": f"{T}/{tid}/{tid}_arm.jpg", "normal_strength": strength, "ao_strength": ao}
    return mr, pbr


def box(name, pos, size, mr, pbr=None, collide=True, shadow=True, rot=None):
    tr = {"pos": list(pos), "scale": list(size)}
    if rot:
        tr["rot"] = rot
    comps = {"Transform": tr, "MeshRenderer": {"mesh": "cube", "cast_shadow": shadow, **mr}}
    if pbr:
        comps["PbrMaps"] = pbr
    if collide:
        comps["Collider"] = {"shape": 1, "size": [1.0, 1.0, 1.0]}  # a collider's size is multiplied by the entity's scale, which already holds the size
    return spawn(name, **comps)


def deck(name, pos, size, mr, pbr, tile_m):
    """A flat tiled surface: a `plane` (its picture runs once over it, so the repeat is worked out from its size), `tile_m` metres to a repeat of the picture."""
    m = dict(mr)
    m["uv_scale"] = [size[0] / tile_m, size[1] / tile_m]
    extra = {} if os.environ.get("NO_PBR") else {"PbrMaps": pbr}
    return spawn(name, Transform={"pos": list(pos), "scale": [size[0], 1.0, size[1]]}, MeshRenderer={"mesh": "plane", **m}, **extra)


def quat_axis(axis, ang):
    s = math.sin(ang / 2)
    return [axis[0] * s, axis[1] * s, axis[2] * s, math.cos(ang / 2)]


def quat_between_y(d):
    """The rotation that turns the cylinder's axis (y) into the unit direction d."""
    dx, dy, dz = d
    ax, ay, az = dz, 0.0, -dx  # cross(y, d)
    s = math.sqrt(ax * ax + az * az)
    if s < 1e-6:
        return [0, 0, 0, 1] if dy > 0 else [1, 0, 0, 0]
    ang = math.atan2(s, dy)
    return quat_axis((ax / s, ay / s, az / s), ang)


call("reset")
HX, HZ, ROOF = 450.0, 300.0, 70.0
BX0, BX1, BZ0, BZ1 = -260.0, 260.0, -170.0, 110.0  # the main basin
spawn("Environment", Environment={"sky": True, "sky_top": [0.42, 0.55, 0.7], "sky_horizon": [0.8, 0.85, 0.88], "ground": [0.35, 0.37, 0.36], "clouds": 0.9, "fog": 0.00012, "fog_color": [0.74, 0.82, 0.84],
                                  "bloom": 0.35, "bloom_threshold": 1.0, "ssao": 1.0, "exposure": 0.8, "vignette": 0.25, "shadows": not os.environ.get("NO_SHADOW"), "shadow_distance": 520, "saturation": 1.0, "ambient": 0.55})
spawn("Sun", Transform={"rot": [-0.55, 0.3, 0.12, 0.77]}, Light={"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 1.7})
spawn("Haze", HeightFog={"density": 0.0011, "falloff": 0.03, "height": 30, "color": [0.78, 0.86, 0.88]})
spawn("Picture effects", PostFx={"shafts": 0.7, "contrast": 1.06})
spawn("Eyes", AutoExposure={"min": 0.6, "max": 1.6, "speed": 1.5, "target": 0.3})
spawn("Reflections", Ssr={"strength": 0.9, "max_distance": 120, "thickness": 0.8})

# ---- the concourse: four slabs round the basin, 5 m thick, tile on top
mr, pbr = tex("long_white_tiles", (1, 1), color=(0.92, 0.95, 0.97), rough=0.25)
slabs = [("North slab", (-HX, HX), (-HZ, BZ0)), ("South slab", (-HX, HX), (BZ1, HZ)), ("West slab", (-HX, BX0), (BZ0, BZ1)), ("East slab", (BX1, HX), (BZ0, BZ1))]
for name, (x0, x1), (z0, z1) in slabs:
    sx, sz = x1 - x0, z1 - z0
    box(name, ((x0 + x1) / 2, -2.5, (z0 + z1) / 2), (sx, 5.0, sz), {"color": [0.55, 0.57, 0.58, 1.0], "roughness": 0.9})
    deck(f"{name} tiles", ((x0 + x1) / 2, 0.003, (z0 + z1) / 2), (sx, sz), mr, pbr, 3.0)
# the basin floor and its water
mr, pbr = tex("long_white_tiles", (1, 1), color=(0.62, 0.92, 1.0), rough=0.3)
box("Basin floor", ((BX0 + BX1) / 2, -4.5, (BZ0 + BZ1) / 2), (BX1 - BX0, 1.0, BZ1 - BZ0), {"color": [0.5, 0.6, 0.62, 1.0], "roughness": 0.9})
deck("Basin tiles", ((BX0 + BX1) / 2, -3.997, (BZ0 + BZ1) / 2), (BX1 - BX0, BZ1 - BZ0), mr, pbr, 3.0)
spawn("Basin water", Transform={"pos": [(BX0 + BX1) / 2, -0.5, (BZ0 + BZ1) / 2]}, Water={"size": [BX1 - BX0, BZ1 - BZ0], "color": [0.05, 0.45, 0.55, 0.6], "wave_height": 0.012, "wave_length": 30, "speed": 0.15})

# ---- the roof: a dark ceiling that lets the sun through (it casts no shadow), girders across the hall, bands of glass
mr, pbr = tex("ceiling_interior", (1, 1), color=(0.5, 0.52, 0.55), rough=0.8)
mr["uv_scale"] = [1.0, 2 * HZ / 8.0]
for k in range(-15, 15):
    box(f"Roof strip {k}", (k * 30.0, ROOF + 0.5, 0), (8.0, 1.0, 2 * HZ + 4), mr, pbr, collide=False, shadow=True)
steel = {"color": [0.32, 0.34, 0.38, 1.0], "roughness": 0.45, "metallic": 0.7}
for k in range(-14, 15):
    box(f"Girder {k}", (k * 30.0, ROOF - 3.0, 0), (1.6, 5.0, 2 * HZ), steel, collide=False, shadow=True)
# across the open bands a few cross beams, so the roof is a lattice and the light falls in blocks
for j in range(-9, 10):
    box(f"Cross beam {j}", (0, ROOF - 0.6, j * 32.0), (2 * HX, 0.9, 1.0), steel, collide=False, shadow=True)
# the end walls: concrete, and a high window band at the far end that glows
mr, pbr = tex("concrete_wall_003", (1, 1), rough=0.85)
mr["uv_scale"] = [2 * HX / 8.0, ROOF / 8.0]
box("North wall", (0, ROOF / 2 - 5, -HZ - 1.0), (2 * HX + 4, ROOF + 10, 2.0), mr, pbr)
box("South wall", (0, ROOF / 2 - 5, HZ + 1.0), (2 * HX + 4, ROOF + 10, 2.0), mr, pbr)
box("West wall", (-HX - 1.0, ROOF / 2 - 5, 0), (2.0, ROOF + 10, 2 * HZ), mr, pbr)
box("East wall", (HX + 1.0, ROOF / 2 - 5, 0), (2.0, ROOF + 10, 2 * HZ), mr, pbr)
box("Far window", (0, 42.0, -HZ + 0.4), (700.0, 26.0, 0.4), {"color": [1.0, 0.96, 0.86, 1.0], "emissive": 2.2}, collide=False, shadow=False)

# ---- columns round the concourse
mr, pbr = tex("rebar_reinforced_concrete", (1, 1), rough=0.8)
mr["uv_scale"] = [4, 14]
for i in range(-12, 13):
    for z in (-HZ + 12.0, HZ - 12.0):
        spawn(f"Column {i}", Transform={"pos": [i * 36.0, ROOF / 2 - 2.0, z], "scale": [3.0, ROOF + 4.0, 3.0]}, MeshRenderer={"mesh": "cylinder", **mr}, PbrMaps=pbr, Collider={"shape": 1, "size": [1.0, 1.0, 1.0]})

# ---- slide towers: a concrete shaft with a spiral tube round it and posts under it
ty = {"color": [0.95, 0.78, 0.12, 1.0], "roughness": 0.22}
tb = {"color": [0.15, 0.45, 0.9, 1.0], "roughness": 0.22}
for t, (cx, cz, h, mat) in enumerate(((-330.0, -240.0, 60.0, ty), (-290.0, -255.0, 48.0, tb), (330.0, -235.0, 56.0, tb))):
    box(f"Tower {t}", (cx, h / 2, cz), (9.0, h, 9.0), {"texture": f"{T}/rebar_reinforced_concrete/rebar_reinforced_concrete_diff.jpg", "uv_scale": [2, 12], "roughness": 0.8}, {"normal": f"{T}/rebar_reinforced_concrete/rebar_reinforced_concrete_nor_dx.jpg", "arm": f"{T}/rebar_reinforced_concrete/rebar_reinforced_concrete_arm.jpg"})
    box(f"Tower {t} platform", (cx, h + 1.0, cz), (16.0, 2.0, 16.0), {"color": [0.8, 0.8, 0.82, 1.0], "roughness": 0.5})
    rad, steps = 22.0, 84
    pts = []
    for k in range(steps + 1):
        a = k * 2 * math.pi / 36 + t
        pts.append((cx + math.cos(a) * rad * (1.0 - 0.2 * k / steps), h - (h - 4.0) * k / steps, cz + math.sin(a) * rad * (1.0 - 0.2 * k / steps)))
    for k in range(steps):
        (x0, y0, z0), (x1, y1, z1) = pts[k], pts[k + 1]
        d = (x1 - x0, y1 - y0, z1 - z0)
        ln = math.sqrt(sum(c * c for c in d))
        u = (d[0] / ln, d[1] / ln, d[2] / ln)
        spawn(f"Slide {t} {k}", Transform={"pos": [(x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2], "rot": quat_between_y(u), "scale": [3.2, ln * 1.04, 3.2]}, MeshRenderer={"mesh": "cylinder", "cast_shadow": True, **mat})
        if k % 6 == 0:
            spawn(f"Post {t} {k}", Transform={"pos": [x0, y0 / 2 - 0.5, z0], "scale": [0.6, y0, 0.6]}, MeshRenderer={"mesh": "cylinder", "color": [0.7, 0.72, 0.75, 1.0], "roughness": 0.4, "metallic": 0.6})

# ---- people-sized things: lifeguard chairs and deck chairs along the basin's north edge, a ladder into the water
white = {"color": [0.92, 0.93, 0.95, 1.0], "roughness": 0.45}
for i in range(-6, 7):
    x = i * 70.0
    box(f"Lifeguard seat {i}", (x, 2.6, BZ0 - 3.0), (1.4, 0.2, 1.4), white)
    for dx in (-0.6, 0.6):
        for dz in (-0.6, 0.6):
            box(f"Lifeguard leg {i}", (x + dx, 1.3, BZ0 - 3.0 + dz), (0.12, 2.6, 0.12), white, collide=False)
    box(f"Lifeguard back {i}", (x, 3.3, BZ0 - 3.7), (1.4, 1.2, 0.1), {"color": [0.95, 0.35, 0.1, 1.0], "roughness": 0.5}, collide=False)
for row in range(3):
    for i in range(-30, 31):
        x = i * 6.5 + 1.5
        box(f"Deck chair {row} {i}", (x, 0.25, BZ0 - 14.0 - row * 3.2), (0.7, 0.5, 1.9), {"color": [0.25 + 0.5 * ((i + row) % 3 == 0), 0.6, 0.85, 1.0], "roughness": 0.6}, collide=False)

# ---- the player: a body with first-person eyes, a flashlight and the rules to walk (Lunaris Script)
call("define_component", {"name": "Player", "fields": [{"name": "stamina", "type": "f32", "default": 100}, {"name": "tired", "type": "f32", "default": 0}]})
call("define_component", {"name": "Flashlight", "fields": []})
KEYS = ["move_x = d, a:-1, gamepad.lx", "move_y = w, s:-1, gamepad.ly", "jump = space, gamepad.a", "sprint = shift, gamepad.ls", "sneak = control, gamepad.rs", "flash = f, gamepad.y", "deadzone = 0.15"]
call("spawn", {"uid": 50, "components": {"Name": {"value": "Input"}, "InputMap": {"bindings": chr(10).join(KEYS)}}})
body = 51
call("spawn", {"uid": body, "components": {"Name": {"value": "Player"}, "Player": {}, "Transform": {"pos": [0.0, 0.1, HZ - 30.0]}, "CharacterBody": {"radius": 0.35, "height": 1.8, "step_height": 0.4, "max_slope": 50}}})
call("spawn", {"uid": 52, "components": {"Name": {"value": "Eyes"}, "Parent": {"target": body}, "LocalTransform": {"pos": [0, 1.62, 0]}, "Transform": {"pos": [0.0, 1.72, HZ - 30.0]},
                                          "Camera": {"fov_y": 72, "near": 0.1, "far": 4000, "active": True}, "MouseLook": {"sensitivity": 0.0022, "pitch_min": -1.45, "pitch_max": 1.45}}})
call("spawn", {"uid": 53, "components": {"Name": {"value": "Flashlight"}, "Flashlight": {}, "Parent": {"target": 52}, "LocalTransform": {"pos": [0.15, -0.1, -0.2]}, "Transform": {"pos": [0.15, 1.6, HZ - 30.2]},
                                          "Light": {"kind": 2, "color": [1.0, 0.95, 0.82], "intensity": 9.0, "range": 60.0, "angle": 26.0, "softness": 0.45}}})
call("set_resource", {"name": "flash.on", "value": 1})
call("set_resource", {"name": "cursor.lock", "value": 1})
SCRIPT = """
/// Which way is ahead and to the right on the ground, from the look.
system fp_axes each MouseLook order 9 {
    $look.fx = MouseLook.forward[0];
    $look.fz = MouseLook.forward[2];
    $look.rx = MouseLook.right[0];
    $look.rz = MouseLook.right[2];
}

/// Walk, sprint (stamina), creep, jump; F is the flashlight.
system fp_walk each Player, CharacterBody order 10 {
    let want = $action.sprint > 0.5 && Player.tired < 0.5 && (abs($action.move_x) + abs($action.move_y)) > 0.1;
    let speed = select(want, 6.4, select($action.sneak > 0.5, 1.8, 3.4));
    CharacterBody.walk[0] = ($look.rx * $action.move_x + $look.fx * $action.move_y) * speed;
    CharacterBody.walk[1] = ($look.rz * $action.move_x + $look.fz * $action.move_y) * speed;
    if want {
        Player.stamina = max(Player.stamina - 14 * dt, 0);
    } else {
        Player.stamina = min(Player.stamina + 9 * dt, 100);
    }
    Player.tired = select(Player.stamina <= 0, 1, select(Player.stamina > 35, 0, Player.tired));
    if $action.jump.pressed > 0.5 && CharacterBody.grounded {
        CharacterBody.jump = 4.6;
    }
    if $action.flash.pressed > 0.5 {
        $flash.on = 1 - $flash.on;
    }
    $stamina = Player.stamina;
}

system flashlight each Flashlight, Light order 11 {
    Light.intensity = select($flash.on > 0.5, 9.0 + 0.25 * sin(tick * 0.9), 0);
}
"""
r = call("define_script", {"source": SCRIPT})
print("script:", json.dumps(r)[:160])
call("step", {"ticks": 3})
r = call("check")
print("check:", json.dumps({k: r.get(k) for k in ("errors", "warnings")}))
for name, eye, target in (("entry", [0.0, 1.7, HZ - 34.0], [0.0, 25.0, -200.0]), ("edge", [-60.0, 1.7, BZ1 + 6.0], [60.0, 14.0, BZ0 - 80.0]), ("tower", [-250.0, 2.0, -200.0], [-330.0, 32.0, -240.0]), ("deck", [-60.0, 4.0, BZ1 + 12.0], [-60.0, 0.0, BZ1 + 5.0])):
    call("render", {"quality": "high", "width": 1600, "height": 900, "eye": eye, "target": target, "fov": 70, "path": f"{SHOTS}_{name}.png", "image": False})
saved = {} if NO_SAVE else call("save_scene", {"path": OUT})
print("saved", OUT, saved, "failed requests:", failed)
proc.stdin.close()
proc.wait(timeout=300)
