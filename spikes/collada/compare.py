#!/usr/bin/env python3
"""Compare out/rust.jsonl (mesh-loader) with out/js.jsonl (gzweb three r141).
usage: python3 -I compare.py out/rust.jsonl out/js.jsonl"""
import json, sys

def load(p):
    return {json.loads(l)["file"]: json.loads(l) for l in open(p)}

rs, js = load(sys.argv[1]), load(sys.argv[2])

def bbdiff(a, b):
    return max(abs(x - y) for k in ("min", "max") for x, y in zip(a[k], b[k]))

def fmt(bb):
    return "[" + ", ".join(f"{v:.3g}" for v in bb["min"]) + "]..[" + ", ".join(f"{v:.3g}" for v in bb["max"]) + "]"

def tex(mats):
    keys = ("diffuse_texture", "normal_texture", "specular_texture", "emissive_texture", "alpha_texture")
    return sorted({f"{k.split('_')[0]}:{m[k].split('/')[-1]}" for m in mats for k in keys if m.get(k)})

print("| file | unit / up | meshes rs/js | tris rs/js | verts rs/js | AABB max abs diff (m) | mats rs/js | opacity<1 rs/js | untextured diffuse equal | textures rs / js | names rs / js | ms rs/js |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|")
details = []
for f in sorted(rs):
    r, j = rs[f], js[f]
    short = f.split("/")[1] + "/" + f.split("/")[-1] if f.startswith("corpus") else f
    d = bbdiff(r["aabb"], j["aabb"])
    rt, jt = tex(r["materials"]), tex(j["materials"])
    rn = sorted({m["name"] for m in r["meshes"]})
    jn = sorted({m["name"] or "(unnamed)" for m in j["meshes"]})
    ro = sum(1 for m in r["materials"] if m["opacity"] is not None and m["opacity"] < 1)
    jo = sum(1 for m in j["materials"] if m["opacity"] < 1)
    jm = len({m["name"] for m in j["materials"]})
    rm = len({m["name"] for m in r["materials"]})
    def dif(ms, js_):
        out = []
        for m in ms:
            if not m.get("diffuse") or m.get("diffuse_texture"): continue
            out.append(tuple(round(v, 3) for v in m["diffuse"][:3]))
        return sorted(set(out))
    dmatch = "yes" if dif(r["materials"], 0) == dif(j["materials"], 1) else "NO"
    ttxt = ("same" if rt == jt else f"{','.join(rt) or '-'} / {','.join(jt) or '-'}")
    ntxt = ", ".join(rn) + " / " + ", ".join(jn)
    print(f"| {short} | {r['unit']:g} {r['up_axis']} | {r['mesh_count']}/{j['mesh_count']} | {r['total_triangles']}/{j['total_triangles']} | {r['total_vertices']}/{j['total_vertices']} | {d:.2g} | {rm}/{jm} | {ro}/{jo} | {dmatch} | {ttxt} | {ntxt} | {r['parse_ms']:.2f}/{j['parse_ms']:.1f} |")
    details.append(f"- **{short}**: rs AABB {fmt(r['aabb'])}; js AABB {fmt(j['aabb'])}; js root scale {j['root_transform']['scale'][0]:g}, quat {j['root_transform']['quaternion']}")
print()
print("\n".join(details))
