"""Brings the animation clips of one rigged model onto the skeleton of another, and writes a model with both sets of clips:

    python -I tools/glb_retarget.py BASE.glb SOURCE.glb OUT.glb [--prefix u2_] [--only Name,Name]

Quaternius's two Universal Animation Libraries are the same skeleton (the same rest pose, joint for joint) with different names for the joints (`DEF-upper_arm.L` and `upperarm_l`), so the clips of the one drop onto the
other by naming alone: this maps the joint names of SOURCE to those of BASE (a table for those two libraries), checks that the rest transforms of every mapped joint agree (and says so if they do not), copies the keyframes
over unchanged, and adds the clips to BASE's own. A joint with no counterpart (a fingertip leaf) is dropped; a clip whose name BASE already has gets the prefix. Everything is standard library.
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import glbkit as K  # noqa: E402


def ue_to_rigify(name):
    """The joint of Quaternius's Rigify-style skeleton that the Unreal-mannequin-style name stands for (None if there is none)."""
    if name in ("root",):
        return "root"
    table = {"pelvis": "DEF-hips", "spine_01": "DEF-spine.001", "spine_02": "DEF-spine.002", "spine_03": "DEF-spine.003", "neck_01": "DEF-neck", "Head": "DEF-head"}
    if name in table:
        return table[name]
    m = re.fullmatch(r"(clavicle|upperarm|lowerarm|hand|thigh|calf|foot|ball)_([lr])", name)
    if m:
        part = {"clavicle": "shoulder", "upperarm": "upper_arm", "lowerarm": "forearm", "hand": "hand", "thigh": "thigh", "calf": "shin", "foot": "foot", "ball": "toe"}[m.group(1)]
        return f"DEF-{part}.{m.group(2).upper()}"
    m = re.fullmatch(r"(index|middle|pinky|ring)_0([123])_([lr])", name)
    if m:
        return f"DEF-f_{m.group(1)}.0{m.group(2)}.{m.group(3).upper()}"
    m = re.fullmatch(r"thumb_0([123])_([lr])", name)
    if m:
        return f"DEF-thumb.0{m.group(1)}.{m.group(2).upper()}"
    return None


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opt = lambda n, d=None: sys.argv[sys.argv.index(n) + 1] if n in sys.argv else d  # noqa: E731
    base_path, src_path, out_path = args[:3]
    prefix = opt("--prefix", "u2_")
    only = set(opt("--only").split(",")) if opt("--only") else None
    base, src = K.Glb(base_path), K.Glb(src_path)
    by_name = {base.name(j): j for j in base.skin_joints()}
    mapping = {}  # source node -> base node
    off = []
    for j in src.skin_joints():
        target = ue_to_rigify(src.name(j))
        if target is None or target not in by_name:
            continue
        mapping[j] = by_name[target]
        (t0, r0, s0), (t1, r1, s1) = src.rest(j), base.rest(mapping[j])
        d = max(abs(a - b) for a, b in zip(r0 + t0, r1 + t1))
        if d > 1e-3:
            off.append((src.name(j), round(d, 4)))
    print(f"{len(mapping)} of {len(src.skin_joints())} joints of the source have a joint in the base; rest transforms that differ by more than 1e-3: {off or 'none'}")
    ap = K.Appender(base)
    doc = base.json
    existing = {a.get("name") for a in doc.get("animations", [])}
    added = 0
    for name, (dur, tracks) in src.animations().items():
        if only is not None and name not in only:
            continue
        out_name = name if name not in existing else prefix + name
        channels, samplers = [], []
        for node, paths in tracks.items():
            dst = mapping.get(node)
            if dst is None:
                continue
            for path, (times, vals) in paths.items():
                t_acc = ap.add(times, "SCALAR", with_minmax=True)
                v_acc = ap.add(vals, "VEC4" if path == "rotation" else "VEC3")
                samplers.append({"input": t_acc, "output": v_acc, "interpolation": "LINEAR"})
                channels.append({"sampler": len(samplers) - 1, "target": {"node": dst, "path": path}})
        doc.setdefault("animations", []).append({"name": out_name, "channels": channels, "samplers": samplers})
        existing.add(out_name)
        added += 1
    K.write_glb(out_path, doc, ap.finish())
    print(f"wrote {out_path}: {len(doc['animations'])} clips ({added} added from the source)")


if __name__ == "__main__":
    main()
