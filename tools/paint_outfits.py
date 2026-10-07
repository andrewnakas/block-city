"""Paint clothing onto the CC0 Quaternius base body texture.

Each triangle of the body mesh is classified by its dominant skin joint
(head, hands, torso, arms, legs, feet). That gives UV-space masks, so we can
repaint torso/arms/legs/feet as fabric while keeping face and hands as skin.
The original texture's lighting (folds, muscle shading) is kept as a soft
shading term so the clothes don't look flat.

Usage: python3 tools/paint_outfits.py   (writes assets/models/people/*)
"""

import json
import os
import shutil
import struct

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

SRC = "tools/src/ubc"
OUT = "assets/models/people"
SIZE = 2048

COMP = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def accessor(g, binbuf, idx):
    a = g["accessors"][idx]
    bv = g["bufferViews"][a["bufferView"]]
    fmt, size = COMP[a["componentType"]]
    n = NCOMP[a["type"]]
    off = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = bv.get("byteStride", size * n)
    out = np.zeros((a["count"], n), dtype=np.float64)
    for i in range(a["count"]):
        out[i] = struct.unpack_from("<" + fmt * n, binbuf, off + i * stride)
    if a.get("normalized"):
        out /= {5121: 255.0, 5123: 65535.0, 5120: 127.0, 5122: 32767.0}[a["componentType"]]
    return out


def category(bone):
    b = bone.lower()
    if b in ("head", "neck_01"):
        return "head"
    if any(k in b for k in ("hand", "thumb", "index", "middle", "ring", "pinky")):
        return "hands"
    if "lowerarm" in b:
        return "forearm"
    if any(k in b for k in ("spine", "clavicle", "upperarm")):
        return "torso"
    if any(k in b for k in ("pelvis", "thigh")):
        return "hips"
    if "calf" in b:
        return "shin"
    if any(k in b for k in ("foot", "ball")):
        return "feet"
    return "torso"


def masks():
    g = json.load(open(f"{SRC}/Superhero_Male_FullBody.gltf"))
    binbuf = open(f"{SRC}/Superhero_Male_FullBody.bin", "rb").read()
    prim = g["meshes"][2]["primitives"][0]
    uv = accessor(g, binbuf, prim["attributes"]["TEXCOORD_0"])
    joints = accessor(g, binbuf, prim["attributes"]["JOINTS_0"]).astype(int)
    weights = accessor(g, binbuf, prim["attributes"]["WEIGHTS_0"])
    idx = accessor(g, binbuf, prim["indices"]).astype(int).reshape(-1, 3)
    skin_joints = g["skins"][0]["joints"]
    names = [g["nodes"][j]["name"] for j in skin_joints]
    vcat = [category(names[joints[i][int(np.argmax(weights[i]))]]) for i in range(len(uv))]
    cats = ["head", "hands", "forearm", "torso", "hips", "shin", "feet"]
    imgs = {c: Image.new("L", (SIZE, SIZE), 0) for c in cats}
    draws = {c: ImageDraw.Draw(imgs[c]) for c in cats}
    for tri in idx:
        votes = [vcat[v] for v in tri]
        c = max(set(votes), key=votes.count)
        pts = [(uv[v][0] * SIZE, uv[v][1] * SIZE) for v in tri]
        draws[c].polygon(pts, fill=255)
    # Grow masks a little so seams don't show skin through, and close small
    # holes (the navel island) inside clothing regions.
    out = {}
    for c in cats:
        im = imgs[c].filter(ImageFilter.MaxFilter(7))
        if c in ("torso", "hips"):
            im = im.filter(ImageFilter.MaxFilter(41)).filter(ImageFilter.MinFilter(41))
        out[c] = np.asarray(im, dtype=np.float32) / 255.0
    # Skin regions win where a closed clothing mask spilled over them.
    for skin in ("head", "hands"):
        for c in ("torso", "hips"):
            out[c] = out[c] * (1.0 - out[skin])
    return out


