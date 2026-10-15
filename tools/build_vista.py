"""Write examples/vista.lunaris.json: an evening scene that shows what the GPU renderer's high-quality mode does. A grassy ground with a
cobbled path, a brick wall, wooden crates, metal and plastic spheres lit by a low sun, glowing lanterns, trees, fog, a procedural sky.

    python -I tools/build_vista.py

The scene is plain data (every component here is documented by `lunaris schema`); the sky, the shadows, the reflections and the glow
come from the `Environment` entity and the materials.
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def quat_from_to(a, b):
    """The rotation that turns the unit vector a into the unit vector b."""
    ax = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    s = math.sqrt(sum(c * c for c in ax))
    d = sum(x * y for x, y in zip(a, b))
    if s < 1e-9:
        return [0, 0, 0, 1] if d > 0 else [0, 1, 0, 0]
    half = math.atan2(s, d) / 2
    k = math.sin(half) / s
    return [round(ax[0] * k, 6), round(ax[1] * k, 6), round(ax[2] * k, 6), round(math.cos(half), 6)]


def norm(v):
    l = math.sqrt(sum(c * c for c in v))
    return [c / l for c in v]


def yaw(deg):
    h = math.radians(deg) / 2
    return [0, round(math.sin(h), 6), 0, round(math.cos(h), 6)]


def pitch(deg):
    h = math.radians(deg) / 2
    return [round(math.sin(h), 6), 0, 0, round(math.cos(h), 6)]


entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


def mesh(name, shape, pos, scale, color, rot=None, **material):
    t = {"pos": pos, "scale": scale}
    if rot:
        t["rot"] = rot
    alpha = material.pop("alpha", 1)
    add(name, Transform=t, MeshRenderer={"mesh": shape, "color": color + [alpha], **material})


# ---- the sky and the sun
to_sun = norm([0.78, 0.22, -0.42])
add("Environment", Environment={"sky": True, "sky_top": [0.13, 0.27, 0.62], "sky_horizon": [0.98, 0.66, 0.42], "ground": [0.24, 0.19, 0.15], "fog": 0.016, "fog_color": [0.97, 0.66, 0.44], "bloom": 0.45, "bloom_threshold": 1.2, "ssao": 1.0, "vignette": 0.28, "exposure": 1.0})
add("Sun", Transform={"rot": quat_from_to([0, 0, -1], [-c for c in to_sun])}, Light={"kind": 0, "color": [1.0, 0.72, 0.46], "intensity": 2.6})
add("Camera", Transform={"pos": [0.5, 2.1, 9.8], "rot": pitch(-6)}, Camera={"fov_y": 55, "clear": [0.5, 0.6, 0.8, 1]})

# ---- ground, path, wall
mesh("Ground", "plane", [0, 0, 0], [2400, 1, 2400], [1, 1, 1], texture="grass", uv_scale=[960, 960], roughness=0.95)
mesh("Path", "plane", [0, 0.012, -2], [3.2, 1, 40], [0.95, 0.93, 0.9], texture="stone", uv_scale=[20, 1.6], roughness=0.8)
mesh("Wall", "cube", [0, 1.7, -11], [18, 3.4, 0.8], [1, 1, 1], texture="bricks", uv_scale=[9, 2.1], roughness=0.85)
mesh("Wall cap", "cube", [0, 3.55, -11], [18.4, 0.3, 1.1], [0.62, 0.6, 0.57], texture="concrete", uv_scale=[9, 1], roughness=0.9)
for x in (-4.2, 4.2):
    mesh("Pillar", "cylinder", [x, 2.2, -9.6], [1.1, 4.4, 1.1], [1, 1, 1], texture="stone", uv_scale=[2, 2], roughness=0.8)
    mesh("Pillar top", "sphere", [x, 4.55, -9.6], [1.0, 1.0, 1.0], [0.85, 0.82, 0.78], roughness=0.5)

# ---- crates, spheres
mesh("Crate", "cube", [-5.0, 0.5, -3.0], [1, 1, 1], [1, 1, 1], rot=yaw(18), texture="wood", uv_scale=[1, 1], roughness=0.7)
mesh("Crate", "cube", [-3.9, 0.4, -2.2], [0.8, 0.8, 0.8], [1, 1, 1], rot=yaw(-25), texture="wood", uv_scale=[1, 1], roughness=0.7)
mesh("Crate", "cube", [-4.8, 1.5, -3.1], [0.9, 0.9, 0.9], [1, 1, 1], rot=yaw(40), texture="wood", uv_scale=[1, 1], roughness=0.7)
spheres = [
    ("Gold", [1.0, 0.77, 0.33], 1.0, 0.22),
    ("Chrome", [0.95, 0.96, 0.98], 1.0, 0.04),
    ("Red plastic", [0.8, 0.08, 0.06], 0.0, 0.22),
    ("Copper", [0.95, 0.52, 0.36], 1.0, 0.5),
    ("Clay", [0.88, 0.86, 0.8], 0.0, 0.85),
]
for i, (name, color, metallic, rough) in enumerate(spheres):
    x = -3.6 + i * 1.8
    mesh(name, "sphere", [x, 0.65, 2.2], [1.3, 1.3, 1.3], color, metallic=metallic, roughness=rough)

# ---- trees, rocks
for x, z, h in ((7.2, -4.0, 3.0), (-8.4, -5.5, 3.6), (9.5, -8.0, 2.6)):
    mesh("Trunk", "cylinder", [x, h / 2, z], [0.45, h, 0.45], [0.36, 0.23, 0.13], roughness=0.9)
    mesh("Canopy", "sphere", [x, h + 1.1, z], [3.2, 2.8, 3.2], [0.17, 0.42, 0.14], roughness=0.9)
    mesh("Canopy", "sphere", [x + 0.9, h + 0.4, z + 0.5], [2.2, 2.0, 2.2], [0.2, 0.46, 0.15], roughness=0.9)
for x, z, s in ((3.4, -3.2, 1.1), (4.2, -2.6, 0.6), (-1.9, -6.0, 0.8)):
    mesh("Rock", "sphere", [x, s * 0.3, z], [s * 1.3, s * 0.8, s], [0.8, 0.8, 0.8], texture="stone", uv_scale=[1, 1], roughness=0.85)

# ---- lanterns and a glowing orb
for x in (-1.8, 1.8):
    mesh("Lantern pole", "cylinder", [x, 0.9, 5.0], [0.1, 1.8, 0.1], [0.2, 0.2, 0.22], metallic=1.0, roughness=0.4, cast_shadow=False)
    mesh("Lantern", "sphere", [x, 1.95, 5.0], [0.34, 0.34, 0.34], [1.0, 0.78, 0.45], emissive=5.0)
    add("Lantern light", Transform={"pos": [x, 1.95, 5.0]}, Light={"kind": 1, "color": [1.0, 0.7, 0.38], "intensity": 3.0, "range": 7.0})
mesh("Glass", "sphere", [0.2, 0.7, 4.4], [1.1, 1.1, 1.1], [0.75, 0.92, 1.0], roughness=0.03, alpha=0.18)
mesh("Orb", "sphere", [1.5, 1.7, -2.2], [0.5, 0.5, 0.5], [0.4, 0.8, 1.0], emissive=6.0)
add("Orb light", Transform={"pos": [1.5, 1.7, -2.2]}, Light={"kind": 1, "color": [0.4, 0.75, 1.0], "intensity": 2.5, "range": 6.0})

path = os.path.join(ROOT, "examples", "vista.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump({"lunaris": 1, "entities": entities}, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities")
