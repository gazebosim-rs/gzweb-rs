# gzweb → gzweb-rs Port: Gap Analysis, Implementation Plan, Parity Test Plan

Status as of 2026-10-16. Reference implementation: `../gzweb` (v3.0.1, three.js r141).
Rust port: this repo (Bevy 0.20, wgpu 30). Bevy is the renderer **and** the UI toolkit
(bevy_ui) — no three.js, no DOM-built UI beyond the bare canvas host page.

Companion docs: [`CONTEXT.md`](./CONTEXT.md), [`DESIGN.md`](./DESIGN.md). There is no
worklog yet — start `.agent/WORKLOG.md` (dated entries: what changed, what was verified).

---

## 1. Where the port stands

### 1.1 What exists (≈3.5k lines Rust vs ≈8.5k lines first-party TS + 12k vendored loaders)

| Area | Rust status |
|---|---|
| WebSocket transport (native tungstenite thread / wasm web-sys) | Works for `protos` → `worlds` → `scene` → `sub` handshake |
| Protobuf types | Hand-written prost structs; tags verified against `gz-msgs` harmonic protos (correct) |
| Scene spawn | Lights + models/links/visuals/nested models, flattened to world transforms |
| Primitives | box, cylinder, sphere, plane, capsule, cone |
| Meshes | glTF/GLB (bevy_gltf), STL (bevy_stl), OBJ (bevy_obj); DAE → pink cube |
| Assets | `fuel://` HTTP source; `wsasset://` websocket proxy (wasm, `file://` only) |
| Materials | diffuse, emissive, PBR scalars, transparency, double-sided |
| Dynamic poses | Subscribes to `dynamic_pose/info`, applies by name (see bugs) |
| UI | FPS / backend / WS status text, orbit camera, click-to-refocus on ground plane |
| Tests | 8 unit tests (fuel URI, frame split, scene proto round-trip) — all pass |

### 1.2 Defects in the current Rust code

> **Status (2026-10-16), on `feature/port-foundation`:**
> - **Fixed:** 1–6 and 10. The fix for 3 holds by construction (raw meshes
>   under the Z-up root, as in gzweb) but has no mesh corpus to test it yet.
> - **7, partly fixed:** URL and auth key are configurable. `wss://` still
>   needs TLS in the native driver.
> - **8, partly fixed:** light direction is now relative to the light pose,
>   attenuation feeds the intensity formula, and the scene dump is gone.
>   `visible`, `cast_shadows`, `scene.shadows`, sky and grid are still to do.
> - **9, partly fixed:** the crate is renamed and the debug background removed.
> - **New defect, fixed:** asset replies are serialized `gz.msgs.Bytes` and
>   were treated as raw bytes.
>
> See WORKLOG.md.

1. **`src/websocket.rs:431`** (old prototype, since removed) — on a binary `authorized`/`invalid` frame it calls
   `ws.receiver.try_recv()` again, silently **dropping the next real message**. Also the
   auth reply is never acted on in that path (gzweb re-sends `protos` on `authorized`).
2. **No entity hierarchy.** Everything is spawned flat with pre-composed world transforms
   (`combine_transforms`). Gazebo's `dynamic_pose/info` carries poses *relative to the
   parent*; `apply_dynamic_poses` writes them as world transforms onto entities whose
   `gz_name` is the *visual* name, so link/model pose updates either never match or land
   in the wrong place. gzweb relies on three.js parenting and sets local pose.
3. **Mesh orientation.** Primitives get the Z-up→Y-up basis change baked into their
   dimensions, but loaded STL/OBJ/DAE vertex data (authored Z-up for Gazebo) is not
   converted, so meshes render rotated −90° about X relative to their poses.
4. **`model://` and other schemes** pass through unchanged into `AssetServer::load`, which
   interprets `model` as an unknown asset source and fails. gzweb strips `file://` and
   `model://` and tries HTTP, then falls back to websocket `asset` requests.
5. **Asset fetch strategy differs by platform**, not by fallback: native reads disk, wasm
   uses websocket only for `file://`. gzweb: HTTP first, websocket on failure, everywhere.
6. **`WsAssetReader` busy-polls** (`yield_now` loop, ~6000 polls); on native this spins a
   task-pool thread. Use a oneshot channel / waker. Asset errors (`StringMsg`) are logged
   but never wake the reader, so it waits for the full timeout.
