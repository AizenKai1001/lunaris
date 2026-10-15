"""Write examples/echo_tour.lunaris.json: a walk through four places that sound different, to hear (and measure) what tracing sound through the
scene does. Everything is boxes with colliders; `AcousticMaterial` says how each surface treats sound.

    x = 0..4     a tiled bathroom (hard: the sound rings), a dripping tap
    x = 4..12    a carpeted living room (soft: dry), and a shut door in its south wall: the radio behind it is muffled, except right
                 at the door
    x = 12..36   a concrete hall (huge: a long tail), handclaps
    x = 36..     the courtyard under open sky (no reverberation at all), handclaps in the distance

The camera, which is also the listener, walks along x at 3 m/s through doorways along the way (about 17 seconds). `lunaris play` lets you
hear it; `lunaris call record_audio '{"ticks":900,"path":"tour.wav"}'` writes it down and reports the levels, and
`lunaris call acoustics '{"listener":[24,1.6,0]}'` says what the hall does without anyone listening.

    python -I tools/make_sounds.py     (once: the sound files)
    python -I tools/build_echo.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []
T = 0.2  # wall thickness


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


def box(name, lo, hi, color, absorption, transmission, rough=0.8, texture=None):
    """An axis-aligned box from corner `lo` to corner `hi`: visible, solid, and with its own acoustics."""
    size = [hi[i] - lo[i] for i in range(3)]
    center = [(lo[i] + hi[i]) / 2 for i in range(3)]
    mesh = {"mesh": "cube", "color": color + [1], "roughness": rough}
    if texture:
        mesh.update({"texture": texture, "uv_scale": [max(1, size[0] / 2), max(1, size[2] / 2)]})
    add(name, Transform={"pos": center, "scale": size}, MeshRenderer=mesh, Collider={"shape": 1, "size": [1, 1, 1]}, AcousticMaterial={"absorption": absorption, "transmission": transmission})


def room(name, x0, x1, z0, z1, h, color, absorption, transmission=0.03, texture=None, skip=()):
    """Floor, ceiling and the two long walls of a room; the two end walls are made by the caller (they have doorways)."""
    if "floor" not in skip:
        box(f"{name} floor", [x0, -T, z0 - T], [x1, 0, z1 + T], color, absorption, transmission, texture=texture)
    if "ceiling" not in skip:
        box(f"{name} ceiling", [x0, h, z0 - T], [x1, h + T, z1 + T], color, absorption, transmission)
    if "south wall" not in skip:
        box(f"{name} south wall", [x0, 0, z0 - T], [x1, h, z0], color, absorption, transmission)
    box(f"{name} north wall", [x0, 0, z1], [x1, h, z1 + T], color, absorption, transmission)


def divider(name, x, z0, z1, h, door_half, door_h, color, absorption, transmission=0.03):
    """A wall across the tour at x .. x + T with a doorway on the line z = 0."""
    box(f"{name} (south part)", [x, 0, z0], [x + T, h, -door_half], color, absorption, transmission)
    box(f"{name} (north part)", [x, 0, door_half], [x + T, h, z1], color, absorption, transmission)
    box(f"{name} (lintel)", [x, door_h, -door_half], [x + T, h, door_half], color, absorption, transmission)


def lamp(name, pos, color, intensity, rng):
    add(name, Transform={"pos": pos}, Light={"kind": 1, "color": color, "intensity": intensity, "range": rng})


add("Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 1.0})
add("Environment", Environment={"sky": True, "sky_top": [0.25, 0.45, 0.85], "sky_horizon": [0.75, 0.82, 0.9], "ground": [0.3, 0.3, 0.3], "bloom": 0.1, "fog": 0.0, "exposure": 0.8})

# the bathroom: white tile, hard and shiny (absorption 0.04)
room("Bathroom", 0, 4, -2, 2, 3, [0.7, 0.78, 0.84], 0.04, 0.01, texture="tiles")
box("Bathroom west wall", [-T, 0, -2 - T], [0, 3, 2 + T], [0.7, 0.78, 0.84], 0.04, 0.01)
divider("Bathroom door wall", 4, -2 - T, 2 + T, 3, 0.6, 2.2, [0.7, 0.78, 0.84], 0.04, 0.01)
lamp("Bathroom lamp", [2, 2.6, 0], [0.9, 0.95, 1.0], 2.0, 9)
add("Tap", Transform={"pos": [2, 1.0, 1.5], "scale": [0.2, 0.25, 0.2]}, MeshRenderer={"mesh": "cylinder", "color": [0.8, 0.82, 0.85, 1], "metallic": 0.9, "roughness": 0.25},
    AudioSource={"file": "examples/sounds/drip.wav", "volume": 0.9, "near": 1.0, "range": 25})

# the living room: carpet and furniture (absorption 0.3)
room("Living room", 4.2, 12, -2, 2, 3, [0.66, 0.5, 0.36], 0.4, 0.03, texture="tiles", skip=("south wall",))
divider("Living room - hall wall", 12, -8 - T, 8 + T, 9, 0.8, 2.4, [0.7, 0.68, 0.64], 0.15)
lamp("Living room lamp", [8, 2.7, 0], [1.0, 0.82, 0.55], 2.0, 10)
add("Sofa", Transform={"pos": [8, 0.4, 1.2], "scale": [2.2, 0.8, 0.8]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.25, 0.25, 1], "roughness": 0.9})

# the store room behind the shut door in the south wall, with a radio on a shelf
box("Store floor", [5.8, -T, -6.2], [10.2, 0, -2.2], [0.5, 0.45, 0.4], 0.2, 0.03)
box("Store ceiling", [5.8, 3, -6.2], [10.2, 3 + T, -2.2], [0.5, 0.45, 0.4], 0.2, 0.03)
box("Store west wall", [5.8, 0, -6.2], [6, 3, -2.2], [0.5, 0.45, 0.4], 0.2, 0.03)
box("Store east wall", [10, 0, -6.2], [10.2, 3, -2.2], [0.5, 0.45, 0.4], 0.2, 0.03)
box("Store back wall", [5.8, 0, -6.2], [10.2, 3, -6], [0.5, 0.45, 0.4], 0.2, 0.03)
# the living room's south wall, with a doorway (x 7.4..8.6) that a wooden door closes
box("Living room south wall (west of the door)", [4.2, 0, -2 - T], [7.4, 3, -2], [0.66, 0.5, 0.36], 0.3, 0.03)
box("Living room south wall (east of the door)", [8.6, 0, -2 - T], [12, 3, -2], [0.66, 0.5, 0.36], 0.3, 0.03)
box("Living room south wall (above the door)", [7.4, 2.2, -2 - T], [8.6, 3, -2], [0.66, 0.5, 0.36], 0.3, 0.03)
box("Wooden door", [7.4, 0, -2 - T], [8.6, 2.2, -2], [0.45, 0.28, 0.14], 0.1, 0.3)
add("Radio", Transform={"pos": [8, 1.0, -5.4], "scale": [0.5, 0.3, 0.2]}, MeshRenderer={"mesh": "cube", "color": [0.15, 0.15, 0.17, 1], "roughness": 0.4},
    AudioSource={"file": "examples/sounds/radio.wav", "volume": 1.0, "near": 1.5, "range": 30})

# the hall: bare concrete, nine metres high (absorption 0.08)
room("Hall", 12 + T, 36, -8, 8, 9, [0.48, 0.48, 0.52], 0.08, 0.02, texture="tiles")
divider("Hall end wall", 36, -8 - T, 8 + T, 9, 3.0, 6.0, [0.48, 0.48, 0.52], 0.08, 0.02)
lamp("Hall lamp 1", [18, 8, 0], [0.9, 0.9, 1.0], 6, 22)
lamp("Hall lamp 2", [30, 8, 0], [0.9, 0.9, 1.0], 6, 22)
add("Clapper (hall)", Transform={"pos": [24, 1.2, -4], "scale": [0.4, 1.8, 0.4]}, MeshRenderer={"mesh": "cylinder", "color": [0.9, 0.55, 0.2, 1], "roughness": 0.6},
    AudioSource={"file": "examples/sounds/clap.wav", "volume": 1.0, "near": 3.0, "range": 70})

# the courtyard: open sky; only the ground
box("Courtyard ground", [36 + T, -T, -30], [90, 0, 30], [0.35, 0.5, 0.3], 0.3, 0.0)
add("Clapper (courtyard)", Transform={"pos": [46, 1.2, 3], "scale": [0.4, 1.8, 0.4]}, MeshRenderer={"mesh": "cylinder", "color": [0.3, 0.6, 0.95, 1], "roughness": 0.6},
    AudioSource={"file": "examples/sounds/clap.wav", "volume": 1.0, "near": 3.0, "range": 70})

# the listener: a camera on a walk
add("Camera", Transform={"pos": [1, 1.6, 0], "rot": [0, -0.70710678, 0, 0.70710678]}, Camera={"fov_y": 70, "far": 400, "clear": [0.5, 0.65, 0.9, 1]})
add("Tour text", Text={"value": "walking tour: {x} metres  (bathroom, living room, hall, courtyard)", "pos": [0.5, 0.94], "size": 0.032, "color": [1, 1, 1, 0.95], "align": 1, "shadow": True})

doc = {
    "lunaris": 1,
    "next_uid": len(entities) + 1,
    "entities": entities,
    "resources": {"x": 1},
    "systems": [
        {"name": "walk", "doc": "The camera (and with it the listener) walks east at 3 m/s and stops in the courtyard.", "each": ["Camera", "Transform"],
         "do": [{"set": "Transform.pos[0]", "to": {"min": [52, {"add": [{"get": "Transform.pos[0]"}, {"mul": [3, {"dt": True}]}]}]}},
                {"res_set": "x", "to": {"floor": {"get": "Transform.pos[0]"}}}]},
    ],
}
out = os.path.join(ROOT, "examples", "echo_tour.lunaris.json")
with open(out, "w", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {out}: {len(entities)} entities")
