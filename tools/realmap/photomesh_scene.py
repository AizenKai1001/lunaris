"""A Lunaris scene of one photogrammetry tile that obj_to_gltf.py made (mesh.gltf in a folder with its JPEG patches):

    python tools/realmap/photomesh_scene.py OUT_SCENE_JSON MESH_PATH [MESH_PATH ...]      (paths relative to the workspace root: one for each chunk obj_to_gltf.py wrote)

Sun, sky, camera only: the pictures are taken by hl_render.sh / render.py with their own views.
"""
import json
import math
import sys


def quat(yaw, pitch):
    sy, cy = math.sin(math.radians(yaw) / 2), math.cos(math.radians(yaw) / 2)
    sp, cp = math.sin(math.radians(pitch) / 2), math.cos(math.radians(pitch) / 2)
    return [cy * sp, sy * cp, -sy * sp, cy * cp]


def main():
    out, meshes = sys.argv[1], sys.argv[2:]
    ents = []

    def add(c):
        ents.append({"uid": len(ents) + 1, "components": c})

    add({"Environment": {"sky": True, "clouds": 0.25, "shadows": True, "shadow_distance": 400, "ssao": 1.0, "bloom": 0.06, "vignette": 0.1, "tonemap": 1, "ambient": 1.6, "ground": [0.5, 0.48, 0.42]}})
    add({"Light": {"kind": 0, "intensity": 3.6, "color": [1.0, 0.95, 0.86]}, "Transform": {"rot": quat(215, -42)}})
    add({"Atmosphere": {"turbidity": 2.2, "intensity": 1.0}})
    add({"Shadows": {"softness": 0.02}})
    add({"AutoExposure": {"min": 0.3, "max": 2.5, "speed": 3.0, "target": 0.34}})
    add({"Camera": {"fov_y": 55, "active": True, "far": 5000.0}, "Transform": {"pos": [0, 120, 200]}})
    for k, mesh in enumerate(meshes):
        add({"Name": {"value": f"Photogrammetry chunk {k}"}, "Transform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": mesh, "roughness": 0.9}})
    json.dump({"lunaris": 1, "entities": ents}, open(out, "w"))


if __name__ == "__main__":
    main()