7. **Host is hard-coded to `localhost`** and only `ws://`. gzweb takes a full URL + auth key.
8. Scene `visible`, `cast_shadows`, `scene.shadows`, `scene.grid`, `scene.sky`, light
   attenuation and `direction` relative to the light pose are ignored; `info!("{:#?}", scene)`
   dumps the whole scene on every load.
9. `index.html` still has debug scaffolding (red canvas background, WebGPU probe).
   Crate is still named `bevy_webgpu_demo`.
10. `DESIGN.md` is stale: it documents JSON-array framing, but the protocol (both repos)
    is comma-joined text `op,topic,type,payload`.

### 1.3 Feature gap vs gzweb (by gzweb module)

**Transport.ts / SceneManager.ts**
- Missing ops: `auth` (key), `topics-types`/`topics`, `unsub`, `throttle`, `adv`, `pub`,
  `req` (services), `image` subscriptions.
- Missing: `scene/info` incremental spawn (gzweb: spawn models whose name is new, else
  update id), connection status model (`disconnected`/`connected`/`ready`/`error`),
  disconnect/reconnect, sim control (`/world/<w>/control` WorldControl pause/play,
  `/server_control` stop), generic subscribe/advertise/publish API.
- Note: gzweb does **not** handle `/scene/deletion` or `/pose/info` — not parity gaps,
  but deletion is a worthwhile improvement.

**SDFParser.ts (proto path)**
- Light intensity rule: `1/(1+att_linear) * 1/(1+att_quadratic)` scaled into the type's
  default; spot target at `direction` (default 0,0,−1); directional target at `direction`.
- Geometry: ellipsoid, plane with arbitrary `normal`, heightmap (heights, size, origin,
  first-texture diffuse+normal, repeat = size/tex size), mesh `submesh` + `center_submesh`,
  mesh `scale` applied to the parent visual.
- Collisions spawned hidden; lights under links gated by `enableLights`.
- Particle emitters (three-nebula).
- Material: legacy `normal_map`, PBR maps (albedo/normal/emissive/roughness/metalness),
  `0.4*ambient + 0.6*diffuse` colour rule, opacity → transparent, RGBA texture → alpha test.
- Mesh de-dup/caching per URI, pending-mesh fan-out.
- Offline SDF path (`spawnModelFromSDF`, `<include>`, Fuel `getFiles`) — used by AssetViewer.

**Scene.ts**
- Z-up world, default camera (−6,0,6)→origin, fov 60, near 0.01, far 1000, clear 0xb2b2b2,
  ambient 0x191919, PCF soft shadows, hidden grid (20×20) at z=0.05.
- Orbit controls: left rotate, middle dolly, right pan; orbit target = pick point on
  pointer-down / scroll (gzweb picks against scene geometry, not just the ground plane).
- Camera modes: follow, third-person follow, first-person, move-to (2 s lerp/slerp).
- Picking (raycast to top-level model, skipping helper visuals), selection + bounding box,
  view-as transparent/wireframe, show collisions, toggle lights, joint visuals, sky cubemap
  (default Fuel skybox or DDS `cubemap_uri`), fog, screenshot, thumbnails, `cleanup`.
- Loaders: Collada (with websocket texture fallback), OBJ+MTL (texture path rewriting),
  STL, glTF/GLB, DDS, TGA, textures.

**Smaller modules**: AssetViewer (offline viewer + auto-scale), SpawnModel, Gamepad,
AudioTopic, Publisher, WsLoadingManager progress events.

**Biggest single gap: Collada.** Most Gazebo/Fuel models ship `.dae`.

---

## 2. Architectural decisions

### 2.1 Keep Gazebo's Z-up frame inside a single root entity
Spawn one `GzWorldRoot` entity with `Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2))`
and parent everything Gazebo under it with **raw Gazebo local poses**. Consequences:
- Hierarchy is model → link → visual/light/collision, exactly like gzweb's three.js tree.
- `dynamic_pose/info` updates become "set local Transform", identical to gzweb `setPose`.
- Meshes load untouched (fixes defect 3); primitive builders mirror three's conventions
  (cylinder/capsule/cone rotated so the axis is +Z, plane rotated to `normal`).
- Local transforms in Bevy are directly comparable to three.js `Object3D` locals, which
  makes the parity harness (§4) a straight tree diff. Delete `combine_transforms` and the
  per-geometry Y/Z swaps.

