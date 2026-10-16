"""Building with the hammer in Wyrdwood: what can be built, the ghost that shows where, the rules of placing and taking back, and the stations (a workbench within reach opens the recipes that need one).

The hammer in the hand (item class 3) turns the camera's centre line into a ghost: a ray from the hero's eye along the camera looks at the ground (`Probe`), and the ghost stands where it lands, green where the piece can
go (flat ground, dry, within reach, not on another piece, the wood in the pack) and red where it cannot. The mouse wheel turns the piece, R takes the next piece, the left button builds, E takes back the piece looked at.

A piece is a prefab (a box collider and a few parts, the root standing in the middle of its box) with a `Piece` component; a ghost is the same parts as `GhostPart`s that follow the ghost's place. Everything that is
the same for every piece is one table here, PIECES, from which the prefabs, the ghosts, the scripts and the list on the screen are made.

Systems, in the order they run:
  3  station_reset   the stations near the hero are counted again every tick: `$station.wb` (a workbench within 6 m), `$station.fire` (a fire within 5 m)
  4  station_bench / station_fire
  11 build_ghost     the probe at the ghost's place (is it in the water?)
  12 build_aim       the ray from the hero's eye, what it hit, the held hammer, the wheel and R
  13 piece_tick      turns a new piece to the angle it was built at, notes the piece looked at, takes it back on E
  14 build_decide    can it go here, and if the button was pressed, build it
  23 ghost_parts     the ghost's parts follow and are coloured
  31 build_text      the list of pieces on the screen
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import wyrdwood_items as I  # noqa: E402

WOOD = [0.5, 0.35, 0.2, 1]
DARK = [0.36, 0.25, 0.15, 1]
IRON = [0.5, 0.52, 0.56, 1]
STONE = [0.5, 0.5, 0.48, 1]
FLAME = [1.0, 0.5, 0.1, 1]
CORE = [1.0, 0.86, 0.35, 1]

# A part: (mesh, scale, offset from the middle of the piece's foot, colour, emissive, turned about up by this many degrees)
BENCH = [
    ("cube", [1.7, 0.1, 0.85], [0, 0.95, 0], WOOD, 0, 0),
    ("cube", [1.5, 0.05, 0.65], [0, 0.28, 0], DARK, 0, 0),
    ("cube", [1.7, 0.42, 0.06], [0, 1.2, -0.4], DARK, 0, 0),
    ("cube", [0.28, 0.22, 0.22], [-0.55, 1.1, 0.05], IRON, 0, 0),
    ("cube", [0.6, 0.05, 0.16], [0.3, 1.03, 0.12], [0.64, 0.48, 0.3, 1], 0, 0),
] + [("cube", [0.12, 0.9, 0.12], [sx * 0.72, 0.45, sz * 0.32], DARK, 0, 0) for sx in (-1, 1) for sz in (-1, 1)]
FIRE = (
    [("sphere", [0.3, 0.22, 0.3], [math.cos(k * math.pi / 4) * 0.5, 0.11, math.sin(k * math.pi / 4) * 0.5], STONE, 0, 0) for k in range(8)]
    + [("cube", [0.9, 0.12, 0.12], [0, 0.2, 0], DARK, 0, a) for a in (0, 60, 120)]
    + [("sphere", [0.34, 0.6, 0.34], [0, 0.52, 0], FLAME, 2.5, 0), ("sphere", [0.2, 0.42, 0.2], [0, 0.46, 0], CORE, 4.0, 0)]
)

# key, name, cost [(item key, how many)], the collider's box (it stands on the ground), parts
PIECES = [
    ("workbench", "Workbench", [("wood", 10)], [1.7, 1.0, 0.9], BENCH),
    ("campfire", "Campfire", [("wood", 5)], [0.95, 0.5, 0.95], FIRE),
]
MARK = {"workbench": "Workbench", "campfire": "Campfire"}  # the marker component of each piece, which the stations are found by
GHOST0 = 3210
TEXT0 = 3300


def quat_y(deg):
    h = math.radians(deg) / 2
    return [0, math.sin(h), 0, math.cos(h)]


def prefabs():
    """`define_prefab` calls: name -> {components, children}."""
    out = {}
    for n, (key, name, cost, box, parts) in enumerate(PIECES):
        half = box[1] / 2
        kids = []
        for k, (mesh, scale, off, color, emis, yaw) in enumerate(parts):
            kids.append({"name": f"{name} part {k + 1}", "components": {"LocalTransform": {"pos": [off[0], off[1] - half, off[2]], "rot": quat_y(yaw), "scale": scale},
                                                                         "MeshRenderer": {"mesh": mesh, "color": color, "roughness": 0.9, "emissive": emis}}})
        comps = {"Name": {"value": name}, "Transform": {"pos": [0, 0, 0], "scale": [1, 1, 1]}, "Piece": {"kind": n + 1}, MARK[key]: {}, "Collider": {"shape": 1, "size": box}}
        if key == "campfire":
            comps["Light"] = {"kind": 1, "color": [1.0, 0.6, 0.26], "intensity": 3.4, "range": 17}
            kids.append({"name": "Flames", "components": {"LocalTransform": {"pos": [0, 0.35 - half + 0.25, 0]}, "Particles": {
                "count": 70, "lifetime": 1.0, "shape": 3, "extent": [0.16, 0, 0.16], "direction": [0, 1, 0], "cone": 14, "speed_range": [0.5, 1.3], "gravity": [0, 0.9, 0], "size": [0.2, 0.03],
                "color": [1.0, 0.72, 0.25, 0.9], "color_end": [1.0, 0.15, 0.0, 0.0], "additive": True, "seed": 3}}})
            kids.append({"name": "Smoke", "components": {"LocalTransform": {"pos": [0, 0.9 - half, 0]}, "Particles": {
                "count": 28, "lifetime": 3.2, "shape": 3, "extent": [0.1, 0, 0.1], "direction": [0, 1, 0], "cone": 16, "speed_range": [0.4, 0.9], "gravity": [0, 0.35, 0], "turbulence": 0.5, "size": [0.18, 0.7],
                "color": [0.55, 0.55, 0.55, 0.28], "color_end": [0.4, 0.4, 0.4, 0.0], "additive": False, "seed": 5}}})
        out[name] = {"components": comps, "children": kids}
    return out


def ghosts(spawn):
    """The aim probe, the ghost's probe and the ghost's parts, spawned with `spawn(uid, name, **components)`."""
    spawn(3200, "Build aim", BuildAim={}, Transform={}, Probe={"up": 0.1, "depth": 1.0, "ray_from": [0, 1.55, 0], "ray_dir": [0, -0.3, -1], "ray_max": 0.0})
    spawn(3202, "Torch light", TorchLight={}, Transform={}, Light={"kind": 1, "color": [1.0, 0.62, 0.3], "intensity": 0.0, "range": 13.0})
    spawn(3201, "Build ghost probe", BuildGhost={}, Transform={}, Probe={"up": 3.0, "depth": 40.0})
    uid = GHOST0
    for n, (key, name, cost, box, parts) in enumerate(PIECES):
        for mesh, scale, off, color, emis, yaw in parts:
            spawn(uid, f"Ghost {name}", GhostPart={"piece": n, "ox": off[0], "oy": off[1], "oz": off[2], "yaw": math.radians(yaw)}, Transform={"scale": scale},
                  MeshRenderer={"mesh": mesh, "color": [0.4, 0.95, 0.45, 0.55], "roughness": 0.9, "visible": False, "cast_shadow": False, "emissive": 0.5})
            uid += 1


def texts(spawn, font):
    spawn(TEXT0, "Build help", BuildText={"n": -1}, Text={"value": "Hammer   wheel: turn   R: next piece   left click: build   E: take back", "pos": [0.5, 0.79], "size": 0.022, "align": 1, "color": [1, 1, 1, 0.8], "shadow": True, "visible": False}, TextFont=font)
    for n, (key, name, cost, box, parts) in enumerate(PIECES):
        need = ", ".join(f"{dict((it[0], it[1]) for it in I.ITEMS)[k].lower()} {c}" for k, c in cost)
        spawn(TEXT0 + 1 + n, f"Build entry {n}", BuildText={"n": n}, Text={"value": f"{name}   ({need})", "pos": [0.5, 0.83 + 0.036 * n], "size": 0.028, "align": 1, "color": [1, 1, 1, 1], "shadow": True, "visible": False}, TextFont=font)


def have_ok(cost):
    return " && ".join(f"$have.{k} >= {c}" for k, c in cost)


def chain(var, values, default="0"):
    expr = default
    for n in range(len(values) - 1, -1, -1):
        expr = f"select({var} == {n}, {values[n]}, {expr})"
    return expr


def script():
    npieces = len(PIECES)
    s = []
    s.append("""/// Stations: counted again every tick (the reset first, then each station that is near says so).
