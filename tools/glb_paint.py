"""Dresses the mannequin of Quaternius's Universal Animation Library as a person of Wyrdwood and trims the model for the game: the vertices are coloured by the part of the body they belong to (skin on the head and hands, a tunic
on the body and arms, trousers on the legs, boots on the feet: one `COLOR_0` per vertex, the materials made white), only the clips the game uses are kept, the tracks that never change (a joint that only turns) are
dropped, and the file is packed again without what it no longer uses.

    python -I tools/glb_paint.py HERO_FULL.glb OUT.glb [--keep Clip,Clip,...]
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import glbkit as K  # noqa: E402


def lin(c):
    return tuple(v ** 2.2 for v in c)


SKIN, TUNIC, TROUSERS, BOOTS, BELT = (0.84, 0.64, 0.5), (0.4, 0.31, 0.2), (0.27, 0.25, 0.22), (0.17, 0.12, 0.09), (0.2, 0.14, 0.09)
KEEP = ("Idle_Loop,Walk_Loop,Jog_Fwd_Loop,Sprint_Loop,Crouch_Idle_Loop,Crouch_Fwd_Loop,Jump_Start,Jump_Loop,Jump_Land,Roll,Swim_Fwd_Loop,Swim_Idle_Loop,Death01,Hit_Chest,Hit_Head,Interact,PickUp_Table,Idle_Torch_Loop,"
        "Sword_Attack,Sword_Idle,Punch_Jab,Punch_Cross,Push_Loop,Sitting_Idle_Loop,Sitting_Enter,Sitting_Exit,TreeChopping_Loop,Sword_Regular_A,Sword_Regular_B,Sword_Regular_C,Sword_Block,Idle_Shield_Loop,Shield_OneShot,Idle_Lantern_Loop,"
        "ClimbUp_1m_RM,Consume,Chest_Open,Farm_Harvest,Farm_PlantSeed,Hit_Knockback,Walk_Carry_Loop,OverhandThrow,Melee_Hook,Walk_Formal_Loop").split(",")


def part_color(name):
    n = name.lower()
    if any(k in n for k in ("head", "neck", "hand", "f_", "thumb")):
        return SKIN
    if any(k in n for k in ("foot", "toe")):
        return BOOTS
    if any(k in n for k in ("thigh", "shin")):
        return TROUSERS if "shin" not in n else BOOTS if False else TROUSERS
    if "hips" in n:
        return BELT
    return TUNIC  # spine, shoulder, upper arm, forearm


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    keep = set(sys.argv[sys.argv.index("--keep") + 1].split(",")) if "--keep" in sys.argv else set(KEEP)
    src, out = args[:2]
    g = K.Glb(src)
    doc = g.json
    ap = K.Appender(g)
    joints = g.skin_joints()
    jname = [g.name(j) for j in joints]
    # ---- colours by the joint with the most weight
    for prim in doc["meshes"][0]["primitives"]:
        js = g.accessor(prim["attributes"]["JOINTS_0"])
        ws = g.accessor(prim["attributes"]["WEIGHTS_0"])
        cols = []
        for j4, w4 in zip(js, ws):
            best = max(range(4), key=lambda k: w4[k])
            c = lin(part_color(jname[j4[best]]))
            cols.append((c[0], c[1], c[2], 1.0))
        prim["attributes"]["COLOR_0"] = ap.add(cols, "VEC4")
    for mat in doc.get("materials", []):
        mat["pbrMetallicRoughness"]["baseColorFactor"] = [1, 1, 1, 1]
        mat["pbrMetallicRoughness"]["roughnessFactor"] = 0.85
    # ---- the clips: only the ones kept, and only the tracks that change
    keep_names = [a.get("name") for a in doc["animations"]]
    kept = []
    dropped_tracks = 0
    anims = g.animations()
    new_anims = []
    for an in doc["animations"]:
        name = an.get("name")
        if name not in keep:
            continue
        dur, tracks = anims[name]
        channels, samplers = [], []
        for node, paths in tracks.items():
            for path, (times, vals) in paths.items():
                if path != "rotation" and all(max(abs(v[k] - vals[0][k]) for k in range(3)) < 1e-5 for v in vals):
                    dropped_tracks += 1
                    continue
                t_acc = ap.add(times, "SCALAR", with_minmax=True)
                v_acc = ap.add(vals, "VEC4" if path == "rotation" else "VEC3")
                samplers.append({"input": t_acc, "output": v_acc, "interpolation": "LINEAR"})
                channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path}})
        new_anims.append({"name": name, "channels": channels, "samplers": samplers})
        kept.append(name)
    doc["animations"] = new_anims
    g.bin = ap.finish()
    doc2, blob = K.compact(g)
    K.write_glb(out, doc2, blob)
    missing = [k for k in keep if k not in kept]
    print(f"wrote {out}: {len(kept)} clips, {dropped_tracks} constant tracks dropped, {len(blob) / 1e6:.2f} MB; missing clips: {missing or 'none'}")


if __name__ == "__main__":
    main()
