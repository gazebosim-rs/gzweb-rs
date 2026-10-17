// JS reference: load .dae files with gzweb's patched three r141 ColladaLoader
// (../../../../gzweb/include/ColladaLoader.js) under jsdom and print the same
// JSON summary the Rust binary prints.
//
// usage: node ref.mjs <file.dae>...   (one JSON object per line)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

const here = path.dirname(fileURLToPath(import.meta.url));
const gzwebInclude = path.resolve(here, "../../../../gzweb/include");

// ColladaLoader needs DOMParser (and TextureLoader wants `document`).
const dom = new JSDOM("<!doctype html><html><body></body></html>");
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.DOMParser = dom.window.DOMParser;

// Copy gzweb's loader next to our node_modules so `import "three"` resolves to
// three@0.141.0 here; ESM needs the explicit .js on the TGALoader import.
const vendor = path.join(here, ".vendor");
fs.mkdirSync(vendor, { recursive: true });
for (const f of ["ColladaLoader.js", "TGALoader.js"]) {
  let src = fs.readFileSync(path.join(gzwebInclude, f), "utf8");
  src = src.replace(/from "\.\/TGALoader"/, 'from "./TGALoader.js"');
  fs.writeFileSync(path.join(vendor, f), src);
}

const THREE = await import("three");
const { ColladaLoader } = await import(path.join(vendor, "ColladaLoader.js"));

// Don't fetch images: record the requested URL instead.
THREE.TextureLoader.prototype.load = function (url) {
  const t = new THREE.Texture();
  t.name = url;
  return t;
};
// Silence the loader's console.log("File version") etc. on stdout.
console.log = (...a) => process.stderr.write(a.join(" ") + "\n");
const out = (o) => process.stdout.write(JSON.stringify(o) + "\n");

const r = (v) => Math.round(v * 1e5) / 1e5;
const v3 = (v) => [r(v.x), r(v.y), r(v.z)];

function texName(t) {
  return t ? t.name || "(texture)" : null;
}

function summarize(file) {
  const text = fs.readFileSync(file, "utf8");
  const loader = new ColladaLoader();
  const t0 = performance.now();
  const collada = loader.parse(text, "");
  const parseMs = performance.now() - t0;
  const scene = collada.scene;
  scene.updateMatrixWorld(true);

  const meshes = [];
  const mats = new Map();
  scene.traverse((o) => {
    if (!o.isMesh) return;
    const g = o.geometry;
    const pos = g.getAttribute("position");
    const tris = g.index ? g.index.count / 3 : pos.count / 3;
    const ml = Array.isArray(o.material) ? o.material : [o.material];
    ml.forEach((m) => mats.set(m.uuid, m));
    const chain = [];
    for (let p = o; p && p !== scene; p = p.parent) chain.unshift(p.name || "");
    const box = new THREE.Box3().setFromObject(o);
    meshes.push({
      name: o.name,
      node_path: chain.join("/"),
      vertices: pos.count,
      triangles: tris,
      primitives: Math.max(1, g.groups.length),
      materials: ml.map((m) => m.name),
      has_normals: !!g.getAttribute("normal"),
      has_uvs: !!g.getAttribute("uv"),
      has_uv2: !!g.getAttribute("uv2"),
      has_colors: !!g.getAttribute("color"),
      aabb: { min: v3(box.min), max: v3(box.max) },
    });
  });

  const box = new THREE.Box3().setFromObject(scene);
  const xmlUnit = /<unit[^>]*meter="([^"]+)"/.exec(text);
  const xmlUp = /<up_axis>\s*([^<\s]+)/.exec(text);
  out({
    file: path.relative(process.cwd(), file),
    loader: "three-r141-gzweb",
    parse_ms: r(parseMs),
    unit: xmlUnit ? parseFloat(xmlUnit[1]) : 1,
    up_axis: xmlUp ? xmlUp[1] : "Y_UP",
    root_transform: {
      position: v3(scene.position),
      quaternion: [r(scene.quaternion.x), r(scene.quaternion.y), r(scene.quaternion.z), r(scene.quaternion.w)],
      scale: v3(scene.scale),
    },
    mesh_count: meshes.length,
    total_vertices: meshes.reduce((a, m) => a + m.vertices, 0),
    total_triangles: meshes.reduce((a, m) => a + m.triangles, 0),
    aabb: { min: v3(box.min), max: v3(box.max) },
    meshes,
    materials: [...mats.values()].map((m) => ({
      name: m.name,
      kind: m.type,
      diffuse: m.color ? [r(m.color.r), r(m.color.g), r(m.color.b)] : null,
      diffuse_texture: texName(m.map),
      normal_texture: texName(m.normalMap),
      specular_texture: texName(m.specularMap),
      emissive_texture: texName(m.emissiveMap),
      alpha_texture: texName(m.alphaMap),
      opacity: r(m.opacity),
      transparent: m.transparent,
      double_sided: m.side === THREE.DoubleSide,
    })),
  });
}

for (const f of process.argv.slice(2)) {
  try {
    summarize(f);
  } catch (e) {
    out({ file: f, loader: "three-r141-gzweb", error: String(e && e.stack || e) });
  }
}