system station_reset each Environment order 3 {
    $station.wb = 0;
    $station.fire = 0;
}

system station_bench each Workbench, Transform order 4 {
    if hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z) < 6 {
        $station.wb = 1;
    }
}

system station_fire each Campfire, Transform order 4 {
    if hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z) < 5 {
        $station.fire = 1;
    }
}

/// A fire flickers.
system fire_flicker each Campfire, Light order 24 {
    Light.intensity = 3.4 + 0.55 * sin(tick * 0.33 + uid) + 0.3 * sin(tick * 0.91 + uid * 2) + 0.15 * sin(tick * 2.3);
}

/// A torch in the hand lights what is round the hero (a point light that follows it; dark when no torch is held).
system torch_light each TorchLight, Transform, Light order 24 {
    Transform.pos[0] = $hero.x + $hero.fx * 0.45;
    Transform.pos[1] = $hero.y + 1.55;
    Transform.pos[2] = $hero.z + $hero.fz * 0.45;
    Light.intensity = select($held.class == 5, 2.6 + 0.4 * sin(tick * 0.37) + 0.25 * sin(tick * 1.13), 0);
}

/// The place the ghost stands at: is there water over it?
system build_ghost each BuildGhost, Transform, Probe order 11 {
    Transform.pos[0] = $build.x;
    Transform.pos[1] = $build.y + 0.5;
    Transform.pos[2] = $build.z;
    $build.wet = select(Probe.wet && Probe.surface > Probe.ground - 0.05, 1, 0);
}
""")
    s.append(f"""/// The ray from the hero's eye along the way the camera looks: what it hits is where the ghost goes. The wheel turns the piece and R takes the next one.
