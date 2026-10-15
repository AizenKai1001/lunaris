"""Write examples/views.lunaris.json: cameras that draw into pictures (`CameraView`, docs/GAMEPLAY.md). A security monitor on the wall shows a second camera's view of the
yard, and a minimap in the corner is a top-down orthographic camera that the `Ui` itself shows.

    python -I tools/build_views.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def look(eye, target):
    """Rotation of a camera at `eye` (it looks down its own -z) facing `target`."""
    d = [target[i] - eye[i] for i in range(3)]
    yaw = math.atan2(-d[0], -d[2])
    pitch = math.atan2(d[1], math.hypot(d[0], d[2]))
    qy = (0.0, math.sin(yaw / 2), 0.0, math.cos(yaw / 2))
    qx = (math.sin(pitch / 2), 0.0, 0.0, math.cos(pitch / 2))
    x1, y1, z1, w1 = qy
    x2, y2, z2, w2 = qx
    return [w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2, w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2, w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2, w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2]


add("Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 2.0})
add("Yard", Transform={"pos": [0, 0, 0], "scale": [24, 1, 24]}, MeshRenderer={"mesh": "plane", "color": [0.35, 0.5, 0.35, 1], "shininess": 8})
add("Crate", Transform={"pos": [-1.5, 0.5, 0], "rot": [0, 0.26, 0, 0.97]}, MeshRenderer={"mesh": "cube", "color": [0.9, 0.55, 0.2, 1]})
add("Ball", Transform={"pos": [1.2, 0.6, 1.0], "scale": [1.2, 1.2, 1.2]}, MeshRenderer={"mesh": "sphere", "color": [0.2, 0.5, 0.95, 1], "shininess": 48})
add("Post", Transform={"pos": [3.0, 1.0, -1.5], "scale": [0.5, 2.0, 0.5]}, MeshRenderer={"mesh": "cylinder", "color": [0.85, 0.2, 0.25, 1]})
camera = add("Main camera", Transform={"pos": [0, 4.5, 11], "rot": look([0, 4.5, 11], [-1.5, 2.2, 0])}, Camera={"fov_y": 55, "clear": [0.45, 0.6, 0.8, 1]})
security = add("Security camera", Transform={"pos": [7, 3.0, 6], "rot": look([7, 3.0, 6], [0.5, 0.5, 0])}, Camera={"active": False, "fov_y": 60, "clear": [0.1, 0.1, 0.12, 1]})
top = add("Map camera", Transform={"pos": [0, 40, 0], "rot": [-0.7071068, 0, 0, 0.7071068]}, Camera={"active": False, "ortho": True, "ortho_height": 14, "clear": [0.1, 0.35, 0.15, 1]})
monitor_view = add("Monitor view", CameraView={"camera": security, "width": 160, "height": 120})
add("Monitor", Transform={"pos": [-6.5, 3.3, 0], "scale": [4.0, 3.0, 1.0]}, MeshRenderer={"mesh": "quad", "texture": f"view:{monitor_view}", "emissive": 0.9, "color": [1, 1, 1, 1]})
add("Monitor frame", Transform={"pos": [-6.5, 3.3, -0.08], "scale": [4.4, 3.4, 1.0]}, MeshRenderer={"mesh": "quad", "color": [0.08, 0.08, 0.1, 1]})
add("Minimap", Ui={"pos": [0.76, 0.04], "size": [0.21, 0.37], "color": [0, 0, 0, 1], "border": 0.004, "border_color": [1, 1, 1, 1]}, CameraView={"camera": top, "width": 128, "height": 128}, UiStyle={"radius": 0.02})
add("Monitor label", Text={"value": "CAM 2", "pos": [0.04, 0.05], "size": 0.06, "color": [1, 0.3, 0.3, 1], "shadow": True})

scene = {"lunaris": 1, "entities": entities}
path = os.path.join(ROOT, "examples", "views.lunaris.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump(scene, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
