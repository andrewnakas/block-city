"""Copy animation clips from the UAL library into the clothed body glTFs.

Bevy only marks bones as animatable when the glTF it loads has animations,
so each body file carries the clips it uses. Channels are retargeted by bone
name (both files use the same Quaternius skeleton).

Usage: python3 tools/merge_anims.py   (run after paint_outfits.py)
"""

import json
import struct

ANIMS = "assets/models/UAL1_Standard.glb"
BODIES = ["assets/models/people/skater.gltf", "assets/models/people/soldier.gltf"]
BODY_BIN = "assets/models/people/body.bin"
CLIPS = [
    "Idle_Loop", "Walk_Loop", "Jog_Fwd_Loop", "Sprint_Loop", "Jump_Start", "Jump_Loop", "Jump_Land",
    "Crouch_Idle_Loop", "Crouch_Fwd_Loop", "Push_Loop", "Roll", "Death01", "Hit_Chest", "Pistol_Idle_Loop",
    "Pistol_Aim_Neutral", "Pistol_Shoot", "Pistol_Reload", "Punch_Cross", "Driving_Loop", "Swim_Idle_Loop",
]


def read_glb(path):
    d = open(path, "rb").read()
    jlen = struct.unpack_from("<I", d, 12)[0]
    g = json.loads(d[20:20 + jlen])
    off = 20 + jlen
    blen = struct.unpack_from("<I", d, off)[0]
    return g, d[off + 8:off + 8 + blen]


def main():
    src, sbin = read_glb(ANIMS)
    body_bin = bytearray(open(BODY_BIN, "rb").read())
    base_len = len(body_bin)
    # The clip data goes into a second buffer shared by all body variants.
    anim_bin = bytearray()
    view_map = {}

    def copy_accessor(g, ai):
        a = dict(src["accessors"][ai])
        bv = src["bufferViews"][a["bufferView"]]
        key = a["bufferView"]
        if key not in view_map:
            start = bv.get("byteOffset", 0)
            data = sbin[start:start + bv["byteLength"]]
            while len(anim_bin) % 4:
                anim_bin.append(0)
            view_map[key] = (len(anim_bin), bv["byteLength"], bv.get("byteStride"))
            anim_bin.extend(data)
        return a, key

    for path in BODIES:
        g = json.load(open(path))
        names = {n.get("name"): i for i, n in enumerate(g["nodes"])}
        g["buffers"] = [g["buffers"][0], {"uri": "anims.bin", "byteLength": 0}]
        views = {}
        g["animations"] = []
        for anim in src["animations"]:
            if anim["name"] not in CLIPS:
                continue
            new = {"name": anim["name"], "samplers": [], "channels": []}
            for s in anim["samplers"]:
                ns = dict(s)
                for k in ("input", "output"):
                    a, key = copy_accessor(g, s[k])
                    if key not in views:
                        off, length, stride = view_map[key]
                        v = {"buffer": 1, "byteOffset": off, "byteLength": length}
                        if stride:
                            v["byteStride"] = stride
                        g["bufferViews"].append(v)
                        views[key] = len(g["bufferViews"]) - 1
                    a["bufferView"] = views[key]
                    g["accessors"].append(a)
                    ns[k] = len(g["accessors"]) - 1
                new["samplers"].append(ns)
            for c in anim["channels"]:
                name = src["nodes"][c["target"]["node"]].get("name")
                if name not in names:
                    continue
                new["channels"].append({"sampler": c["sampler"], "target": {"node": names[name], "path": c["target"]["path"]}})
            g["animations"].append(new)
        g["_anim_views"] = True
        json.dump(g, open(path, "w"))
    for path in BODIES:
        g = json.load(open(path))
        g.pop("_anim_views", None)
        g["buffers"][1]["byteLength"] = len(anim_bin)
        json.dump(g, open(path, "w"))
    open("assets/models/people/anims.bin", "wb").write(anim_bin)
    assert len(body_bin) == base_len
    print(f"merged {len(CLIPS)} clips, anims.bin {len(anim_bin) / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