system build_aim each BuildAim, Transform, Probe order 12 {{
    let busy = $invopen > 0 || $paused > 0;
    let armed = $held.class == 3 && !busy && $hero.swim < 0.5;
    Transform.pos[0] = $hero.x;
    Transform.pos[1] = $hero.y;
    Transform.pos[2] = $hero.z;
    let cp = cos($cam.pitch);
    Probe.ray_dir[0] = 0 - sin($cam.yaw) * cp;
    Probe.ray_dir[1] = 0 - sin($cam.pitch);
    Probe.ray_dir[2] = 0 - cos($cam.yaw) * cp;
    Probe.ray_max = select(armed, 14, 0);
    $build.on = select(armed, 1, 0);
    $build.has = select(Probe.hit, 1, 0);
    $build.x = Probe.hit_point[0];
    $build.y = Probe.hit_point[1];
    $build.z = Probe.hit_point[2];
    $build.flat = Probe.hit_normal[1];
    $build.near = hypot(Probe.hit_point[0] - $hero.x, Probe.hit_point[2] - $hero.z);
    $build.hit = select(Probe.hit, Probe.hit_uid, 0);
    $build.hp = 0;
    $build.yaw = $build.yaw + $input.mouse_wheel * 0.3927 * select(armed, 1, 0);
    if armed && $action.build_next.pressed > 0.5 {{
        $build.sel = select($build.sel + 1 >= {npieces}, 0, $build.sel + 1);
    }}
}}