### 2.2 Split into a workspace
```
crates/
  gz-proto/     prost types generated from vendored gz-msgs .proto via protox + prost-build
                (pure Rust, no protoc); replaces hand-written gz_msgs.rs. Keep ign/gazebo/gz
                type-name aliases for msg-type strings.
  gz-core/      NO Bevy. Sans-IO transport state machine (bytes in → events + frames out),
                frame codec, URI/Fuel resolution, light/material/geometry rules, scene
                description model (`SceneNode` tree), pose math. Builds for native + wasm.
  gz-bevy/      Bevy plugin: websocket drivers, asset sources, spawning, pose application,
                camera, picking, UI (bevy_ui), debug visuals.
  gzweb-app/    native binary + wasm entry (`wasm-bindgen` exported `GzWeb` handle).
  gz-parity/    snapshot schema, comparator, replay server, recorder (§4).
```
`gz-core` is where most parity logic lives and is testable without a GPU.

### 2.3 Embedding API (wasm)
Export a `GzWeb` class mirroring `SceneManager`: `new(canvasSelector, {url, key, enableLights})`,
`connect`, `disconnect`, `destroy`, `resize`, `getConnectionStatus`, `onStatus(cb)`,
`getModels`, `select`, `follow`, `thirdPersonFollow`, `firstPerson`, `moveTo`, `resetView`,
`snapshot`, `play`, `pause`, `stop`, `subscribe`, `advertise`, `publish`. Commands flow into
Bevy via a channel resource; events flow out via callbacks. Same API backs native CLI flags.

### 2.4 Non-goals
- **Ogre material scripts** (`<material><script>`, e.g. `Gazebo/Grey`) are not supported
  (decided 2026-10-16). gzweb doesn't support them either.

### 2.5 Intentional divergences from gzweb (do not replicate)
Parity tests allow-list these with a reason string:
- Pose updates matched by **entity id** first, name second (gzweb: first DFS name match).
- Spot penumbra not clamped to ≥ 1; directional shadow map 4096 (not 4094).
- Shadows/materials applied to **all** submeshes, not just the first descendant mesh.
- `<submesh>` follows Gazebo: **every** submesh of that name is kept (gzweb: the first
  three.js mesh, depth first), unnamed nodes' geometry joins the nearest named ancestor's,
  COLLADA nodes are named by `name` only, and `center` centres each submesh's bounds under
  its full transform (gzweb zeroes ancestor translations but keeps their rotations).
- MTL: keys in any case and `-s`/`-o` as in three.js, but `map_Ka` is ignored (gzweb
  rewrites its path, three.js never reads it; gz-common reads only `map_Kd`); absolute and
  URI texture paths are used as they are (gzweb prefixes them with `materials/textures/`);
  `map_Bump`/`bump` are normal maps, as in gz-common (three.js: bump maps). Not done:
  `-bm`, `map_d`, `map_Ks`, per-texture `-s`/`-o` (one `uv_transform` per material, from
  `map_Kd`).
- Click with no hit keeps the orbit target (gzweb resets to origin).
- OBJ meshes actually cached; STL websocket fallback honours submesh.
- STL faces with a zero stored normal take their winding's normal (gzweb keeps
  the zero normal, so they render unlit). ASCII STL starting `SOLID` loads
  (three.js only looks for lowercase `solid`, then reads it as binary and fails).
- An OBJ whose `.mtl` fails to load still shows, with the default material.
- Heightmaps are built from the image Gazebo names (gzweb only uses message heights,
  which Gazebo never sends, so it draws nothing).
- Local mesh paths go straight to the websocket server (gzweb first fetches them over
  HTTP relative to the page).
