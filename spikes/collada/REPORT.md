# Spike: Collada (.dae) loading with `mesh-loader`

**Question.** Can the pure-Rust [mesh-loader](https://github.com/openrr/mesh-loader)
crate (v0.1.13, Apache-2.0, `forbid(unsafe)`, roxmltree-based) be the base of
a Bevy 0.18 `AssetLoader` for `.dae`? That would stop Collada visuals
rendering as a pink cube. How close is its output to gzweb's three.js loader?

**Answer.** Yes, but with patches. The verdict is to vendor its `collada`
module and rewrite the scene-assembly layer.

- **What works:** XML, geometry and effect parsing are solid. Triangle and
  vertex counts match three.js exactly on all 14 real Fuel meshes.
- **What is broken:** the scene-assembly layer (`collada/instance.rs`) has four
  bugs that break real models:
  - It misplaces geometry when a node has a transform and the `<unit>` is not
    metres.
  - It loses per-primitive materials.
  - It does not transform normals.
  - It ignores instancing.
- **What is missing:** it keeps no node names, so gz `<submesh>` selection
  cannot work.
- **Estimate:** about 1 to 1.5 engineer-weeks to a production loader.

Reproduce with `./run.sh`, after `./fetch.sh` downloads the corpus. The full
comparison table is generated into `out/compare.md`.

## Method

| Piece | What it does |
|---|---|
| `fetch.sh` | Downloads 14 `.dae` files from 10 Fuel models into `corpus/` (gitignored). |
| `synthetic/*.dae` | Three hand-written probes for features the corpus lacks: a node hierarchy (rotated parent, translated child), one geometry instanced three times (once through `library_nodes`/`instance_node`), a two-material geometry (`<triangles>` + quad `<polylist>`), cm units with node translations, an FCOLLADA `<bump>` in `<extra>`, and transparency. |
| `src/summary.rs`, `src/main.rs` | `collada_spike [--runs N] files…` parses each file with `mesh_loader::collada::from_slice` and prints JSON lines: meshes, counts, AABBs, materials, textures, UV/normal presence, and the median parse time. |
| `js/ref.mjs` | Node + jsdom + `three@0.141.0` running gzweb's own `../gzweb/include/ColladaLoader.js`. It copies the loader into `js/.vendor/` so `import "three"` resolves; only the TGALoader import path is rewritten. `TextureLoader.load` is stubbed to record URLs. Prints the same JSON, using `Box3.setFromObject(scene)`. |
| `compare.py` | Joins the two outputs into `out/compare.md`. |
| `src/bevy_conv.rs`, `tests/bevy_conv.rs` | Converts mesh-loader output to `bevy_mesh::Mesh` and `bevy_pbr::StandardMaterial`, plus an `AssetLoader` sketch. A headless test converts the whole corpus and checks the attributes. |

### Which transform each loader applies

**gzweb (three.js).** gzweb's patched `parseAssetUpAxis` always returns
`"Y_UP"`, and the remaining code only rotates for `Z_UP`. So `up_axis` is
ignored entirely: every file loaded with an identity root quaternion,
including the `Y_UP` Coke Can and Cordless Drill.

- The only root transform is `scene.scale = unit`. It sits above all nodes, so
  node translations are scaled too.
- Collada coordinates land directly in Gazebo's Z-up world.
- The header comment in `ColladaLoader.js` describes an older `Y_UP → +90°`
  diff, but the current code does not do that.
- `prepareColladaMesh` only removes lights.
- `useSubMesh` matches `THREE.Mesh.name === submesh`. In three, that name is the
  Collada `<node name>`, but only when the node holds a single primitive and
  has no child nodes. Otherwise the meshes are unnamed children of a named
  `Group`, and `useSubMesh` cannot select them.

**mesh-loader.** It also ignores `up_axis` (`"up_axis" => {} // TODO`), so the
two loaders agree. That is the behaviour parity needs.

- It bakes every transform into the vertex positions:
  `world = node · parent · … · root · S(unit)`.
- That order is wrong twice:
  - Parent and child are multiplied in reverse (`transform *= parent`).
  - The unit scale is post-multiplied, so `<unit>` does not scale node
    translations.
- Normals are copied untransformed.
- The returned `Scene` has no node tree and no transforms.

## Comparison: mesh-loader (rs) vs gzweb three r141 (js)

| file | unit / up | meshes rs/js | tris rs/js | AABB max \|Δ\| (m) | materials rs/js | textures | names rs / js (node) | parse ms rs / js |
|---|---|---|---|---|---|---|---|---|
| Backpack | 0.01 Z | 1/1 | 2062/2062 | 0 | 1/1 | same | `Backpack-lib` / `Backpack` | 0.69 / 15.6 |
| Cave Straight A | 0.01 Z | 4/4 | 15586/15586 | **600** | 4/4 | same | `CaveWall-lib`… / `CaveWall`… | 5.7 / 74 |
| Coke Can | 0.00254 **Y** | 1/1 | 5120/5120 | 0 | 1/1 | same | `shape0-lib` / `node` | 1.4 / 19 |
| Construction Cone | 0.0254 Z | 1/1 | 166/166 | 0 | 1/1 | same | `geom-Box001` / `Box001` | 0.08 / 2.4 |
| Cordless Drill | 0.001 **Y** | 1/1 | 1126/1126 | 0 | 1/1 | same | `shape0-lib` / `node` | 0.31 / 4.2 |
| Fire Hose connector | 1 Z | 1/1 | 124/124 | 0 | 1 (empty)/1 (three default) | – | `Cylinder-mesh` / `Cylinder` | 0.05 / 1.2 |
| Gazebo logo | 0.0254 Z | 1/1 | 1900/1900 | 0 | 1/1 | same | `geom-Gazebo` / `Gazebo` | 0.50 / 6.7 |
| Pickup | 0.0254 Z | 1/1 | 1084/1084 | **0.037** | **1/2** | **wheels2.jpg lost** | `geom-Truck` / `Truck` | 0.26 / 4.1 |
| Pine Tree | 1 Z | 2/2 | 676/676 | 0 | 2/2 | same | `Cylinder002-mesh`… / `Bark`, `Branch` | 0.21 / 3.5 |
| Rescue Randy | 0.01 Z | 1/1 | 5266/5266 | 0 | 1/1 | same | `Object014-lib` / `Object014` | 1.9 / 22 |
| Urban Station (48 MB) | 0.01 Z | 16/16 | 243185/243185 | **840** | **14/15** | **ThirdRail gets Rail texture** | `*-lib` / node names | **104 / 1190** |
| X2 led | 1 Z | 1/1 | 94/94 | 0 | 1/1 | same | `LEDIndicator-lib` / `LEDIndicator` | 0.05 / 1.5 |
| X2 chassis (polylist) | 1 Z | 1/1 | 2002/2002 | 1e-5 | 1/1 | same | `X2_Body-lib` / `X2_Body` | 0.61 / 8.1 |
| X2 wheel left | 1 Z | 1/1 | 2436/2436 | 0 | **1/2** (tyre colour lost) | same | `X2_Wheel_Left-lib` / `X2_Wheel_Left` | 0.78 / 9.7 |
| synthetic/hierarchy | 0.01 Y | 2/1 | 4/1 | **100** | 1/1 | – | geometry ids / `ChildNode` | – |
| synthetic/probe | 0.01 Y | **2/5** | **4/6** | **300** | 1/2 | **rs: none; js: diffuse + normal** | `tri-mesh`,`twomat-mesh` / `ChildNode`,`SecondInstance`,`RefNode`,(unnamed)×2 | – |
| synthetic/textured | 0.01 Y | 3/1 | 6/2 | 3 | 2/1 | **normal map lost** | +2 stray uninstanced geometries | – |

**Notes on the table:**
- Both loaders emit triangle soup, so vertex counts are 3 × triangles on both
  sides.
- UV and normal presence matched on every real file. Non-textured diffuse
  colours matched, except for the material used by the wheel's lost primitive.
- Both loaders report opacity 0 for the two SketchUp exports (Coke Can,
  Drill), which declare `<transparency>0</transparency>` with
  `opaque="A_ONE"`. The Bevy conversion treats opacity 0 as opaque.

## What mesh-loader gets right, wrong, and misses

**Right**
- **Primitives:** `<triangles>`, `<polylist>`, `<polygons>` (fan-triangulated,
  holes ignored), `<tristrips>` and `<trifans>`. Lines are skipped.
- **Input handling:** accessor strides, shared `<vertices>` inputs, comma
  decimals and UTF-16.
- **Counts and bounds:** counts are identical to three on all 14 real files.
  AABBs are identical wherever the nodes have no translation or the unit is 1.
- **Units:** the `<unit>` scale is applied to the vertices themselves.
- **Effects:** `constant`, `lambert`, `phong` and `blinn` parse correctly:
  colours, shininess, assimp-style transparency/opacity rules, and texture
  lookup through `newparam` surface/sampler2D or a direct image id.
  - Use `collada::from_slice`: it returns the raw `init_from` path.
  - Avoid the path-based `Loader::load`, which silently drops any texture that
    is not on local disk next to the `.dae`. That makes it useless for Fuel and
    websocket assets.
- **Speed:** about 2.2 ms/MB native, 11× faster than three under Node/jsdom.
  That is not a like-for-like comparison with a browser DOMParser.

**Wrong** (these affect real Fuel models)
1. **Hierarchy order is reversed.** It computes `child · parent` instead of
   `parent · child`. In `synthetic/hierarchy` the triangle lands at x = 100
   instead of y = 1.
2. **`<unit>` is not applied to node translations.** It post-multiplies
   `S(unit)` at the root. The Cave tile is 600 m off, the Urban Station 840 m,
   the Pickup 3.7 cm.
3. **Normals are not transformed** by the node matrix; only positions are.
4. **One material per geometry.** All of a geometry's primitives are merged,
   and the first bound primitive's material wins:
   - The Pickup loses its wheels texture.
   - The X2 wheel loses its tyre colour.
   - The Station's `ThirdRail` gets `Rail_Diffuse`, because its first primitive
     is an empty `count="0"` Rail primitive.
5. **The material symbol map is global**, not per `<instance_geometry>`. When
   two geometries reuse a symbol name, the last binding wins.
6. **One output mesh per `<geometry>`, not per instance.**
   - A geometry used by several nodes is emitted once, with the last node's
     transform.
   - Geometries nothing instances are still emitted, in raw units.
7. **Diffuse is reported as assimp's 0.6 grey** when the diffuse slot is a
   texture; three uses white. The prototype corrects this.
8. **It can panic.** Mesh building has `panic!()`/`todo!()` paths (normal/uv
   index exhaustion, `VERTEX` inputs that point at another mesh), and a panic
   aborts a wasm app. Also, `<double_sided>` directly under a technique reads
   the attribute from the wrong node, which fails the parse.

**Missing**
- **Node identity:** the node `name`, `id` and `sid`, and the node tree itself.
  gz `<submesh>` and `center` need the node name. Centring a submesh the way
  gzweb does also needs per-node transforms.
- **Node libraries:** `library_nodes` and `instance_node`.
- **Normal maps in `<extra>`:** maps written as FCOLLADA/OpenCOLLADA
  `<extra><technique><bump>` are dropped. That is the usual 3ds Max and Blender
  export path. Only `<bump>` as a direct child of the shading technique is
  read.
- **`up_axis` is not exposed.** That is fine for gzweb parity, but a loader
  should report it.
- **Also unsupported:**
  - `<texture texcoord>` / `bind_vertex_input` set mapping (only UV set 0 is
    used, the same as three in practice);
  - embedded image data;
  - `<matrix>` skew.
- **Output is always unindexed triangle soup.**

**three/gzweb quirks.** The reference is not ground truth:
- For an `instance_node` under an otherwise empty node, three collapses the two
  and overwrites the library node's own transform. The probe's `scale 2` is
  lost.
- Multi-material nodes become unnamed meshes, which gzweb's `useSubMesh`
  cannot find.

## Bevy 0.18 conversion sketch (`src/bevy_conv.rs`, test passes)

**Mesh.** `Mesh::new(TriangleList, RenderAssetUsages::default())` with these
attributes:
- `ATTRIBUTE_POSITION`;
- `ATTRIBUTE_NORMAL`, or `compute_normals()` if the lengths mismatch;
- `ATTRIBUTE_UV_0`, with V flipped (`1 - v`);
- `ATTRIBUTE_COLOR`, if present;
- `Indices::U32`.

Conversion takes 2 ms for the 243k-triangle Station.

**StandardMaterial.**
- **Base colour:** `Color::srgba(diffuse)`. gzweb runs three r141 with linear
  output and no colour management, which matches sRGB-in / sRGB-out in Bevy.
  It is forced to white when there is a diffuse texture.
- **Alpha:** `diffuse.a × opacity`, with opacity 0 treated as 1. `alpha_mode`
  is `Blend` when alpha < 1.
- **Surface:** `perceptual_roughness = sqrt(2 / (shininess + 2))`,
  `metallic = 0`.
- **Textures:** `base_color_texture`, `normal_map_texture` (needs
  `generate_tangents()` in production) and `emissive_texture`.

**AssetLoader.** `ColladaLoader` (extensions `["dae"]`):
1. Reads the bytes and calls `collada::from_slice`.
2. Adds labeled `Mesh{i}` and `Material{i}` assets.
3. Resolves texture paths with `load_context.path().resolve_embed(p)`, so
   `../materials/textures/x.png` works relative to the `.dae`.
4. Returns `ColladaAsset { names, meshes, materials }` with `#[dependency]`
   handles.

**Production shape.** It should mirror the glTF loader and emit a
`bevy_scene::Scene`:
- one entity per Collada node, with `Name(node name)` and a local `Transform`;
- one child per primitive, with `Mesh3d` + `MeshMaterial3d`;
- the root scaled by `unit`, with no up-axis rotation (as in gzweb).

The spawner then spawns a `SceneRoot` as it does for glTF/OBJ. Submesh
selection keeps the named node's subtree, and `center` recentres that
primitive's vertices.

## Performance (native, release, median of 5 parses)

| File | Size | Triangles | mesh-loader parse | three r141 + jsdom (Node) | Bevy conversion |
|---|---|---|---|---|---|
| Station | 47.8 MB | 243k | 104 ms | 1190 ms | 2 ms |
| Cave tile | 2.5 MB | 15.6k | 5.7 ms | 74 ms | – |
| Typical robot or prop | 30 KB – 900 KB | – | 0.05 – 1.9 ms | 1 – 22 ms | – |

WASM was not measured.

## Recommendation

**Vendor mesh-loader's `collada` module and patch it.** It is Apache-2.0, so
keep the NOTICE and the file headers.

- **Keep its parsers:** geometry, effect, image, material and XML utilities,
  about 3.5k LOC, verified here.
- **Rewrite `instance.rs`** (about 330 LOC) **and the node parsing in
  `scene.rs`** to produce:

  ```rust
  struct ColladaScene { unit: f32, up_axis: UpAxis, roots: Vec<Node>, meshes: Vec<Primitive>, materials: Vec<Material> }
  struct Node { name, id, local: Mat4, children: Vec<Node>, primitives: Vec<(mesh_idx, material_idx)> }
  ```

- **The rewrite must also deliver:**
  - one `Primitive` per `<triangles>`/`<polylist>`;
  - per-instance material binding;
  - correct `parent · child` order;
  - `unit` as a root scale;
  - `library_nodes` and `instance_node`;
  - `<bump>` inside `<extra>`;
  - white diffuse under a texture;
  - errors instead of panics.
- **Upstream:** bugs 1–3 are small and worth sending as PRs to
  openrr/mesh-loader. Don't block on upstream for the structural API change.
- **Validation:** use this spike's harness, `run.sh` plus a per-node AABB check
  against `js/ref.mjs`.

**Effort, about 6–8 days in total:**

| Work | Days |
|---|---|
| Fork and patch, plus regression tests on this corpus and the probes | 3–4 |
| Bevy `AssetLoader` → `Scene`: labeled assets, texture resolution through the `gzhttps://`/`wsasset://` sources, tangents for normal maps, and hooking into `spawn_mesh_file` | 2–3 |
| `<submesh>`/`center` support | 1 |

**Alternatives ruled out:**
- Using mesh-loader as-is is not viable: 4 of the 14 real files come out wrong.
- Writing from scratch on roxmltree would redo accessor, primitive and effect
  parsing that already works here: about 2–3 weeks for no gain.