/// A piece that was just built turns to the angle it was built at; the piece looked at is noted; E takes it back (what it cost comes back).
system piece_tick each Piece, Transform order 13 {{
    if Piece.set < 0.5 {{
        Transform.rot[1] = sin(Piece.yaw * 0.5);
        Transform.rot[3] = cos(Piece.yaw * 0.5);
        Piece.set = 1;
    }}
    if $build.on > 0.5 && $build.hit == uid && Piece.set < 1.5 {{
        $build.hp = 1;
        if $action.interact.pressed > 0.5 && $give.n == 0 {{
            $give.id = {chain("Piece.kind - 1", [I.ID[c[2][0][0]] for c in PIECES])};
            $give.n = {chain("Piece.kind - 1", [c[2][0][1] for c in PIECES])};
            emit "unbuild";
            // the parts go with it: a `Lifetime` removes an entity and everything attached to it (a plain `despawn` leaves the parts)
            Piece.set = 2;
            remove Collider;
            add Lifetime {{ seconds: 0.03 }};
        }}
    }}
}}
""")
    # what each piece costs, and whether it can be paid for
    d = ["/// Can the piece in hand go here: flat, dry, in reach, free of other pieces, paid for. If the button was pressed, it is built (the wood is asked for, and the hero makes the building gesture)."]
    d.append("system build_decide each Hero order 14 {")
    for n, (key, name, cost, box, parts) in enumerate(PIECES):
        d.append(f"    let pay{n} = {have_ok(cost)};")
        d.append(f"    $afford.{n} = select(pay{n}, 1, 0);")
    d.append(f"    let paid = {chain('$build.sel', [f'select(pay{n}, 1, 0)' for n in range(npieces)])} > 0.5;")
    d.append("    let ok = $build.on > 0.5 && $build.has > 0.5 && $build.flat > 0.8 && $build.near < 8.2 && $build.near > 0.9 && $build.y > @SEA@ + 0.35 && $build.wet < 0.5 && $build.hp < 0.5 && paid;")
    d.append("    $build.ok = select(ok, 1, 0);")
    d.append("    if ok && $action.attack.pressed > 0.5 && Hero.state == 0 && $rm.0.n == 0 && $give.n == 0 {")
    for n, (key, name, cost, box, parts) in enumerate(PIECES):
        d.append(f"        if $build.sel == {n} {{")
        d.append(f"            spawn_prefab \"{name}\" at ($build.x, $build.y + {box[1] / 2:.3f}, $build.z) with {{ Piece {{ yaw: $build.yaw }} }};")
        for k, (item, count) in enumerate(cost):
            d.append(f"            $rm.{k}.id = {I.ID[item]};")
            d.append(f"            $rm.{k}.n = {count};")
        d.append("        }")
    d.append("        Hero.state = 5;")
    d.append("        Hero.t = 0;")
    d.append("        emit \"build\";")
    d.append("    }")
    d.append("}")
    s.append("\n".join(d) + "\n")
    s.append("""/// The ghost's parts follow the ghost and are green (it can go) or red (it cannot).
system ghost_parts each GhostPart, Transform, MeshRenderer order 23 {
    MeshRenderer.visible = $build.on > 0.5 && $build.sel == GhostPart.piece && $build.hp < 0.5;
    let c = cos($build.yaw);
    let s = sin($build.yaw);
    Transform.pos[0] = $build.x + c * GhostPart.ox + s * GhostPart.oz;
    Transform.pos[1] = $build.y + GhostPart.oy;
    Transform.pos[2] = $build.z - s * GhostPart.ox + c * GhostPart.oz;
    let a = ($build.yaw + GhostPart.yaw) * 0.5;
    Transform.rot[1] = sin(a);
    Transform.rot[3] = cos(a);
    let good = $build.ok > 0.5;
    MeshRenderer.color[0] = select(good, 0.3, 0.95);
    MeshRenderer.color[1] = select(good, 0.95, 0.25);
    MeshRenderer.color[2] = select(good, 0.4, 0.22);
    MeshRenderer.color[3] = 0.55;
}
""")
    t = ["/// The list of what can be built: the one picked is gold, the ones that cannot be paid for are red."]
    t.append("system build_text each BuildText, Text order 31 {")
    t.append("    Text.visible = $build.on > 0.5;")
    t.append("    if BuildText.n >= 0 {")
    t.append("        let sel = select($build.sel == BuildText.n, 1, 0);")
    t.append(f"        let pay = {chain('BuildText.n', [f'$afford.{n}' for n in range(npieces)])};")
    t.append("        Text.color[0] = select(pay > 0.5, select(sel > 0.5, 1.0, 0.85), 0.9);")
    t.append("        Text.color[1] = select(pay > 0.5, select(sel > 0.5, 0.82, 0.85), 0.35);")
    t.append("        Text.color[2] = select(pay > 0.5, select(sel > 0.5, 0.35, 0.85), 0.3);")
    t.append("        Text.color[3] = select(sel > 0.5, 1.0, 0.75);")
    t.append("    }")
    t.append("}")
    s.append("\n".join(t) + "\n")
    return "\n".join(s)


SCRIPT = script()