def noise(scale, seed):
    rng = np.random.default_rng(seed)
    small = rng.random((max(1, SIZE // scale), max(1, SIZE // scale))).astype(np.float32)
    return np.asarray(Image.fromarray((small * 255).astype(np.uint8)).resize((SIZE, SIZE), Image.BICUBIC), dtype=np.float32) / 255.0


def fabric(color, grain=0.06, seed=1):
    c = np.array(color, dtype=np.float32)[None, None, :] / 255.0
    n = (noise(1, seed) - 0.5) * grain + (noise(8, seed + 1) - 0.5) * grain * 0.8
    return np.clip(c * (1.0 + n[..., None]), 0, 1)


def denim(seed=3):
    base = fabric((52, 74, 112), 0.10, seed)
    y, x = np.mgrid[0:SIZE, 0:SIZE]
    twill = (((x + y) // 2) % 4 < 2).astype(np.float32) * 0.07
    fade = noise(24, seed + 5) * 0.25
    return np.clip(base * (1.0 + twill[..., None]) + fade[..., None] * 0.18, 0, 1)


def camo(seed=7):
    cols = np.array([(78, 84, 52), (52, 58, 38), (104, 96, 66), (34, 32, 26)], dtype=np.float32) / 255.0
    a, b = noise(64, seed), noise(40, seed + 1)
    out = np.empty((SIZE, SIZE, 3), dtype=np.float32)
    out[:] = cols[0]
    out[a > 0.58] = cols[1]
    out[(b > 0.62) & (a <= 0.58)] = cols[2]
    out[(a < 0.3) & (b < 0.4)] = cols[3]
    return np.clip(out * (1.0 + (noise(1, seed + 2)[..., None] - 0.5) * 0.08), 0, 1)


OUTFITS = {
    "skater": {
        "torso": fabric((44, 46, 52), 0.08, 11),  # charcoal hoodie
        "forearm": fabric((44, 46, 52), 0.08, 11),
        "hips": denim(),
        "shin": denim(),
        "feet": fabric((24, 24, 26), 0.05, 13),  # black skate shoes
    },
    "soldier": {
        "torso": camo(),
        "forearm": camo(),
        "hips": camo(9),
        "shin": camo(9),
        "feet": fabric((22, 20, 18), 0.05, 21),  # boots
        "hands": fabric((122, 104, 76), 0.06, 23),  # tan gloves
    },
}


# The hair texture is a neutral base meant to be tinted per character.
HAIR_COLORS = {"skater": [0.22, 0.14, 0.08, 1.0], "soldier": [0.09, 0.075, 0.06, 1.0]}


def tint_hair(g, color):
    for m in g.get("materials", []):
        if "Hair" in m.get("name", ""):
            m.setdefault("pbrMetallicRoughness", {})["baseColorFactor"] = color


def main():
    os.makedirs(OUT, exist_ok=True)
    m = masks()
    base = np.asarray(Image.open(f"{SRC}/T_Superhero_Male_Dark.png").convert("RGB"), dtype=np.float32) / 255.0
    rough = np.asarray(Image.open(f"{SRC}/T_Superhero_Male_Roughness.png").convert("RGB"), dtype=np.float32) / 255.0
    nrm = np.asarray(Image.open(f"{SRC}/T_Superhero_Male_Normal.png").convert("RGB"), dtype=np.float32) / 255.0
    lum = base @ np.array([0.299, 0.587, 0.114], dtype=np.float32)
    ref = np.median(lum[m["torso"] > 0.5])
    shade = 1.0 + (np.clip(lum / ref, 0.4, 1.6) - 1.0) * 0.45
    flat = np.array([0.5, 0.5, 1.0], dtype=np.float32)
    for name, outfit in OUTFITS.items():
        col, rgh, nm = base.copy(), rough.copy(), nrm.copy()
        for part, tex in outfit.items():
            w = m[part][..., None]
            col = col * (1 - w) + np.clip(tex * shade[..., None], 0, 1) * w
            rgh = rgh * (1 - w) + 0.9 * w  # cloth is rough
            nm = nm * (1 - w * 0.75) + flat * (w * 0.75)  # lose skin muscle detail
        Image.fromarray((col * 255).astype(np.uint8)).save(f"{OUT}/{name}_base.png", optimize=True)
        Image.fromarray((rgh * 255).astype(np.uint8)).save(f"{OUT}/{name}_rough.png", optimize=True)
        Image.fromarray((nm * 255).astype(np.uint8)).save(f"{OUT}/{name}_normal.png", optimize=True)
        g = json.load(open(f"{SRC}/Superhero_Male_FullBody.gltf"))
        g["buffers"][0]["uri"] = "body.bin"
        swap = {"T_Superhero_Male_Dark.png": f"{name}_base.png", "T_Superhero_Male_Roughness.png": f"{name}_rough.png", "T_Superhero_Male_Normal.png": f"{name}_normal.png"}
        for im in g["images"]:
            im["uri"] = swap.get(im["uri"], im["uri"])
        tint_hair(g, HAIR_COLORS[name])
        json.dump(g, open(f"{OUT}/{name}.gltf", "w"))
    shutil.copy(f"{SRC}/Superhero_Male_FullBody.bin", f"{OUT}/body.bin")
    for f in ["T_Hair_1_BaseColor.png", "T_Hair_1_Normal_png.png", "T_Hair_1_Normal.png", "T_Eye_Brown.png", "T_Eye_Normal_png.png"]:
        shutil.copy(f"{SRC}/{f}", f"{OUT}/{f}")
    for f, who in [("Hair_SimpleParted", "skater"), ("Hair_Buzzed", "soldier")]:
        shutil.copy(f"{SRC}/{f}.bin", f"{OUT}/{f}.bin")
        g = json.load(open(f"{SRC}/{f}.gltf"))
        tint_hair(g, HAIR_COLORS[who])
        json.dump(g, open(f"{OUT}/{f}.gltf", "w"))
    print("wrote", sorted(os.listdir(OUT)))


if __name__ == "__main__":
    main()