- Publisher encodes binary protobuf (gzweb's `TextDecoder` round-trip corrupts bytes).
- AudioTopic uses the *new* playback flag.
- Unknown light types fall back to a point light (gzweb logs and adds nothing).
- A zero quaternion becomes identity; a spot with both angles 0 gets penumbra 0
  (gzweb: NaN).
- Sim control uses `gz.msgs.*` types and header-stamped payloads. gzweb's
  `ignition.msgs.*` requests are rejected by Harmonic, and its `play()` does
  nothing at all (`WorldControl { pause: false }` serializes to zero bytes, which
  its `requestService` refuses). See DESIGN.md "Server limits".
- `image` op for topics `topics-types` lists as `gz.msgs.Image` (gzweb checks the
  old `ignition.`/`gazebo.` names, so it always sends `sub`).
- Reconnect with backoff (gzweb stays disconnected). Interactive mode also
  subscribes to `/world/<w>/stats`; snapshot mode keeps gzweb's exact frames.
- A sky cubemap also comes from Harmonic's `Sky.cubemap_uri` field (gzweb reads only
  the header's `cubemap_uri`).
- Rendering does not match gzweb's look (decided 2026-10-17): HDR, tone mapping,
  TAA, SSAO, image-based light, atmosphere, fog and Gazebo's grid. Light
  brightness uses the message `intensity`; the snapshot still compares gzweb's
  derived intensity. See DESIGN.md "Rendering".

---

## 3. Implementation plan (phased; each phase ends with its parity suite green)

**Phase 0: Foundation (1–2 wk)**
- Workspace split (§2.2), rename crate, generated protos, fix defects 1, 6, 7, 9, 10.
- `GzWorldRoot` + hierarchy (§2.1), fixing defects 2 and 3.
- Start `WORKLOG.md`; update `DESIGN.md`.
- Build the parity harness skeleton (§4.2–4.4) *before* adding features.

**Phase 1: Protocol parity** (done 2026-10-17; `scene/deletion` not done:
gzweb doesn't handle it either)
- Full op set: auth, protos, topics-types, worlds, scene, sub/unsub, throttle, adv, pub,
  req, asset (+ `StringMsg` error replies), image.
- Status state machine, reconnect, URL + key config (`?url=`/`?key=` on wasm, CLI on native),
  `wss://` (tungstenite + rustls on native).
- `scene/info` incremental spawn; optionally `scene/deletion`.
- Sim control services.

**Phase 2: Scene graph and lighting** (done 2026-10-17). Decided: render with
Bevy's best quality instead of matching gzweb's look; see DESIGN.md "Rendering".
- Light rules (intensity formula, ranges, spot angles/falloff, direction targets,
  `cast_shadows`, `is_light_off`, `enableLights`).
- Ambient, clear colour, default camera, shadows, grid.
- Sky: default Fuel cubemap and DDS `cubemap_uri`. Fog.
- `visible`, `cast_shadows`, collisions hidden.

**Phase 3: Geometry and assets**
- Primitives: ellipsoid, plane normal (done in Phase 2). Heightmap (done 2026-10-17 from
  the image, ≤1025 vertices a side, texture repeat; layer blending by height done
  2026-10-18, up to 4 layers as in ogre2, see DESIGN.md "Heightmaps").
- URI resolution identical to gzweb: strip `file://`/`model://`, `createFuelUri`, HTTP first
  → websocket fallback on all platforms (one `AssetReader` doing the chain), per-URI cache
  and de-dup. (Done 2026-10-17; local paths skip HTTP, see DESIGN.md "Assets".)
- **Collada** (done 2026-10-17, see DESIGN.md "Assets"): `gz-collada` (mesh-loader's
  parsers, vendored, with a rewritten scene assembly) as a Bevy `AssetLoader`. `<unit>` is
  applied; `<up_axis>` is not, as in gz-common and gzweb. Not done:
  `<instance_controller>` (skinned meshes): skipped, with one warning per file.
- OBJ/MTL texture path rewriting (`model://`, bare `map_Kd` → `materials/textures/`)
  (done 2026-10-17: own loader on `tobj`, see DESIGN.md "OBJ").
- Submesh selection + centering (done 2026-10-17, Gazebo's rules, see DESIGN.md
  "Submeshes"), mesh scale (done in Phase 2).

**Phase 4: Materials**
- gzweb colour rule, opacity/blend, alpha-mask for RGBA textures, double-sided.
- PBR maps (albedo, normal, emissive, metallic-roughness), legacy `normal_map`.
- A visual's SDF `<material>` and PBR maps on a `.dae` mesh: ignored now (the file's
  own materials are used).
- Texture loading via the same HTTP→websocket chain. DDS/TGA via Bevy's image features.
- Decide and document the colour space: gzweb feeds raw floats to three r141 with no
  sRGB conversion. Pick the Bevy equivalent so rendered colours match.

**Phase 5: Interaction and UI (Bevy + bevy_ui)**
- Camera: orbit/dolly/pan mapping as gzweb, pick-point orbit target (mesh raycast via
  `bevy_picking`/`MeshRayCast`), reset view, follow / third-person / first-person / move-to.
- Selection + bounding box, view-as normal/transparent/wireframe (`WireframePlugin` is
  native-only, so use a line-mesh fallback on wasm), show collisions, toggle lights, joint
  visuals, grid toggle.
- bevy_ui panels: connection status, play/pause/stop, entity tree, view toggles, loading
  progress (WsLoadingManager equivalent). Evaluate `bevy_feathers` widgets if stable in 0.20.
- Screenshot (Bevy `Screenshot`), `GzWeb` embedding API (§2.3).

**Phase 6: Extras**
- Gamepad (Bevy gilrs), AudioTopic (bevy_audio), Publisher.
- Particle emitters: CPU sprite system, because GPU particle crates need compute, which
  WebGL2 lacks.
- AssetViewer mode: SDF XML (`roxmltree`), `<include>`, Fuel `getFiles`, bearer token,
  auto-scale. SpawnModel. Thumbnails.

**Phase 7: Performance and packaging**
- Benchmarks (§4.8), wasm size budget (`wasm-opt`), a WebGL2 fallback build (Bevy's
  `webgpu` feature overrides `webgl2`, so this needs a second wasm build chosen by
  the page when `navigator.gpu` is absent), release
  pipeline (native binaries + npm package wrapping the wasm `GzWeb`).

---

## 4. Testing plan: differential testing against gzweb

Principle: **same input bytes → both implementations → canonical snapshot → diff.**
Gazebo is only needed to *record* inputs. Every test run replays recordings, so CI is
deterministic and needs no simulator.

### 4.1 Input corpus (`tests/corpus/`)
- **Recorded sessions.** Use `Dockerfile.gazebo` with worlds: `empty.sdf`, `shapes.sdf`,
  `lights.sdf`, `pbr.sdf`/material worlds, `heightmap.sdf`, `particle_emitter.sdf`,
  `visibility`/`shadows`, a mesh-heavy Fuel world (DAE + OBJ + STL + GLB + textures),
  `nested_model.sdf`, plus a moving-robot world for pose streams.
  The recorder is a transparent WS proxy that logs every frame in both directions with
  timestamps and direction to `session.wslog` (length-prefixed binary). It also captures
  every `asset` reply and HTTP-fetched file into `assets/` keyed by URI.
- **Synthetic scenes.** JSON → protobuf `Scene` via a small encoder (protobufjs or prost),
  targeting edge cases: zero/missing poses, non-unit quaternions, ±0, NaN-free extremes,
  nested models 4 deep, duplicate names, every geometry type, every light type with every
  attenuation combo, materials with/without ambient/PBR/maps, transparency 0/0.5/1,
  submesh+center, `is_light_off`, `delete_me`, collisions.
- **Malformed frames.** Truncated headers, unknown ops, bad protobuf, `StringMsg` asset
  errors, text vs binary proto definitions, `authorized`/`invalid`.

### 4.2 Replay server (`gz-parity replay`)
A WS server that plays a `.wslog` back *reactively*: it answers `protos`, `worlds`, `scene`
and `asset` requests from the recording, serves captured HTTP assets on a local port
(Fuel host rewritten to it), and streams `pub` frames after `sub` in recorded order. A
`--step` mode only advances on a control command, so both clients can be frozen at
message *k*. It **logs every client→server frame** for the protocol test (§4.5).

### 4.3 Canonical scene snapshot (`gz-parity` schema, JSON)
Tree keyed by Gazebo name path. Per node:
- `kind`, local `pos`/`quat`/`scale`, and derived world transform.
- `visible`, `castShadow`, `receiveShadow`.
- `geometry`: type, params, vertex/triangle count, local AABB, submesh names.
- `material`: base colour, opacity, transparent, metalness, roughness, double-sided,
  resolved texture URIs per slot.
- `light`: type, colour, intensity (post-mapping), range, angles, falloff, target direction.

Scene level: ambient, clear colour, camera pose/fov, grid visibility, sky source, fog.
Both exporters emit **only Gazebo-derived nodes**, skipping helpers (light helpers,
bbox, grid internals).

### 4.4 Exporters and harness
- **gzweb side.** Playwright drives headless Chrome (SwiftShader WebGL) on a tiny page that
  imports `../gzweb` (built `dist/gzweb.module.js`) and starts `SceneManager` against the
  replay server. `page.evaluate(dumpScene)` traverses `scene.scene` into the schema
  (three.js → Gazebo axes are identity since gzweb is Z-up). This exercises the *real*
  gzweb including loaders, with no mocking.
- **Rust side.** A `--snapshot <out.json>` mode for the native app, plus a test-only
  headless Bevy app (`MinimalPlugins` + asset + gz-bevy spawning, no renderer). Both walk
  the `GzWorldRoot` subtree into the same schema. The wasm build gets a `GzWeb.dumpScene()`
  export so the *same Playwright run* can snapshot the wasm build too.
- **Comparator.** Match nodes by name path, unordered children; floats ε = 1e-5 (pose),
  1e-3 (derived AABBs); quaternions compared up to sign; colours ε = 1/255. Reports:
  missing/extra nodes, field diffs, and diffs suppressed by the §2.5 allow-list (each must
  cite a reason id). Output is a JSON + HTML report artifact.

### 4.5 Test layers

| # | Layer | Input | Compared | Runs |
|---|---|---|---|---|
| L1 | **Pure-function tables** | shared `tests/fixtures/*.json` cases | gzweb functions (`createFuelUri`, `parsePose`, `parseColor`, light intensity, colour rule, frame split, ellipsoid/plane transforms, `useSubMesh` centering) run via a Node/vitest harness importing `../gzweb/src` **vs** the `gz-core` equivalents in `cargo test` | every PR |
| L2 | **Protocol conformance** | each corpus session via replay | ordered client→server frame log of gzweb vs Rust (ops, topics, payloads; timing ignored); status transitions | every PR |
| L3 | **Static scene parity** | each session, after `scene` processed + assets settled | snapshot diff (§4.3) | every PR |
| L4 | **Dynamic parity** | pose-stream sessions in `--step` mode | snapshots at checkpoints k = 1, 10, 100, last; world transforms of every link | every PR |
| L5 | **Mesh-loader parity** | every mesh in the corpus | three.js loaders in Node (jsdom DOMParser for Collada) vs Rust loaders: vertex/tri counts, AABB, submesh names, material count, texture refs | every PR |
| L6 | **Visual parity** | key sessions, fixed 1280×720, gzweb default camera | (a) "parity render mode" on both sides: unlit flat per-entity ID colours → silhouette IoU ≥ 0.98 per entity; (b) lit screenshots → SSIM against a threshold (informational, not gating) | nightly |
| L7 | **Robustness** | malformed-frame corpus + fuzzing (`cargo fuzz` on the frame codec and Scene decode) | no panics; Rust handles every case gzweb handles | nightly |
| L8 | **Live smoke** | real Gazebo in Docker; also re-records the corpus | both clients reach `ready`, snapshots diff clean | nightly / manual |

Unit tests inside each crate (frame codec, URI rules, state machine transitions) sit
alongside L1. wasm-specific code is covered by `wasm-bindgen-test --headless --chrome`.

### 4.6 Bootstrapping the gzweb side
`../gzweb` has no `node_modules` and only 3 trivial jest tests. The harness lives in
`gz-parity/js/` (its own `package.json`, Node 24 + Playwright), depends on `../gzweb` by
path, and runs `npm ci && npm run build` in gzweb once. **No changes to gzweb itself are
required.** gzweb is pinned to a commit (currently `519d4cf`) recorded in the corpus manifest.

### 4.7 Acceptance criteria per phase
A phase is done when its corpus sessions pass L1–L5 with zero un-allow-listed diffs and
the L6 silhouette IoU threshold holds. Track a parity scoreboard (sessions × layers) in
the report and paste the summary into `WORKLOG.md`.

### 4.8 Performance comparison (the reason for the port)
Same replay sessions, same Chrome, measured via Playwright tracing / `performance` marks:
- Time from WS open to `ready` and to all assets loaded.
- Scene decode + spawn time.
- Steady-state frame time p50/p95 with N = 10/100/1000 models.
- Pose-update throughput (Pose_V msgs/s before frame time degrades).
- JS heap vs wasm memory. Bundle size.

Native: criterion benches on `gz-core` (decode, frame parse, pose apply). Regressions
greater than 10% fail the nightly job.

### 4.9 CI layout
- PR: `cargo fmt/clippy/test` (native), `wasm32` build + `wasm-bindgen-test`, then L1–L5 on
  the recorded corpus (Playwright container with Chrome).
- Nightly: L6–L8 + perf; the live Gazebo job may refresh recordings into a PR branch for
  review. Never auto-merge.

---

## 5. Immediate next steps

> **Done 2026-10-16:** all four. See WORKLOG.md. The next work is to grow the
> corpus, then Phase 1, then Collada.
1. Fix defect 1 (dropped message) and defect 6 (busy-poll) — small and isolated.
2. Workspace split + generated protos + `GzWorldRoot` hierarchy.
3. Build the recorder, replay server and both snapshot exporters; record `shapes.sdf` and
   `empty.sdf`; get L2/L3 running end to end on those two before expanding features.
4. Collada spike with `mesh-loader` against the corpus meshes (L5).
