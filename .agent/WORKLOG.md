# Worklog

Dated entries: what changed, and what was verified. Newest first.

## 2026-10-16: CI, quality gate, coverage

- **Gate:** `scripts/check.sh` (fmt, clippy native + wasm32, rustdoc,
  cargo-deny, coverage ≥ 95% lines), used by `.pre-commit-config.yaml` and
  `.github/workflows/ci.yml`.
- **CI jobs:** lint, coverage, cargo-deny with advisories, wasm build, and
  browser parity against gzweb pinned at `519d4cf`.
- **Lint policy:** `[workspace.lints]` plus `clippy.toml`. Production
  `unwrap`s were replaced; a poisoned asset-store lock is now recovered.
- **cargo-deny:** path dependencies now carry versions. Three advisories are
  ignored with reasons in `deny.toml`:
  - ruzstd OOM: the fix needs ruzstd 0.9, but Bevy (0.18 and 0.20) requires 0.8;
  - paste, unmaintained;
  - ttf-parser, unmaintained.
- **Testability refactors:**
  - gz-parity is now a lib with a thin bin, and has `corpus::Builder`,
    `replay::serve_on`, replay `--key`, and record `--timeout-ms`;
  - gzweb `parse_args` and `default_plugins` are pure;
  - `hud::adapter_style` is a pure function.
  - The orbit controls are unchanged.
- **Tests:** 130, up from 26.
  - Line coverage is **99.7%** (5314 lines, 17 missed), up from 37.7%.
  - `crates/gzweb/tests/headless.rs` runs the real binary headless against
    in-process replay. The shapes/empty corpora are diffed against the
    committed gzweb references in `tests/corpus/*/gzweb.*`.
  - A synthetic world covers all primitives, OBJ/STL/glTF over the websocket
    (glTF with an external buffer) and OBJ over HTTP. This is the first
    verification of mesh loading. It also covers lights, `#n` paths, poses by
    id and by name, `scene/info` additions, and failures: auth, refusal,
    mid-session close.
- **Uncovered, by design:**
  - the TLS branch in record;
  - `accept()` failure in replay;
  - a reset during a read in the native driver;
  - the HUD branch that needs a real `RenderAdapter`;
  - wasm-only code (not compiled natively).
- **New divergences found:** unknown light type, zero quaternion, spot with
  zero angles (PORT_PLAN §2.5).

## 2026-10-16: Camera controls match gzweb

- **Bug:** a left click re-centred the view on the clicked point.
  `bevy_panorbit_camera` always aims the camera at its focus.
- **Fix:** replaced it with `GzOrbitControls`, a port of gzweb's patched
  OrbitControls. Clicking only picks the pivot; dragging orbits the camera
  about it, without re-aiming. Picking uses Bevy `MeshRayCast`, falling back
  to the ground plane. See DESIGN.md "Camera controls".
- **Verified:** by hand against live Gazebo `shapes.sdf` (native, by the user).
  Tests, clippy and the wasm32 build pass.

## 2026-10-16: Foundation (PORT_PLAN.md §5 next steps 1–4)

Branch `feature/port-foundation`.

### 1. Bug fixes in the original single-crate code (commit `a0871df`)

- **Lost message on auth reply.** A binary `authorized`/`invalid` reply called
  `try_recv()` again and silently dropped the next message.
- **Asset payloads were never decoded.** Replies are serialized
  `gz.msgs.Bytes`, with `gz.msgs.StringMsg` for errors (confirmed in
  gz-launch `WebsocketServer.cc`). The old code treated the payload as raw
  file bytes.
- **The asset reader busy-polled.** It looped on `yield_now()` up to 6000
  times. It now waits on a channel, and errors wake it.

### 2. Workspace split

- **New crates:** `gz-proto`, `gz-core`, `gz-bevy`, `gzweb` and `gz-parity`.
  See DESIGN.md.
- **Generated protobuf types.** gz-msgs Harmonic protos, commit `292056c`,
  are vendored and compiled with protox. The old hand-written field numbers
  were correct, but generation covers every message.
- **New core design.**
  - Transport is a sans-IO state machine: one implementation for native and
    wasm, unit-tested without a socket.
  - A `GzWorldRoot` with the real Gazebo hierarchy replaces the flattened
    world transforms.
  - This fixes pose updates (they were applied as world transforms onto
    visuals) and mesh orientation.
- **gzweb rules ported to `gz_core::scene`:**
  - geometry dispatch order, plane normal, ellipsoid;
  - `<mesh><scale>` applies to the visual;
  - the colour rule `0.4·ambient + 0.6·diffuse`;
  - light intensity `1/(1+L)·1/(1+Q)`, spot angle/decay, direction targets;
  - `createFuelUri`;
  - stripping `file://` and `model://`.
- **New behaviour:**
  - Configurable URL and auth key (previously hard-coded to localhost).
  - Assets fetched over HTTP or the websocket on every platform.
  - Model visuals in the gzweb defaults: camera (-6,0,6) with fov 60, clear
    colour `0xb2b2b2`, ambient `0x191919`.
  - Headless `--snapshot` mode.
- **Not yet ported:** Collada, textures/PBR maps, heightmap, sky, grid,
  picking, camera modes, collisions.
- **Verified:**
  - `cargo test --workspace`: 26 tests pass. That covers gz-core frame,
    transport, uri and scene, the gz-bevy asset store and paths, and the
    gz-parity comparator and corpus.
  - Native and `wasm32` builds succeed.
- **Leftover:** the old `src/` stayed on disk until 2026-10-16, when it was
  removed.

### 3. Parity harness

- **Spec:** PARITY.md.
- **Tools:**
  - `gz-parity record`, `replay`, `compare` and `compare-protocol`.
  - `parity/js` loads the real gzweb in headless Chromium (Playwright 1.60,
    SwiftShader) and dumps the reference snapshot.
  - `parity/run.sh` ties them together.
- **Corpus:** `tests/corpus/{shapes,empty}`, recorded from the Harmonic
  Docker image (`Dockerfile.gazebo`) with 100 `dynamic_pose/info` frames each.
- **Results:**

  | Corpus | Snapshot | Protocol | Suppressed diffs |
  |---|---|---|---|
  | shapes | PASS (22 nodes) | PASS | 2 |
  | empty | PASS (4 nodes) | PASS | 1 |

  The suppressed diffs are the allow-listed gzweb quirks listed below.
  - The ellipsoid's world pose after the pose stream matches, so pose
    application agrees.
  - The bounding boxes of every primitive and the ground plane match.
  - The sun's world direction matches to 1e-6.
- **Harness fix:** the gzweb dumper now uses `Box3.setFromObject(obj, true)`.
  The default transforms the local box, which over-estimates rotated meshes
  (the ellipsoid showed it).

### 4. Collada spike

- **Location:** `spikes/collada/REPORT.md`.
- **Finding:** mesh-loader parses correctly, with triangle counts identical to
  three.js on 14 Fuel meshes. Its scene assembly is wrong:
  - parent/child order is reversed;
  - `<unit>` is not applied to translations;
  - only one material per geometry;
  - no node names, so `<submesh>` can't work.
- **Recommendation:** vendor its `collada` module, rewrite the instancing
  layer, and emit a Bevy `Scene`. Estimate 6–8 days.

### Browser smoke test

- **Result:** the release wasm build (`./build.sh`) works in Playwright
  Chromium with hardware WebGPU (Vulkan) against `gz-parity replay` of the
  shapes corpus.
  - It reaches Ready and renders the scene at 60 FPS.
  - It sends the same client frames as gzweb and native gzweb-rs.
  - Shape positions and orientations match gzweb's screenshot. Shading differs
    until the Phase 2 light calibration.
- **Not possible headless:** SwiftShader's software WebGPU rejects a buffer
  wgpu creates, so the smoke test can't run in a GPU-less browser.
- **No WebGL2 fallback.** With WebGPU disabled, the build panics with "Unable
  to find a GPU": Bevy's `webgpu` feature overrides `webgl2` on wasm. The
  fallback the docs claimed never existed. It needs a second build (Phase 7).

### gzweb behaviours confirmed against real Gazebo

These are findings for the plan, not changes:

- **Light colour:** gzweb lights are always white. The `gz.msgs.Color` object
  passed to `new THREE.*Light()` is ignored. Allow-listed.
- **Background:** gzweb ignores `<scene><background>`. gzweb-rs honours it.
  Allow-listed.
- **Spot penumbra:** gzweb uses `max(1, …)`, so it is always ≥ 1. gzweb-rs
  keeps three's [0, 1] range. Allow-listed (`lights` corpus).
- **Ignored fields:** gzweb ignores `Visual.scale`, `Visual.transparency` and
  model-level visuals. gzweb-rs matches the first; the other two are not
  exercised yet.
- **Camera-sensor worlds don't load in gzweb.** In worlds with a camera
  sensor (e.g. `sky.sdf`), gzweb's scene decode throws
  `no such Type '.gz.msgs.PixelFormatType'`, so it never reaches `ready`.

### Next

- **Corpus:** done 2026-10-16, see the entry below.
- **Phase 1:** reconnect, `wss://` (the native driver has no TLS yet), and
  sim-control services.
- **Collada:** implement it per the spike's recommendation.

## 2026-10-16: Corpus growth

- `parity/record.sh` records a world from the Gazebo image in one step.
- Recorded 10 more worlds; status table in PARITY.md §1. Six now pass and run
  in `cargo test`.
- **Sphere tessellation:** gzweb-rs now builds spheres and ellipsoids
  vertex-for-vertex like three.js `SphereGeometry(r, 32, 32)`. Bevy's UV
  sphere put rotated sphere bounds 1.5 mm off gzweb's.
- **Spot penumbra** is allow-listed (`gzweb-spot-penumbra-clamp`).
- **gzweb fails on four worlds:** the `PixelFormatType` decode error (camera
  sensors) and a null `albedo_map` in `particle_emitter`. No reference is
  possible until gzweb is fixed.
- **Gaps found:** heightmaps, Collada (`import_mesh`), and HTTP asset capture
  (Fuel meshes and textures are fetched live, so those worlds aren't
  hermetic).

## 2026-10-17: Fuel assets without committing them

- `gz-parity mirror`: a caching HTTP mirror with SHA-256 lock files; spec in
  PARITY.md §1. Measured cost of committing the files instead: 25 MB for
  three worlds, so they stay out of the repo.
- gzweb-rs `--http-mirror URL` (wasm: `?http_mirror=`); HTTP asset URLs now
  encode spaces.
- `gaps.toml` per corpus for not-yet-ported features; `heightmap` and
  `import_mesh` now pass with gaps.
- CI: a separate `fuel` job; the browser parity job covers all six offline
  worlds and never contacts Fuel.
- Fixed `parity/run.sh`'s exit trap, which `set -e` cut short and left the
  second server running.

## 2026-10-17: Phase 1, protocol parity

- **Op set:** `topics`, `image`, `adv`, `pub_in`, `req` and service replies;
  gzweb's send gating (`SendError`); frames are bytes, sent as text when
  UTF-8 and binary otherwise.
- **Probed gz-launch on Harmonic** (and read its source, 7 to `main`): payloads
  are cut at `0x00` and split on `,`, an empty `req` crashes the server, and
  there's no `adv`/`pub_in`. gzweb's play/pause/stop don't work on Harmonic
  (`ignition.msgs.*` types are rejected). Written up in DESIGN.md "Server limits".
- **Sim control:** `gz_core::control` plus the `GzCommand` message; HUD sim
  line from `/world/<w>/stats`; Space and N keys. Pause and unpause verified
  live.
- **Reconnect** with backoff; a new scene replaces the old one. End-to-end
  test against a replay server that drops the first connection
  (`ReplayOptions::drop_first_after`).
- **`wss://`** on native via rustls (`gz_core::net`), `--ca-cert` for private
  CAs; the recorder uses it too. Tested with a generated CA.
- Replay answers `req` and `topics` and reads binary client frames.
- Not done: `scene/deletion` (gzweb doesn't handle it either).

### PR #19 review fixes (2026-10-17)

- `control::step` splits counts that encode with `0x00`/`,`; every count to
  20 000 is tested sendable.
- `Transport` validates every frame: no `,`/`0x00` in any field, no empty
  field the server indexes, no `throttle` rate 0. Publishes on topics we
  aren't subscribed to are dropped (as gzweb does).
- Connect/handshake timeouts (`net::CONNECT_TIMEOUT`,
  `connection::HANDSHAKE_TIMEOUT`); invalid URLs and rejected keys are
  final and keep their reason.
- Drivers close their socket and stop when dropped; on wasm one `Closed` per
  failure, and the callbacks are freed with the socket.
- `GzPublish` and `GzServiceReply` messages; world topics matched exactly.
- The replay server drops its first connection at the first `sub`, not on a
  timer, so the reconnect test no longer depends on timing.
- `frame::build` takes bytes (the old `String` version is gone).

## 2026-10-17: Phase 2, rendering

- **Decision:** don't match gzweb's rendering; use Bevy's best. Calibrating
  to three.js legacy units first showed how far gzweb's look is from good
  (ambient 1.0 with no tone mapping clips most worlds to white).
- `render.rs`: HDR + TonyMcMapface (AgX looked flat on Gazebo's saturated
  primaries) + bloom, TAA, SSAO, 4096 px cascaded shadows, image-based light
  (procedural studio cubemap, or the sky's), Bevy's atmosphere for skies
  without a cubemap, distance fog, Gazebo's grid. `--quality low`.
- `--screenshot` for visual review (`target/visual/*.png` locally).
- `gz-core`: fog parsed; the sky cubemap falls back to Harmonic's
  `cubemap_uri` field.
- **Environment switching:** a camera that changes environment-map source
  fails wgpu validation, because Bevy's derived `EnvironmentMapLight` and the
  atmosphere's private probe state stay on it. Fixed by respawning the camera
  per environment (PR #20 review).
- Not verified: a DDS cubemap sky (no corpus world has one), and the browser
  build of these effects (WebGPU should support all of them).

### PR #20 review fixes (2026-10-17)

- Camera respawned per environment (the real cause of the "mip-count bug");
  identical scenes on reconnect are a no-op. Lifecycle test covers studio →
  atmosphere → failed cubemap → studio.
- Cubemaps are checked once loaded (6 square power-of-two faces) and fall
  back to the studio; tracked in `GzLoads` for screenshots.
- Ambient colour only from the scene (gzweb's default otherwise); fill light
  driven by `<ambient>`, 64 px sRGB studio map, a 5% ambient floor, no
  atmosphere without a sun, no environment maps without a GPU or at Low.
- Fog only when the scene has it (no divide-by-zero "off" fog).
- Quality: Low / High (2048 px shadows) / Ultra (4096 px).
- `--screenshot`: own save (creates the directory, exit 1 on failure), the
  timeout covers a capture that never arrives, `.png` only.
- `GzConfig.headless` split into `interactive` and `gpu`; spot lights use
  `spot_inner_angle`; unset light intensity means 1.

## 2026-10-17: Collada

- New crate `gz-collada`: mesh-loader 0.1.13's COLLADA parsers vendored
  (`src/vendor/`, changes marked `gz-collada:`), scene assembly rewritten
  (`src/assemble.rs`). Fixes the spike's four bugs: parent-first transforms,
  `<unit>` as a root scale, a mesh and material per primitive, instances
  rather than geometries. Adds node names/ids, `<library_nodes>` +
  `<instance_node>`, `<bump>` in `<extra>`, and errors instead of panics on
  bad indices. Fixed upstream's `<double_sided>`/`<faceted>`/`<wireframe>`
  reading a missing attribute (failed the whole parse).
- Verified: all 14 Fuel meshes and the 3 probes match three.js triangle
  counts and world bounds; `import_mesh` passes browser parity with no gaps.
- `gz_bevy::collada::ColladaLoader` → Bevy `Scene` (labelled meshes and
  materials, sRGB colour and linear normal textures, tangents for normal
  maps). The pink placeholder is gone.
- Coverage: the vendored code is excluded (`COVERAGE_IGNORE` in
  `scripts/check.sh`); `assemble.rs` and the loader are gated.
- Not done: `<submesh>`/`center`.

### PR #21 review fixes (2026-10-18)

- **No panics on bad `.dae` files** (they aborted the wasm client): an XML
  nesting pre-scan (roxmltree recurses; 300 levels), `<node>`s capped at
  256, untrusted counts no longer size allocations, the iterators yield
  nothing instead of panicking, and `validate.rs` checks each instantiated
  primitive's sources, accessors and `<p>`. A `<Name_array>` hung the
  parser (an upstream loop that never advanced); fixed.
- `<instance_node>`: ancestor-stack cycle detection, a map lookup, and a 1M
  node/primitive budget; plain nesting is no longer cut at 65.
- Dangling scene references fail the load, as in three.js; VERTEX reads the
  mesh's own `<vertices>`; polylists skip 1–2-vertex polygons correctly;
  tristrips are strips; `<extra>` is best effort and its `<double_sided>`
  is read; `<unit>` ≤ 0/NaN → 1; `<rotate>` axes normalised.
- Loader: failed textures are dropped from materials (any
  `StandardMaterial`), textures repeat, roughness `(2/(n+2))^¼`, Lambert
  rough, reflectance from specular, opacity-only alpha, cut-outs masked,
  emission textures glow, one `units::emissive` for COLLADA and SDF (SDF
  emission was ~0), vertex colours linearised, no index buffer.
- HTTP: 256 MB bodies, 5-minute whole-fetch timeouts (native and wasm), a
  2-minute one in the mirror; CI saves the Fuel cache only when it changed.
- Verified: the review's reproductions are tests in `gz-collada`; the
  probes match three.js (triangles and bounds) with `run.sh`'s reference.
- Mirror: a Fuel file the lock pins falls back to Fuel's old domain,
  fuel.ignitionrobotics.org (same service, verified byte-identical), when
  fuel.gazebosim.org fails. CI's Fuel job passes during Fuel outages; the
  lock still checks every body.

## 2026-10-17: Phase 3 — submeshes

- Fuel CI job on PR #21 hung: the HTTP cache (keyed on lock files only) was
  saved before `.dae` loads existed, so the mesh and texture came from
  fuel.gazebosim.org, which stalled (502 since). Fetches now time out
  (mirror and client) and the CI cache is saved every run.
- `gz_bevy::submesh`: `<submesh>` + `center` for scene meshes (COLLADA,
  glTF), Gazebo's rules. Unit tests on a hand-built scene; headless test
  cuts the probe `.dae` to `ChildNode`, centred, and checks an unknown name
  leaves nothing.
- OBJ: `gz_bevy::obj` on `tobj` replaces `bevy_obj` (which kept no
  object names and resolved textures only relative to the `.mtl`). gzweb's
  MTL texture rules, MTL → PBR, flat normals when missing; headless test
  with two objects, two materials and a submesh.
- Assets: a failed `gzhttps://` fetch retries through the websocket
  `asset` request with the real URL (not the mirror's), as gzweb does.
  Headless test serves a mesh only over the websocket.
- Heightmaps: `GeometryDesc::Heightmap`, terrain from the image (Gazebo's
  rules) or message heights; first layer textured. `heightmap` corpus:
  material matches gzweb, geometry allow-listed (`gzweb-heightmap-empty`).
- Websocket fallback requests the unescaped URL (`source_url`), as gzweb.
- JPEG and TGA textures enabled (glTF/COLLADA models with JPEGs loaded
  untextured).

## Bevy 0.20

- Bevy 0.18 → 0.20; `rust-toolchain.toml` pins rustc 1.97.1 (Bevy's MSRV).
- Renames: `Scene`/`SceneRoot`/`SceneInstanceReady` →
  `WorldAsset`/`WorldAssetRoot`/`WorldInstanceReady` (`Scene` is now the
  BSN trait), `load_with_settings` → `load_builder().with_settings()`,
  `resolve_embed(&str)` → `resolve_embed_str`, `shadows_enabled` →
  `shadow_maps_enabled`, `FontSize::Px`, `Hdr` in `bevy::camera`.
- `Atmosphere` is now a planet entity (its transform is the planet's
  centre), not a camera component: on the camera it put the camera at the
  planet's centre (dark, orange, no shadows). `render::GzAtmosphere` spawns
  it at Bevy's default (below the origin); the camera keeps
  `AtmosphereSettings`.
- `bevy_stl` has no 0.20 release: own `gz_bevy::stl` parser, after
  `stl_io` proved too strict (binary files whose header starts `solid `
  read as ASCII; bare `solid` lines failed). Binary vs ASCII as three.js
  decides it; non-indexed, 3 vertices per face. Faces with zero normals
  use their winding (they rendered unlit before).
- One `ScatteringMedium` for every atmosphere; tests check the planet's
  position.
- `wgpu-types` dropped: Bevy doesn't re-export `wgpu::Backend`, so the HUD
  matches the backend's `Debug` name.
- CI names toolchain 1.97.1; cargo-deny runs on minimal stable.
- Tests with `GizmoPlugin` need `MeshPlugin` (gizmos draw skinned bounds).

## 2026-10-18: Heightmap image in the lock

- `heightmap_bowl.png` added to `tests/corpus/heightmap/http.lock.json`,
  so `heightmap_matches_gzweb` passes offline. fuel.gazebosim.org still
  answered 502; the bytes came from fuel.ignitionrobotics.org (same Fuel
  server, which served all six already-locked files with matching hashes,
  and the image equals gz-sim's `test/media/heightmap_bowl.png`). Seeded
  into `target/http-cache`; the mirror wrote the lock entry.
- CI's HTTP cache doesn't have the image yet; while fuel.gazebosim.org is
  down the mirror takes it from fuel.ignitionrobotics.org (the lock pins it).

## 2026-10-18: PR #22 review fixes

- Merged #21's review fixes; OBJ now uses its material rules (roughness
  `(2/(n+2))^¼`, `units::emissive`, sRGB vertex colours, repeating textures).
- `.DAE` loaded as OBJ (Bevy matches extensions case-sensitively and fell
  back to the last `Scene` loader); `.GLB` found no loader (a labelled
  path). One `mesh_file::SceneFileLoader` for COLLADA and OBJ, glTF loaded
  as a `Gltf`; headless test with `.GLTF`, `.DAE`, `.Obj`, `.STL`.
- `mesh_file`: the loaders' shared texture paths/loading, Phong roughness
  and mesh tail. Textures climbing out of their source are refused (Bevy
  panicked in the loader).
- `<submesh>`: Gazebo's rules, checked against gz-common (COLLADA, Assimp,
  OBJ loaders) and gz-rendering's `Ogre2MeshFactory`: every match, unnamed
  nodes join the named ancestor, COLLADA `name` only, each submesh centred
  on its own bounds; idempotent on respawn.
- Heightmaps: the brightest pixel is at `size.z` (gz `MaxElevation`);
  layer textures tracked for snapshots; image CPU-only; grid tangents only
  under a normal map (1025²: ~14 ms instead of 3 s); snapshot corners and a
  headless orientation check.
- OBJ: absolute/URI texture paths through the sources, MTL keys in any
  case, `-s`/`-o` → `uv_transform`, `map_Ka` dropped, gzweb's
  `/materials/textures` rule matched; tobj's unused `async` feature off.
- Assets: hosts that fail to connect or time out are skipped for 5 min;
  unanswered websocket `asset` requests fail after 60 s.

## 2026-10-18: PR #22 review fixes on Bevy 0.20

- Merged #22 (with #21's review fixes) into the 0.20 branch: the shared
  `SceneFileLoader` and `GzSubmesh` work on `WorldAsset`/`WorldInstanceReady`,
  `AssetMut` needs a mutable borrow to mark a change, and `#23`'s own STL
  loader stays.

## 2026-10-18: Phase 4, materials

Branch `feature/phase-4-materials`. Materials follow Gazebo (gz-sim
`SceneManager`, gz-rendering `Ogre2Material`, ogre-next PBS shaders, read in
`~/work/gazebo/harmonic`), not gzweb. See DESIGN.md "Materials".

- **Rules verified in the sources:**
  - ogre2 uses diffuse only (ambient is stored, never rendered); opacity is
    `(1 − transparency) × diffuse.a`; the `Transparent` mode scales the
    diffuse by α² and F0 by α;
  - an albedo map turns on an alpha test at 0.5 (if the texture has alpha)
    and two-sided lighting from `double_sided`, and only then;
  - the metalness and roughness maps are sampled `.x`; `kS` (specular)
    multiplies every specular term; dielectric F0 is 3%;
  - an emissive map is multiplied by the emissive colour only when that is
    non-zero; a light map goes in the emissive slot, × diffuse;
  - a visual without a material: primitives get `gz-grey` (metalness 1,
    roughness 0.2), meshes keep theirs (STL: ogre2's white default);
  - a mesh visual with a `<material>`: `BaseMesh::SetMaterial` puts it on
    every submesh; without: transparency multiplies each submesh's;
  - colours go to the shaders unconverted and render to sRGB targets:
    linear.
- **gz-core:** `MaterialDesc` carries the whole message (diffuse, ambient,
  specular, emissive, render order, legacy normal map, PBR workflow and every
  map as a load URI). gzweb's colour rule is gone.
- **gz-bevy:** new `material` module (rules above, metalness/roughness
  combine, scene-mesh overrides via `WorldInstanceReady`). Primitives get
  tangents and UV1 when they need them. COLLADA, OBJ, vertex colours, light
  colours and `units::emissive` are linear. Snapshots report linear colours
  and, gzweb-rs only, the emissive, metallic-roughness, occlusion and light
  maps; they wait for the combines.
- **Tests:** unit tests per rule (colour, specular, opacity and blend modes,
  defaults, combine, tangents/UV1, override and fade on an instanced scene,
  map loading through the asset sources, DDS and TGA); the synthetic
  headless world gained a PBR box with every map over websocket, HTTP and
  `file://`, transparency cases, `gz-grey`, and a `.dae` with an SDF
  material and one faded by transparency. Parity: every corpus passes,
  Fuel worlds included (offline cache); `nested_model`'s material-less
  primitives needed the `gazebo-default-material` allow-list entry.
- **Visual check** (`--quality ultra`, against main's binary):
  `shapes.sdf`, `sky.sdf` and the all-mesh world render brighter (linear
  colours, as in Gazebo); a synthetic showcase (PBR maps, glass, cut-out,
  emissive, DAE override) at ultra and low.

## 2026-10-18: Heightmap layer blending

- Rules checked against gz-rendering8 ogre2 (`Ogre2Heightmap.cc`, Terra
  `gz/100.gz_CustomWeights_piece_ps.any`, `100.gz_CustomVs_piece_vs.any`)
  and gz-common's `HeightmapData::MinElevation` (0 for images): layer
  `i > 0` blends in with `smoothstep(min_height, min_height + fade_dist,
  h)`, `h` above the terrain's lowest point (`origin.z`); colours `lerp`ed
  in order; ≤ 4 textures / 3 blends; a layer without a blend has weight 0.
  ogre1 (`OgreHeightmap.cc`) fades linearly.
- `heightmap::TerrainMaterial` (`ExtendedMaterial<StandardMaterial,
  TerrainExtension>`) for every terrain: layer 0 in the StandardMaterial,
  layers 1–3 as individual textures with one shared sampler. WESL shaders
  (Bevy 0.20 composes WESL, not naga_oil `#import`s), embedded: forward +
  deferred (`terrain.wesl`) and a prepass that writes the blended normal
  (`terrain_prepass.wesl`), since TAA/SSAO make the forward pass read
  prepass normals. Normals blend with the colour weights (ogre adds them
  on layer 0's); `fade_dist` 0 is a hard step.
- Tangents when any drawn layer has a normal map; snapshots report layer 0
  from `base` and wait for every drawn layer's textures.
- Checked on the 3-layer `allmesh` world (dirt, grass > 1 m, fungus > 3 m)
  at Low and Ultra: same bands in both (forward-only and prepass paths).
- Not checked in a browser (WebGPU): the extension adds 6 textures and a
  sampler per fragment stage.

## 2026-10-19: Gazebo Jetty (UI_PLAN §3a, J1–J4)

- **J1, image:** `Dockerfile.gazebo` is now Ubuntu 24.04 + `gz-sim10-server`
  (server only: `/usr/libexec/gz/sim10/gz-sim-server`, no GUI, no `gz` CLI),
  tag `gz-websocket-jetty`. `docker/websocket.config`, through
  `GZ_SIM_SERVER_CONFIG_PATH`, adds `gz-sim-websocket-server-system` (port
  9002) to every world: since Ionic (gz-sim 9) server-config systems load
  alongside the world's own unless it sets
  `<gz:policies><include_server_config_plugins>false` (gz-sim's
  `tutorials/server_config.md`), de-duplicated by filename. The
  file repeats the default server.config's three systems, since the
  variable replaces it. Same interface: `docker run … gz-websocket-jetty
  <world.sdf>`. No Harmonic image is kept: nothing replays against a live
  server (the parity jobs use recordings). Harmless upstream warning: the
  system logs "Partial SSL configuration" whenever `<ssl>` is absent.
- **J2, protos:** vendored gz-msgs `gz-msgs12_12.0.2` (`703d291`). No wire
  change to anything the client decodes (Scene, Visual, Material/PBR,
  Light, Geometry/Heightmap, WorldStatistics, Pose_V, Publishers,
  StringMsg_V, Bytes). Changes: `Sensor.navsat` (22), `GUICamera.duration`,
  `WheelSlipParametersCmd` friction fields, the
  `SerializedEntityMap.components` map key int64 → uint64
  (`serialized_map.proto`), `SphericalCoordinates.LOCAL2` removed, GPS messages
  deprecated.
- **J3, protocol:** gz-sim10_10.5.0's `WebsocketServer.cc` diffed against
  gz-launch 7's; DESIGN.md "Websocket protocol" lists the result. Same ops,
  framing, `strtok` split, asset resolution and allowed paths, error codes,
  `req`, `image`, `throttle`. Different: `auth` is answered without a key
  (`authorized`), `protos` includes top-level enums, world names go
  through `AsValidTopic`, and the server starts after the world loads.
  Probed live: `auth,,,secret` on a keyless server → `authorized`; unknown
  service → `service_not_found`; `ignition.msgs.*` → empty `Boolean`; a `,`
  in the payload → dropped; `sub,,,` still crashes it, now taking gz-sim
  down. Client and replay server needed no behaviour change (the replay
  server already accepted any key without `--key`); docs updated.
- **Live check** (`--release`, Jetty containers): `--snapshot` and
  `--screenshot` on shapes, sky, heightmap and fuel_textured_mesh. All
  load, meshes and Fuel textures included. The heightmap screenshot shows
  no terrain: in that long-running container the spheres had rolled off the
  edge of the 128 m terrain and fallen for about 10 minutes, to z ≈
  −1.6 × 10⁶ m (corrected after review; see the PR #26 review entry and
  PARITY.md); the same binary renders the Harmonic recording's
  terrain. Two of four heightmap screenshot runs
  crashed (a segfault after writing the image, with `NVVM compilation
  failed` from the NVIDIA driver in the log, and an abort at `--quality
  low`); the other two exited cleanly.
- **J4, corpus:** all 12 corpora re-recorded from Jetty (`parity/record.sh`
  now defaults to `gz-websocket-jetty` on port 9010, waits for a websocket
  upgrade, and writes `source.gazebo`; `gz-parity record --gazebo`). gzweb
  references regenerated with `parity/run.sh`; Fuel was up, so new lock
  files (`sky`, `fuel_textured_mesh`) were fetched from fuel.gazebosim.org.
  New references for `sky`, `visibility` and `fuel_textured_mesh`, which
  gzweb couldn't load from Harmonic's protos. New allow-list entries:
  material-less camera visuals (`gz-grey`) and the two Fuel PBR models.
  `particle_emitter` still has no reference (gzweb throws on a null
  `albedo_map`). Headless tests: `visibility` (default), `sky` and
  `fuel_textured_mesh` (`--ignored`); CI parity job gains `visibility`.

## 2026-10-19: PR #26 review fixes

- **World topics:** `gz_core::topic` ports gz-transport 15's
  `TopicUtils::AsValidTopic`/`IsValidTopic` (upstream's test cases), and
  every `/world/<w>/…` topic or service the client, `gz-parity record` and
  the replay server build goes through `world_topic`, as gz-sim's do.
  Decision and divergence from gzweb: DESIGN.md "World topics", PORT_PLAN
  §2.5.
- **Replay:** `auth` replies are binary, like both real servers'.
- **Parity:** `sky` runs in the default `cargo test` (headless gzweb-rs never
  fetches the cubemap). The Fuel PBR allow-list entries name each differing
  field (`material.*` also matched maps, opacity and transparency).
- **`parity/record.sh`:** stops at once, with the server log, when the
  container exits (it used `--rm` and polled for 5 minutes); writes
  `source.gazebo` from the image (`jetty 10.5.0`); binds 127.0.0.1; caches
  Fuel in a volume. `gz-parity record` prints the pose streams' sim time and
  warns about non-finite or far-away poses.
- **Heightmap, corrected:** the 0d61ae3 recording (sim t ≈ 50–52 s) has the
  spheres on the terrain. The live z ≈ −1.6 × 10⁶ m was a container left
  running about 10 minutes: the spheres roll off the 128 m terrain's edge
  and fall (ode as well as bullet), not through it; checked live, by sim
  t ≈ 118 s all four are past the ±64 m edge and 13–32 km down, and
  `gz-parity record` warns about each. The Harmonic recording
  was at t ≈ 12–13 s, not "at rest". The timing shift: the old 30 s warmup
  began at the TCP accept, before the world loaded; the new one after the
  websocket upgrade. Re-recorded with `RECORD_WARMUP_S=5` (t ≈ 9.3–11.0 s,
  all spheres on the terrain); gzweb reference regenerated, lock unchanged.
- **Image:** runs as `gazebo` (Fuel cache `/home/gazebo/.gz/fuel`; checked
  with the heightmap recording, whose scene names that path), base pinned by
  digest, apt tools purged, `COPY --chmod`, `Dockerfile.gazebo.dockerignore`.
  `gz-sim10-server` depends on `libgz-sensors10-dev` (21 direct `-dev`
  dependencies; 142 `-dev` packages end up installed, much of the 1.49 GB
  layer); upstream packaging.
  `include_server_config_plugins` defaults to true from Ionic (gz-sim 9),
  not 10.

## 2026-10-19: Phase 5 M0, views and UI modes

Branch `feature/phase-5-ui`. Plan: `.agent/UI_PLAN.md` §2 and §4 (M0); spec
`design/ui/DESIGN.md` §3.4, §6.2, §6.3, §13. See DESIGN.md "Views, input and
UI modes".

- **`gz-bevy::view`:** `camera.rs` keeps only spawning; the controller is
  now `ViewCamera` + `ViewCommand { view, op }`: pivot at the point under
  the cursor at drag start (then selection centre, then origin), orbit, pan,
  zoom, zoom at a point, perspective ↔ orthographic keeping the framing
  (orthographic stands off ≥ 50 m), six axis snaps (Gazebo GUI axes, 250 ms
  smoothstep, orthographic at the end, perspective again on orbit), focus,
  home, drag action, fly. `Selection` exists, empty until M5.
- **Key map:** `shortcuts.json` embedded and parsed with serde into
  `InputMap` (scopes, `mod` per platform, `web_alt`, tap vs hold), plus a
  `minimal`-scope fly table. The §13 test runs on macOS, Linux, Windows and
  web: no conflicts once `constrain_x`/`teleop.stop` (Q5) and the Space
  tap/hold pair are allowed; every reserved chord has a `web_alt`.
- **`ViewerControls`:** mouse, keys and fly to `ViewCommand`s; Space tap
  (< 200 ms, no button) play/pause, ⌘→ step, G grid. HUD keys removed (N is
  gone), hint updated, `--no-hud` / `?hud=0`.
- **`UiMode`** replaces `GzConfig.interactive` (`interactive()` keeps the
  meaning; stats subscription follows it). `--ui full|minimal` (default
  full), `?ui=` (default minimal); capture modes force `None`. `Full` =
  `Minimal` until M1.
- **Decisions:** DECISIONS.md D-11 to D-16 (wheel vs Ctrl+wheel, fly keys,
  snap axes, focus order, how the conflict rule reads the file, no reset in
  minimal).
- **Tests:** every `ViewOp`, snap end states and animation, focus/home, fly
  speed, input map parse and conflict/web rules, headless minimal app driven
  by `KeyboardInput`/`MouseButtonInput`/`MouseWheel` messages (drags, wheel,
  Ctrl+wheel, Space tap/hold, keys, fly), CLI and `?ui`/`?hud` parsing,
  plugin wiring per mode, camera respawn keeping the view. Headless and
  `--ignored` Fuel tests pass (HTTP cache copied in).
- **Visual check** (allmesh world, ws://localhost:9004): release gzweb
  windowed in `--ui minimal` and `--ui full` (both reach Ready, no errors);
  `--screenshot` at High and Low unchanged; a throwaway example (not
  committed) captured top/front snaps, ortho toggle, ortho zoom and
  near-vertical views at High and Low. One early top-snap capture at High was
  flat sky colour; removing TAA, SSAO or the atmosphere light fixed it and it
  never came back; projection switches now reset TAA history.
- **Left:** pointer routing around UI nodes and `preventDefault` for
  browser chords (M2/M3, M10); layout-aware letter keys (M10); browser
  build not run.

## 2026-10-19: Phase 5 M1, theme, icons, fonts and widgets

Branch `feature/phase-5-m1` (from `feature/phase-5-ui`). Plan:
`.agent/UI_PLAN.md` §4 M1; spec `design/ui/DESIGN.md` §4, §5, §9.5, §10,
§12 item 1, §13. See DESIGN.md "UI crate (gz-ui)"; decisions D-21 to D-29.

- **New crate `gz-ui`** (`GzUiPlugin`, added by gzweb in `--ui full`; no
  shell yet). `build.rs` generates the token tables from `tokens.json` and
  the icon atlases (89 icons × 15/17/18/20 px, 1× 53 KB and 2× 114 KB PNG)
  from `icons.json` with resvg; `IconId` is generated. Theme resource,
  role components, `apply_theme_system`, `button_visuals_system`, OS theme
  from the window, focus ring.
- **Fonts:** Geist and Geist Mono v1.8.1 from the vercel/geist-font GitHub
  release (`geist-font-v1.8.1.zip`, sha256 matches the release digest; name
  tables carry the Geist Project copyright and the OFL notice; the files
  are newer builds (1.801) than the ones committed in the repo's `fonts/`
  at the tag (1.800)). The five files `tokens.json` lists plus `OFL.txt`,
  embedded (680 KB, 316 KB gzipped). The pre-commit text fixers skip them.
- **Widgets** §5.1–5.16 + splitter on `bevy_ui_widgets`, with AccessKit
  names; NumberField's expression parser; menus with keyboard navigation,
  type-to-search and on-screen flipping; tooltip timing; slider Shift/Alt
  steps.
- **Gallery:** `--ui gallery` / `?ui=gallery`, `--theme`, T to switch;
  `--screenshot` works with it (1440×900).
- **Text fields** stand the viewer's keys down; in the browser the canvas
  listener (`view::web::set_text_focus`) keeps browser shortcuts while one
  has focus.
- **Wasm payload** (release, after wasm-bindgen, not wasm-opt'd):
  `gzweb_bg.wasm` 71.5 → 74.2 MB raw (+2.7 MB, debug names included),
  12.75 → 13.64 MB gzipped (+0.88 MB): fonts ≈ 0.32 MB, atlases and logos
  ≈ 0.2 MB, the rest code.
- **Tests** (50 in gz-ui, 98% line coverage): generated tables match
  `tokens.json`; §13 contrast pairs ≥ 4.5:1 in both themes, computed from
  `tokens.json` (lowest: dark `text_3` on `hover`, 4.53); no colour literals
  in `crates/gz-ui/src` outside `theme.rs`; fonts and icons as listed; every
  atlas cell has white ink; state table and system; number parsing,
  commit/revert/blur/scrub; slider steps; menu keyboard navigation, Esc,
  outside click, selection, checkbox items, select; tooltip timing and cards;
  splitter maths and drags; toggles, tree rows; headless layout heights
  (section header 36, menu item 30, tree row 28, button 30, input 32, icon
  buttons 32/28/rail 32); the whole gallery re-colours on a theme switch.
- **Visual check:** release build, `--ui gallery --theme light|dark
  --screenshot target/shots/gallery-{light,dark}.png`, compared with board
  06 (and, in a throwaway run, open menus, the select, the Snap popover and
  live tooltips). Colours match the tokens exactly and the component strip
  matches the board's sizes and layout. Differences: Bevy's text is thinner
  and lighter than the board's (dark-on-light glyphs blend in linear space);
  one theme per screenshot instead of side by side; menu shadow lighter;
  status chip has the spec's warning icon (the board's has none); the logo is
  the spec's 28 px.
- **Merged** `origin/feature/phase-5-ui` (PR #27 fixes, then main with
  Jetty) before pushing.

## 2026-10-19: Phase 5 M2, the shell

Branch `feature/phase-5-m2` (from `feature/phase-5-m1`, PR #28). Plan:
`.agent/UI_PLAN.md` §4 M2; spec `design/ui/DESIGN.md` §2, §3, §5.18,
§7.10, §8, §9.3. See DESIGN.md "Shell (gz-ui, M2)"; decisions D-30 to D-38.

- **`gz_ui::shell::ShellPlugin`** (gzweb adds it in `--ui full`): the
  region tree of §2.1 with every size and colour from the theme; top bar
  (world name, tools radio, Snap, transport wired to `GzCommand`, steps per
  click, clock, RTF, search); activity bar (order, groups, Settings, badges,
  click / Shift+click / collapse); sidebar with two slots, row splitter and
  header buttons, panel shells in their empty, loading and error states;
  World inspector shell; status bar with its narrow-width drops; splitters
  (6 px, col-resize, `accent` while dragging, double-click resets); ⌘B and
  ⌘⌥I from the input map; start-up collapse at 1280 / 1100 and the floor
  rule; `ShellLayout` holds it all. The full UI's window starts at
  1440×900, minimum 1024×640. The status text is minimal mode's only.
- **3D view in the viewport area:** first as `Camera::viewport`, which
  rendered the scene squeezed into the middle of the view at High (Low was
  right): Bevy 0.20's TAA samples the whole window-sized main texture and
  only clips its output to the viewport. Now the main camera renders into a
  texture of the view node's physical size (`gz-bevy::view::ViewPlacement`,
  D-31), resized with its projection and target size in the same frame
  after UI layout; colours match the window render (ground and box means
  within 2/255). A first resize failed wgpu validation (Bevy's target
  textures copy on resize, which needs COPY_SRC); copy-on-resize is off.
- **Routing** (`ViewRouting`, D-32) from the hover map; viewport keys after
  a press in the view; minimal mode unchanged.
- **Connection:** `GzReconnect { url }` (Retry now, Reconnect, the card's
  Connect), `GzStatus::attempting`, `SimStats::{real_time, step_size}`.
  Connect card on both targets (D-33), banner with the 60 s change, the
  dimmed last frame (D-34). Browser: `?url=`, else the last server
  (`localStorage`).
- **Captures:** `--ui full --screenshot` (1440×900), `--window WxH`,
  `--capture-at-ms MS` (shoots at that time instead of when settled).
- **Tests:** 17 shell tests in gz-ui (regions at 1440×900; collapse at six
  widths; floor order; splitters and slot split; keys; placement at scale
  1, 1.5, 2 and `UiScale` 1.25; routing; activity, transport, readouts,
  formatting, phases, banner with fake time, card, cursor, theme, slot
  buttons), plus gz-bevy tests for routing, placement, the ground pick,
  reconnecting and forced captures, and gzweb flag tests. Headless suite
  green with `--include-ignored`.
- **Visual check** (fast non-LTO release build, against the running
  `allmesh` world on ws://localhost:9004; `target/shots/`): `full-light`,
  `full-dark` (1440×900), `w1280-light`, `w1280-dark`, `w1024-light`,
  `w1024-dark`, `card-light`, `card-dark` (bad URL), `banner-light` and
  `banner-long-dark` (a `gz-parity replay` server killed after 5 s, shot at
  9 s and 68 s), compared with board 01 in crops at 2×. Region edges match
  the board to the pixel (top bar 44, activity 56, sidebar 300, inspector
  300, status 28); activity buttons within 2 px; inspector title row within
  3 px; the transport sits centred between the left group and the search as
  on the board. Differences: Bevy's text is lighter (as in M1); the search
  placeholder clips instead of ending in "…" (no ellipsis in bevy_ui); key
  caps are spelled ("Ctrl+K", D-26); the board's content (tree, rail,
  cube, model inspector, message chip, badges, split view) is later
  milestones.

## 2026-10-19: PR #28 review fixes

- **NumberField:** unedited text commits nothing on Enter or blur and shows
  the current value again (D-39; tabbing through fields truncated them). A
  unit applies to its whole term, so `pi/2 rad` is 90° (D-40); nesting is
  capped at 64 and signs don't recurse (5 000 `(` overflowed the stack). A
  scrub adds each move's delta, so Shift mid-drag doesn't jump. The input's
  AccessKit node has a spelled-out name and the numeric value.
- **Cursors:** `CursorIconPlugin` was never added, so no hover cursor showed;
  drags hold theirs (D-41). Splitter drags divide by `UiScale`; splitters
  take arrow keys, Home and End and a caller-given name.
- **Keys:** the viewer's scopes come back a frame after a field blurs, so its
  Esc can't fire a viewport binding (D-43); in the browser Tab stays in the
  canvas while a field has focus (D-42), and the web gallery installs the
  canvas key listener. The gallery focuses its example field only in a
  capture (D-47), so T switches theme after launch.
- **Icons:** cells at 10/12/13/14/24 px too, and white transparent texels
  (D-44); a changed `Icon::px` resizes the node.
- **Change detection** for every widget system (D-45).
- **Looks:** disabled menu items dim their shortcut, icon and box; the
  selected filter chip has no seam; tooltips set the shortcut in Geist Mono
  (D-46). Menus gain the mirrored end-aligned placement; type-to-search
  ignores chords; sliders take Up/Down.
- **Spec checks:** the colour-literal check tokenizes (types, constructors,
  constants, aliases, 3–8 digit hex, across lines); the tokens test walks
  every radius, border, motion and elevation entry; build.rs rejects unknown
  motion and elevation entries and generates elevation colours; the gallery
  check covers icon tints, outlines and shadows.
- **Housekeeping:** `Theme::logo_path` removed; `Theme::set_mode` reads the
  tracked OS preference; systems, observers and internal markers are
  `pub(crate)`; pre-commit excludes the fonts from the text fixers only.
- **Not changed:** `modifier_steps` re-inserts `SliderStep` because Bevy
  makes it immutable; tooltip cards are trigger children, but neither the
  card nor its text has an AccessKit node, so nothing joins the trigger's
  subtree. Disabled content at 40% reads lighter in dark than a browser's
  40% because Bevy blends in linear space; matching it needs the
  background, so it's left as the spec's 40% for now.
- **Merged into M2:** the shell's sidebar/inspector and slot splitters had
  the same `UiScale` bug and divide by it now (D-41); `UiCommand::SetTheme`
  uses the one-argument `Theme::set_mode`; the text field's Enter handler
  runs on the input only and its focus look only when focus moves.

## 2026-10-19: M2 owner changes

- **Top bar** (D-48, spec 0.4 §3.1/§5.4): the tool and transport groups
  have no container; their buttons (Play/Pause included, no `primary`
  fill) use the activity buttons' look on the bar's `surface` (white in
  light). A shell test switches theme by the Settings command both ways and
  by the OS in System mode and checks the bar and every resting button.
- **Tooltips** (D-49, §5.15): activity buttons' open to the right; any card
  past an edge is shifted 8 px inside after the popover picks its side, its
  arrow on the facing edge pointing at the trigger; titles don't wrap to the
  trigger's width any more.
- **Collapsed panels** (D-50, §2.1/§3.2/§3.3): a 24 px bar with a
  chevron button ("Show sidebar · Ctrl+B", "Show inspector ·
  Ctrl+Alt+I") restores the panel at its previous width; the bars count in
  the floor maths and the view's placement; the collapsed panel's activity
  button stays selected (and pressed to AccessKit), and clicking it
  expands the sidebar.
- **"System … has not run for N ticks"**: Bevy warns once a system's last
  run is ~3.2 billion change ticks old. Probed headlessly (system
  `get_last_run` after start-up): the only idle systems of ours are the
  start-up ones (`init_layout`, `spawn_shell`); the rest are Bevy's
  start-up systems and its run-condition-gated ones (hover maps, unused
  asset events, dialogs). Nothing misses a change it needs, so gzweb's log
  filter sets `bevy_ecs::system::system=error` (that module logs only this).
- **Visual check** (release build against the `allmesh` world on
  ws://localhost:9004; `target/shots/`): `full-light`, `full-dark`
  (1440×900) and `w1024-light`, `w1024-dark` (1024×700, both panels
  collapsed to their bars, Scene still selected).

## 2026-10-19: gz-ui restructure (D-51)

- **Layout**: feature folders (`theme/`, `assets/`, `widgets/`, `shell/`
  with a folder per region, `panels/`, `inspectors/`, `gallery/`) instead
  of one file per region with a 646-line `shell/mod.rs`, a 1072-line
  gallery and central test files. Conventions in UI_PLAN §7, spec 0.4.1
  §9.1.
- **Panels and inspectors** are plugins registering a `PanelDef` /
  `InspectorDef`; the sidebar's per-panel `match`, `Panel`'s
  title/icon/binding and the per-panel badge rules moved into
  `panels/<name>/`, the World inspector into `inspectors/world/`.
- **Shared**: `widgets::layout` (row, column, flex_spacer, separator,
  overlay, clip_box, group, property_row, set_shown), `widgets::text::
  set_text`, `Tooltip::binding`, `TextButtonProps` / `SliderProps` /
  `MenuItemProps`, one `side_panel` region. The top bar's and status bar's
  god systems split per readout (`StatusSources`, `SlotSources`,
  `PointerOver` SystemParams); clippy allows down from 25 to 10.
- **API**: 345 public items in 42 modules before, 252 in 21 after
  (rustdoc pages); `unreachable_pub` warns.
- **Tests**: next to their modules; 83 before, 98 after (registry, each
  panel, the World inspector, layout primitives, set_text/set_shown change
  detection; the connection test split in two).
- **Behaviour**: unchanged. Captures from the M2 binary and after (full UI
  light/dark at 1440×900 and 1024×700 against `allmesh` on
  ws://localhost:9004 with the world paused, the connect card against a
  closed port, the gallery light/dark) are pixel-identical outside the 3D
  view (temporal AA differs run to run) and the fps readout. Readout texts
  and the overlays' nodes are now marked changed only when they change.

## 2026-10-20: PR #29 review fixes

- **Focus** (D-52): a click no longer leaves keyboard focus on a button
  (`PressFocusless` on every `clickable` widget, `PointerLed`), and focus
  inside a hidden or fading node is released (`release_focus`, `Hiding`
  on the fading card). Regression tests click Connect, "Retry now", the
  sidebar chevron, the Scene activity button, Step and Play through
  bevy_picking's press/click events and press Space and Enter; Enter in
  the address field followed by the world loading; a Tab-focused button
  still takes Enter. Each fails without `release_focus`. Esc in an open
  menu only closes it (`MenuTakesEscape`).
- **Start-up rules** on the first real window size (D-53); **steps per
  click** in `gz-bevy`, used by ⌘→, with the current choice marked and a
  Custom field (D-54); **addresses**: `?url` > `?port` > remembered URL
  (full UI only) > page host, `http(s)` → `ws(s)`, `wss` on https pages,
  the default port for a bare host, no world name or step carried to a new
  server, the minimum window size native only (D-55).
- **Readouts** (D-56): the step size survives a pause in `gz-bevy` (and
  a world paused before the connection gets sim time over iterations: the
  live capture of the paused `allmesh` world still read "—"); the
  shell's readouts, overlays, slot rebuilds and activity states run on
  change; `key_scopes` doesn't allocate; `apply_view_placement` runs only
  when placed (the main camera keeps rendering with no world: inactive, it
  panicked in Bevy's atmosphere probe in a live capture); accessible
  names and open tooltips follow their state; a shifted tooltip drops its
  popover; a splitter double-click needs the primary button and no drag;
  an interrupted fade restores opacities; the search placeholder steps
  down instead of cutting a word; the workspace shortcut comes from the
  input map; Reset is enabled alike in the top bar and the inspector.
- **Docs:** DESIGN.md §9.3 and UI_PLAN's M2 entry and risk table describe
  the view texture of D-31 instead of `Camera::viewport`.
- **Merged into the restructure** (D-51): each fix went to its feature
  folder (`top_bar/transport.rs`, `top_bar/search.rs`, `splitter.rs`,
  `regions.rs`, `commands.rs`, `status_bar/`, `viewport/`,
  `widgets/focus.rs`) and each test beside its module; the banner's
  change gate is a `SystemParam` rather than an eighth argument.

## 2026-10-20: PR #30 review fixes (D-57)

- **Registry**: open `PanelId`/`InspectorId`; `PanelDef` carries the
  activity bar group and order and `default_open`; badges and counts are
  the panels' own systems writing the shell's `PanelStatus`;
  `ActivityBadges`, `Panel::ALL`, the count and badge function types are
  gone. A count changes the header's text in place (a test checks the
  body entity survives it); `register_panel` is the only way in, warns on
  a duplicate id or binding and panics after the shell spawned;
  `ShellPlugins` = `ShellPlugin` + `PanelPlugins` + `InspectorPlugins`.
- **Nits**: `widgets::format` (was `shell::format`), module trees
  `pub(crate)` with re-exports, the last `too_many_arguments` allows
  replaced by `SystemParam`s, the checkbox item's icon, the gallery on
  `row`/`column` and `BAR_SEPARATOR_H`, the cursor readout chained before
  the status texts, docs in build.rs and theme/tokens.rs, tests for
  panel actions and `PanelDef::content`.
- **Captures** (release, `allmesh` on ws://localhost:9004 paused, full UI
  light/dark at 1440×900 and 1024×700, the connect card against a closed
  port, the gallery light/dark): against M2 with the #29 fixes, identical
  outside the 3D view (temporal AA differs run to run) but for the fps
  digits; against the M2 binary before the fixes, the differences are the
  search placeholder ("Search…", D-56) and the step size ("— · 1 ms" in
  the status bar, "1 ms" in the inspector, D-56). The gallery is
  pixel-identical to both.

## 2026-10-20: Panel contract (PR #30 second review, D-58)

- **Slots:** a header is keyed on (panel, two slots) and a body on (panel,
  view): opening or closing the second slot keeps the first slot's body, and
  a swap re-parents both (the review's XP1/XP3 rebuilt them). The slots
  rebuild in `Readouts`, after the panels write.
- **Views:** panels write their `PanelView` (`Cow` messages) into
  `PanelStatus` through `SetPanelStatus` (compare, then mark a change);
  `PanelDef::view` is the default; `ViewFn` is gone. Scene and Topics write
  theirs; Sensors/Assets/Teleop keep placeholders until their data exists.
- **Actions:** open `PanelAction { id, label, enabled }` and
  `HeaderAction`s; both trigger `PanelActionActivated`; Reconnect is the
  shell's. Scene's Expand/Collapse all and Topics' Publish/Services are
  defined, disabled until M4/M7.
- **Content:** `.content::<Marker>()` and the panel's own `Add<Marker>`
  observer; ready bodies are full-bleed, other states padded. Bodies are
  disposable views; panel state lives in panel resources (UI_PLAN §7).
- **Sets:** `ShellSystems::Panels` (was `PanelStatus`) and
  `panel_shown(id)`.
- **Tab:** a `TabGroup` per region in §6.5's order; `hide_undisplayed`
  makes `Display::None` nodes invisible so Tab skips them (Bevy skips only
  `InheritedVisibility`); a test presses Tab for real.
- **Nits:** shared `Registry<D>` and `ui_id!`; a duplicate id panics;
  `.place(group, order)`; unknown ids fall back to the first panel, a
  disabled default panel to the first, no panels to a collapsed sidebar;
  `format` top-level; `BAR_SEPARATOR_H` in the top bar; `button::build`
  gone; `SystemParam`s with methods; panels' systems in `systems.rs`; a
  probe panel for the shell's tests; per-panel string tests replaced by
  behaviour tests through the shell; binding ids checked against
  `shortcuts.json` (the gallery named two that don't exist); `ShellPlugin`
  asks for `GzUiPlugin`.
- **Later items** (review items 7, 8, 9, 11, 12, 13) are filed under
  M4–M10 in UI_PLAN.
- **Captures** (release, `allmesh` on ws://localhost:9004 paused, full UI
  light/dark at 1440×900 and 1024×700, the connect card against a closed
  port, the gallery light/dark), against main's code built in the review
  worktree: the gallery and both 1024×700 shots (sidebar collapsed) are
  identical outside the 3D view; at 1440×900 and on the card the only
  change is Scene's header gaining Expand all and Collapse all (disabled,
  left of the chevron), plus the fps digits. The Scene body has no content
  yet, so ready bodies going full-bleed changes nothing on screen.
- **Tests:** workspace 485 → 493 passing (gz-ui 119 → 127); coverage
  98.4% lines.

## 2026-10-20: PR #33 review fixes (D-58a)

- **M1, Space on a focused button:** with the regions tab groups, Tab to
  Reset then Space reset the world and toggled pause (gz-bevy's global
  chord fired too), and Space on Play/Pause toggled twice. A focused widget
  now consumes the keys it acts on: Bevy dispatches a key to the focused
  entity and a widget that handles it stops it, so `focus::note_window_key`
  notes the presses that reach the window and `focus::hide_taken_keys`
  resets the rest (and their repeats, until release) in
  `ButtonInput<KeyCode>`, which every shortcut reads. With nothing focused
  Space still toggles; ⌘B still works with a button focused.
- **S1:** with no panels the sidebar stays collapsed through start-up's
  refits, ⌘B and its bar's button.
- **S2:** the second slot opening or closing rebuilds only a header's slot
  buttons; a focused header action keeps focus.
- **S3:** `PanelState::actions`, `PanelStatus::action_enabled`,
  `SetPanelStatus::set_action_enabled`; the header button changes in place.
- **S4:** a header action's binding triggers `PanelActionActivated` while
  its panel is shown and the action enabled (`header_action_keys`).
- **Nits:** `hide_undisplayed` leaves a visibility its owner set (N1); the
  slots despawn only bodies they host (N2); ready bodies have no row gap
  (N3); disabled controls are no Tab stops (N4); no nested `TabGroup`s,
  UI_PLAN §7 (N5); an unknown activity id is ignored (N6);
  `ShellSystems::Slots` orders the slots before the readouts (N7);
  `BAR_SEPARATOR_H` in `widgets::layout` (N8); the binding scanner reads
  raw strings (N9); the footer is filed under UI_PLAN M7 (N10); tests for
  each, plus Shift+Tab and Tab walks after reopening the sidebar and around
  the second slot (N11).
- **Web check** (release wasm, `?ui=full` against `allmesh` on
  ws://localhost:9004): Tab ×8 to Play, Space plays once and Space again
  pauses (before, the two toggles cancelled); with the view clicked
  (nothing focused) Space toggles; Tab ×9 to Step, Space steps 1 ms and
  the sim stays paused. No console errors. The world was left paused.
- **Tests:** workspace 493 → 502 passing (gz-ui 127 → 136); coverage
  98.7% lines. check.sh, clippy on 1.99.1 (native and wasm32) and
  `./build.sh` pass.

## 2026-10-20: M3 viewport controls (D-59 to D-64)

- **Rail** (`gz_ui::shell::viewport::rail`): Orbit/Pan/Zoom radio, Projection,
  Camera mode (menu, accent look and dot when not Free, target chip beside
  it), Focus (double-click home), Overlays (menu, count badge), Layout
  (presets; everything but Single disabled until M6). Shows `gz-bevy`'s state
  both ways. Compact rail below a 500 px view (UI pixels). Rail controls are
  tab stops in the viewport area's group (no `TabGroup` of their own, per the
  #33 review); a press or focus on them keeps the view's keys.
- **View cube** (`view_cube`): own orthographic camera on layer 30 rendering
  into a texture of the node's physical size; token colours by on-screen
  facing; compass labels as UI text; picking on the image (3 × 3 per face:
  face → snap + ortho, edge/corner → look from, projection kept); hover in
  `accent_ring`; drag orbits. Animations ease out cubically (motion tokens).
- **HUD** (`hud`): scale label with a 1-2-5 bar in orthographic views (D-63);
  target chip with ×.
- **gz-bevy:** `overlays` (new: only the grid existed) with collisions,
  inertia, CoM and joints from `generate_world_sdf` (`gz_core::sdf`, new),
  frames from the scene, wireframe via `WireframePlugin` natively and line
  meshes in the browser; `view::modes` (Follow/Track, ⇧F/⇧T on the selection
  or the model under the pointer); `ViewOp::LookFrom`; `NoViewPick`;
  `StartView`; C/I/J/⇧F/⇧T in both modes. A `StringMsg` service reply is only
  a failure when it says `service_not_found`.
- **gzweb:** `--view` and `--overlays` (D-64).
- **Browser check** (wasm release on a local page, `?ui=full`): the rail, the
  cube (click → top view, ortho, scale label), the Overlays menu; turning on
  Wireframe quit the app (WebGPU refuses a depth bias on line lists) — fixed,
  the line-mesh fallback draws.
- **Found:** with the inspector open, a window shorter than about 730 px grows
  the shell past the window (DECISIONS.md, Open). The compact captures use
  1100 × 560, where the inspector starts collapsed.
- **Captures** (release, `allmesh` on ws://localhost:9004 paused, full UI):
  light/dark 1440×900, light/dark 1100×560 (compact rail), light/dark
  `--view top` (ortho, scale label "50 cm", snapped TOP face), light with
  every overlay. Against board 03/01: rail order, sizes, separators, badge,
  accent states and tooltips side match; the cube shows two faces from
  gzweb's home view (along +X) where the board draws a corner view; the
  scale label carries a bar (D-63).
- **Tests:** workspace 493 → 546 passing (gz-ui 127 → 152, gz-bevy 205 →
  226, gz-core 65 → 71, gzweb +1); coverage 98.7% lines.

## 2026-10-20: M3 view cube fix (D-61 amended)

- **Found:** the cube drew about 30 px wide in its 56 px node and, from
  gzweb's home view, as two flat rectangles (TOP over W). Cause: its
  orthographic camera was framed to the cube's bounding sphere (3.7 units
  across the node), so a face took 2 / 3.7 of it; the texture and node sizes
  and the scale factor were right. Its shadow, a 3D box slightly larger than
  the cube, also came in front of the face looking at the camera after the
  1 px offset (the west face rendered 12 levels darker than `cube_left`).
- **Fix** (`view_cube`): a perspective camera (4 half-sides away, 57°, about
  the view's 60°) framed so the bounding sphere fills 52 of 56 px: a corner
  view fills the node, nothing clips, the size never changes as the view
  turns; home is about 42 × 40 px with the near edge standing out, a face on
  36 px. Picking casts rays from the cube camera (`pick::project` and
  `pick` invert each other); labels sit at the projected face middles and
  show by the angle to the camera. A side facing the camera squarely is
  `cube_left` (it was `cube_right` or `cube_left` by rounding, and
  `cube_right` is barely off white next to `cube_top`). The drop shadow is a
  second UI image of the same texture tinted `shadow`, 1 px lower, under the
  cube's image. Edges 0.07 of the half side (about 1 px).
- **Captures** (release, `allmesh` on ws://localhost:9004, full UI,
  scratchpad `cube/`): light/dark 1440×900, light/dark `--view top`, and
  light at scale factor 2 (2880×1800 with a local, uncommitted
  scale-factor override in the capture window). Face colours match the
  tokens exactly; the shadow shows as a 1 px line under the cube; at scale 2
  the labels are sharp (at scale 1 the mono 7 W is a blur, as before).
- **Tests:** gz-ui view cube 11 → 14 tests: no clipping from any roll-free
  view, fill and perspective from home, pick inverting the projection on
  every visible patch, the shadow image (offset, tint per theme), and the
  edges' theme colour.

## 2026-10-20: M4 backend, scene sync, scene tree, status (D-65 to D-70)

- **Backend** (D-65): `gz_bevy::backend::SimBackend` (spec §9.4, plus
  `poll`/`flush`) with `WebsocketBackend` (was `GzConnection`) as the
  non-send `Backend`; `pump` maps `GzCommand`/`GzReconnect` to trait calls;
  the backend writes `GzStatus`/`GzInbox` through `Mut`, so a quiet frame
  marks nothing changed. `GzCommand::Publish` is refused by the websocket
  backend (no released server takes `pub_in`).
- **Scene sync**: `scene/deletion` (interactive modes only; the capture
  modes keep gzweb's protocol), `apply_deletions` before `apply_scene`
  (delete-and-respawn in one frame works; an unknown id is retried once);
  `GzIndex` was marked changed every frame by `apply_scene`'s `&mut`
  reborrow and now changes only with the structure; sensors on links
  (`SensorDesc`, `GzSensors`; checked against Jetty's `sensors_demo`: the
  scene message carries sensors, not joints); `SdfWanted` and one
  `generate_world_sdf` in flight.
- **Header actions** at run time through `PanelStatus` (separate commit
  1e7a162 for PR #33, which now carries the same API).
- **TreeRow** (D-67): recyclable (every part follows the spec, AccessKit
  label/expanded/level/selected too), controlled caret and eye
  (`TreeRowExpand`, `TreeRowVisibility`), not a tab stop, cursor ring
  inside the row.
- **Scene panel** (D-66): `panels/scene/{model,view,systems}`; the tree
  model in preorder with subtree sizes (rows in one pass; 20,000 nodes
  well under a frame in debug), state in `TreeState`/`TreeScroll`, the
  body built by `Add<SceneTree>`; a `ScrollArea` whose content is as tall
  as the rows and a recycled pool over the view (a 3,000-model world
  builds about 30 rows); filter field (`filter_field`, placeholder),
  controlled type chips (`choice_chip`), Expand/Collapse all enabled,
  visibility on the scene entity, arrow keys, footer disabled until M5.
- **Status bar** (D-68): `gz_bevy::messages` (a tracing layer gzweb
  installs through `LogPlugin::custom_layer`; the newest 500 of our
  crates' warnings and errors) and the message chip in item 5. The
  transport and the other readouts are M2's; checked against §3.1/§3.6.
- **Found and fixed** (D-69): Bevy puts UI roots in an implicit grid
  whose items are at least as tall as their content; a long tree made
  the shell 84,000 px tall and a short window with the inspector open
  grew it past the window (M3's open item). The root has min sizes 0.
- **gzweb** (D-70): `--scene-filter TEXT`, `--scene-expand`.

## 2026-10-21: PR #34 review fixes (D-64a)

- **SDFormat requests** (`overlays::extras`): keyed on the world and the
  set of model names (was the count); one in flight, the next asked when
  its reply lands; forgotten while the connection isn't `Ready` (a lost
  reply or a reconnect asks again); `receive_extras` runs before
  `request_extras`.
- **Collisions** after a respawn: links without collision meshes get them
  when links were spawned or the overlay turned on, from the held reply.
- **`gz_core::sdf`**: `relative_to` on collisions and inertials;
  `inner::link` frames through the nested model (`frame_in_model`
  replaces `link_in_model`).
- **View cube:** `turn_cube` before `TransformSystems::Propagate`,
  `place_labels` before `UiSystems::Layout` (both a frame late before);
  hover re-picked each frame from `CubePointerAt`; `on_cube_press` resets
  the drag distance (a drag released off the cube swallowed the next
  click). `click_cube` in the tests presses first; `release_on_cube` is the
  click that ends a drag.
- **Scale label:** below 1 mm to a thousandth ("0.05 mm").
- **Camera menu:** the current mode's item names its target (latent).
- **Tests:** a regression test for each (overlays 4, sdf 2, view cube 3,
  rail 1, and the scale label's lengths test extended); workspace 558 →
  568 passing; coverage 98.7% lines. check.sh passes.

## 2026-10-21: PR #35 review fixes (D-70a)

- **Merged** M3's review fixes first: `extras.rs` takes M3's request
  logic with M4's `SdfWanted` (which covers the replaced-model finding).
- **Paths:** `PathAllocator::free`; `apply_deletions` frees the subtree's
  paths; the lowest free suffix, skipping paths another name has (`a#1`
  named so collided with the second `a`).
- **`--scene-expand`:** extends the open rows, waits for more than the
  world; `rebuild_model` after `gz_bevy::spawn::apply_scene`.
- **Rebuilds:** on the world's name, not every `GzStatus` change; rows
  repaint when the model changes (found while testing it: a renamed
  world kept its old name).
- **Scrolling:** `first_row` allows for the list's top padding,
  `pool_size` is a row longer.
- **AccessKit:** `TreeContent` has a `GenericContainer` node; empty pool
  rows are hidden.
- **Tests:** path reuse (gz-core), respawned paths (gz-bevy), scene expand
  on an empty world, stats leave the tree alone, the pool covers the list
  at any scroll (checked to fail with the old maths), the accessibility
  tree; workspace 586 → 591 passing after the merge; coverage 98.6% lines.
  check.sh passes.

## 2026-10-21: Orthographic views blank in a world with a falling model (D-20 amended)

- **Bug:** switching to orthographic (the rail, `O`, or a view cube face)
  showed only the background in the Jetty `diff_drive` world. Its
  `spawned_box` had fallen to z ≈ −6.1 × 10⁹ m, so `fit_clip` stood the
  camera that far behind the pivot, out of `f32`'s range for the scene.
- **Fix:** `fit_clip` caps the orthographic extent at `MAX_ORTHO_EXTENT`
  (10 km, where `f32` steps are about a millimetre). Review (PR #37): the
  first cap, the perspective far floor (1000 m), clipped kilometre-scale
  worlds that perspective shows.
- **Tests:** with a model 6 × 10⁹ m away, the toggle and a front snap
  stand the camera the cap plus the reach from the pivot, with the clip
  range bounded by the cap; a model 3 km away stays in the orthographic
  view (checked to fail with the 1000 m cap). Both checked to fail against
  what they guard; workspace 591 → 593 passing; coverage 98.6% lines.
  check.sh passes.
- **CI** (PR #37): `assert_world_in_front` allowed 1 mm of rounding,
  which a camera 10 km out exceeds by a step or two in CI's
  coverage build (depth 10008.483 against 8.484). It now allows a few
  f32 steps at the camera's distance.

## 2026-10-21: Asset-load tests wait on the clock, not a frame count

- **Flake:** three of the last eight CI runs failed in `gz-bevy` tests that
  wait for a load on another thread (mesh, STL, DDS/TGA, heightmap, sky,
  the asset request): each polled for a fixed number of frames, about 1 s,
  which a busy runner measuring coverage can exceed. PR #36's run failed
  this way on a tree that passed as #35.
- **Fix:** `test_util::wait_until` polls until the condition holds or 30 s
  pass, so a fast machine is no slower. The six loops use it.

## 2026-10-21: Runaway models left out of the view's bounds (D-20 amended)

- **Bug** (PR #37 review): with `diff_drive`'s fallen `spawned_box`, F
  with nothing selected framed a 6 × 10⁹ m world (blank again in
  orthographic), zooming out was all but unlimited, and the orthographic
  shadow cascades spanned the 20 km slab the cap leaves.
- **Fix:** `ViewScene::bounds_of` takes a `Skip` (planes, runaway): the
  world's bounds and extent, and F over a model, leave out meshes lying
  wholly more than `RUNAWAY_DISTANCE` (10 km) from the origin;
  `Bounds::distance_to`. The selection's bounds keep them.
- **Review** (PR #38): the first version judged whole top-level models, so
  a model with one link fallen away still blew up F; the filter is per
  mesh now. The cascade figures in the amendment were the first cap's
  (2 km); corrected.
- **Tests:** F (nothing selected, and over the model) and the zoom-out
  limit ignore a model or link fallen 6 × 10⁹ m (checked to fail without
  the filter), a world that has all run away goes home and zooms out to
  the grid only, a model 5 km down is still framed, `distance_to`; the
  cap test uses terrain reaching 29 km, since the fallen box no longer
  reaches the cap. Workspace 593 → 599 passing; coverage 98.6% lines.
  check.sh passes.

## 2026-10-21: M5a selection (D-71 to D-76, D-64b)

- **Jetty service check:** the editing services exist (`set_pose`,
  `set_pose_vector`, `create`, `remove`, `light_config`, `visual_config`,
  `material_color`, `set_physics`), but the websocket server builds each
  frame as a C string and splits it on `,`, so binary edit payloads with
  zero or comma bytes (any round double) don't arrive. Decided with the
  owner: patch the server in our image, prepare the upstream change, and
  make editing read-only on a stock server (D-76, lands with M5c).
- **gz-bevy:** `view::select` (click within 3 px selects the top-level
  model, Shift adds, ⌘/Ctrl toggles; ⌘/Ctrl+drag or `BoxSelectTool`
  draws a `Marquee` and selects what it overlaps; `prune_selection`),
  `KeyMods::mod_key`, `Bounds::corners`; `view::outline` (mask clones,
  `MaskCamera`, `OutlineMask`, `Hovered`); `StartView::select`.
- **gz-ui:** tree rows select (Shift range, ⌘/Ctrl toggle, the world row
  clears) and show `Active` for selected entities; "sel:" readout;
  `BoxSelectTool` follows the tool; the footer's Hide others and Isolate
  (`panels::scene::footer`); the marquee box; the outline overlay
  (`UiMaterial`, `outline.wesl`); tree hover feeds `Hovered`.
- **gzweb:** `--select MODEL,…`.
- **Live check** (`--release`, Jetty `diff_drive`, `--select
  vehicle_blue`): the outline follows the vehicle's silhouette in
  perspective and in an orthographic top view; the tree row, the status
  bar and the footer follow the selection.
- **Tests:** click, Shift, ⌘/Ctrl, the slop, both marquees, hidden and
  ground models left out of a marquee, pruning and the select modes
  (gz-bevy view); clones, a late mesh and the mask camera's placement
  (gz-bevy outline); tree selection, a view's selection marking its row,
  the footer, the Box select tool, the marquee box, the outline overlay
  and tree hover (gz-ui); `--select` (gzweb). Workspace 599 → 618
  passing; coverage 98.5% lines. check.sh passes.

## 2026-10-21: M5a review fixes (D-71 to D-75, D-64b amended)

- **Wireframe:** the overlay wired the mask clones, so every triangle edge
  was outlined; `NotVisual` skips `OutlineClone`.
- **Minimal mode:** views select only with `SelectInViews` (the full UI);
  `--select` needs `--ui full`.
- **Input:** ⌘ toggles in a browser (`mod_key` on `Platform::Web`); Esc
  cancels a marquee and the shell leaves the tool alone while one is
  drawn; a Space-hold pan press doesn't select; a click on the ground is
  empty space; a flat marquee picks; boxes are clipped at the near plane.
- **Tree:** Enter and Space select the cursor row; right and middle
  clicks don't select. `select::select` changes `Selection` only when the
  pick changes it.
- **Footer:** enabled while the selection is part of a model; Isolate
  forgets a reloaded world.
- **Outline:** one clone per mesh (a map), active only with clones, the
  overlay sized to `OutlineMask::rect`; `place_mask` writes only changes.
- **Tests:** a regression test for each (checked to fail without the fix
  for the wireframe and the near-plane clipping).

## 2026-10-21: M5b inspectors (D-77 to D-82)

- **Frame:** `InspectKind`, `Inspected` and content markers; the body
  rebuilds for a new subject or SDFormat reply; pin; a removed subject
  follows its respawn or shows "(removed)" with Dismiss.
- **Inspectors:** Model (live world pose, Physics, Plugins, parts), Link
  (live pose in the model, inertial, flags, geometry; Select model, Show
  inertia, Show collision), Light (live pose, parameters; Focus, Hide),
  Joint (live position, velocity, effort; definition; Select parent and
  child, Show axis), Sensor (pose on the link, output; View in Sensors),
  multi-selection (count, kinds, shared pose).
- **Data:** gz-core's SDFormat reader keeps model and link flags, model
  plugins, sensor outputs and joint parents, children, limits and
  dynamics; lights keep specular and attenuation; `GeometryDesc::kind_name`,
  `JointKind::as_str`; `gz_bevy::joint_state` subscribes to a model's
  joint states while a joint is inspected.
- **Selection:** `Selection::parts` for joints and sensors, selected from
  the tree; focus frames a selection without meshes at its position.
- **Live check** (`--release`, Jetty `diff_drive`, `--select
  vehicle_blue`): the Model inspector shows the live world pose, Physics
  from the SDFormat, the DiffDrive plugin, 4 links and 3 joints.
- **Tests:** frame (subject, pin, respawn, removed and Dismiss), each
  inspector's values and actions on a fixture scene, joint states,
  sensor rows selecting, the SDFormat extras; workspace 631 → 646
  passing; coverage 98.4% lines. check.sh passes.

## 2026-10-21: M5b review fixes (D-77 to D-80 amended)

- **Multi:** the body rebuilds when the selection changes (its count,
  kinds and Focus went stale).
- **Removed subjects:** a part selection replaces one; Dismiss clears
  parts; a pinned removed subject stays until Dismiss; a part follows its
  owner's respawn as a part; the Pin button follows `pinned`.
- **Joints:** `Joint::model`, the declaring model, for `WorldExtras::joint`
  (it matched nested models' joints of the same name, in hash order),
  `joints_of`, the tree and the Model inspector's count, so cross-model
  joints are their declaring model's; a model's Gravity counts its own
  links; ball joints read in degrees; limits carry units.
- **Actions:** Focus and Select model replace parts; Hide reads Show while
  hidden; focus on a part frames its owner.
- **Tests:** one for each (the multi rebuild and the nested-joint lookup
  checked to fail without the fix); workspace 646 → 654 passing; coverage
  98.4% lines. check.sh passes.

## 2026-10-22: M5b second review fixes (D-77 to D-80 amended)

- **Order:** `choose_subject` runs after `prune_selection` and reads the
  selection's live entries only, so a removal shows "(removed)" and a
  respawn is selected again whatever the system order (the harness ran
  them in the right order by luck; checked to fail without).
- **Pin:** off for a multi-selection; unpinning goes back to the selection.
- **Parts:** Follow and Track take a part's owner (`first_target`).
- **Joint:** Select parent and child only for ends in the scene.
- **SDFormat:** flags in any case; `__default__` topics are none.
- **Pose rows:** X Y Z R P Yw (§3.5). Joint-state topics and body rebuilds
  run on changes only. Workspace 654 → 659 passing; coverage 98.4%
  lines. check.sh passes.

## 2026-10-22: M5b third review fixes (D-77, D-80 amended)

- **World:** a World body built again (after a deselect) reads the
  server's fields and the phase's Reset at once, not at the next status
  change; World, Light and Multi bodies don't rebuild for a new SDFormat
  (they don't read it).
- **Subject:** a respawn is taken up only while pinned or with nothing
  else selected, so a selection in the same frame wins; a subject shown
  removed looks for its respawn only as entities arrive, and the live
  selection is built only when the subject changes.
- **Pin:** off for the World too.
- **Multi:** Focus is on for joints and sensors alone (they frame their
  owners, D-79).
- **Sensor:** Topic reads "—" until the SDFormat arrives.
- **Joint:** the JointStatePublisher's `<topic>` is subscribed when set;
  the Axis row shows the SDFormat's `xyz` and its frame; the wanted
  states update on changes only.
- **Tests:** one for each (the five UI ones checked to fail without the
  fix), plus reconnect and world change for joint states; workspace
  659 → 667 passing; coverage 98.4% lines. check.sh passes.

## 2026-10-21: M5c transform tools (D-83 to D-88)

- **Server:** `Dockerfile.gazebo` builds the websocket server system with
  `docker/websocket-server/binary-requests.patch` (gazebosim/gz-sim#4093)
  and installs it over every copy; the connection probes `/gazebo/worlds`
  with a `0x00`/`,` payload on Ready and sets `GzStatus::edits`.
- **Edits:** `gz_bevy::edit` (`SetPose` to `set_pose`, `Held`, `movable`);
  `apply_poses` skips held entities.
- **Gizmo:** `gz_bevy::view::gizmo`, Translate (arrows, planes) and Rotate
  (rings), screen-space picking, X/Y/Z, Shift snap, Esc, readout; sends at
  10 Hz and on release. The UI gates the tools on `edits` and styles the
  gizmo and readout.
- **Inspector:** a movable model's pose is number fields when
  the server takes edits; commits send `SetPose`; the body rebuilds when
  the probe's answer changes.
- **Left:** the joint command slider (no client publish, D-88).
- **Live check** (`--release`, Jetty `diff_drive`): a scratch client sent
  `set_pose` with round values (zero bytes) to the patched server on 9011,
  got `true`, and `pose/info` read 2.000 0.500 0.325; the stock server on
  9010 answered the probe empty. `--ui full --select vehicle_blue`: on 9011
  the pose is fields showing that pose and Translate and Rotate are on; on
  9010 the pose is text and the tools are disabled.
- **Tests:** probe and binary requests, `set_pose` encoding, holds, gizmo
  picking and drag math, arrow and ring drags through the plugin, snap and
  Esc, tool gating, readout, pose fields and commits; workspace 667 → 687
  passing (on M5b's fixes); coverage 98.3% lines. check.sh passes.
- **Review fixes:** lights and models with a plane aren't movable (gz-sim
  moves neither); the probe needs a `StringMsg_V`; a drag's Esc doesn't
  also switch to Navigate and the tool keys wait for it; ⌘/Ctrl+Z isn't Z;
  an unmoved release sends nothing; a scrub's release commits its own value
  and each step holds only a moment; pose fields and the gizmo read this
  frame's transform; a reconnect still asking keeps the tool; the gizmo's
  state changes only when it does. Tests for each.
- **PR review fixes:** an arrow pointing at the camera or a ring seen edge
  on isn't offered, and a drag freezes while its axis is end on (it threw
  the model metres); a ring turns the selection about the gizmo's centre;
  holds last until the server answers each `set_pose` (3 s at most), then
  0.5 s; a scrub's held-back step goes out when its time is up; the fields'
  six numbers stand while the pose is theirs (pitch past ±90° flipped); a
  scrub that changed nothing commits nothing; a plane found late turns the
  pose back to text and blocks commits. 687 → 690 tests.

## 2026-10-22: M5d undo (D-89; D-37, D-74 amended)

- **Stack:** `gz_bevy::undo`: `Record` messages (by entity) become edits
  kept by scene path on the `UndoStack`; `UndoCommand` undoes or redoes
  the top one. A move goes back as a `set_pose` with the stream held a
  moment, and waits while the server doesn't take edits; a hide needs no
  server. An open edit (a scrub) takes the steps after it; one that ends
  where it began is dropped.
- **Recorded:** gizmo drags (Esc and unmoved releases record nothing),
  inspector pose commits, the tree's eyes, the inspector's Hide or Show,
  Hide others, and Isolate on and off (its saved state, now
  `gz_bevy::undo::Isolated` by path, comes back with it).
- **Keys:** ⌘Z and ⇧⌘Z in the shell's `shortcut_keys`, not mid-drag.
- **Dirty:** the world button's unsaved dot shows while the stack isn't
  where it was at load.
- **Worlds:** a new server or world clears the stack; a Reset forgets the
  moves (gz-sim put them back) and keeps the hides. Reset itself isn't
  undoable (the server can't put a world back).
- **Left:** the World menu's Undo and Redo items and Save (M8), Delete's
  chip (M8).
- **Tests:** the stack (undo, redo, scrub merging, without edits, by path
  across a respawn, Isolate, worlds and Reset), a gizmo drag and Esc,
  ⌘Z and ⇧⌘Z on an inspector scrub with the dot, and each visibility
  edit; workspace 690 → 698 passing; coverage 98.3% lines. check.sh
  passes. Not checked live (⌘Z needs keys in the native window).
- **PR review fixes:** only a scrub's own field's steps join its edit
  (a scrub cut short swallowed the next drag), and an edit closed with no
  change goes; ⌘Z waits for a scrub as for a drag; `UndoSystems` runs
  after the gizmo and the keys; a reconnect's new scene forgets the hides
  and Isolate (`GzIndex::scenes`), and the sim's iterations going back
  (another client's Reset, a restarted server) forget the moves; after a
  Reset, undoing the hides left is clean again; the spec's undoable Reset
  lines, D-16, D-74 and D-89 updated. 698 → 701 tests.

## 2026-10-22: M6a split views (D-90 to D-92)

- **Cameras:** each view camera has a `ViewId`, its own `ViewPlacement`
  (now a component), outline mask (`Outlining` says whether anything is
  outlined) and render layer; `ViewSlots` and `render::spawn_views` give
  each view a camera dressed for the environment, a copy of the first; a
  new environment respawns them keeping id, placement and state; a cubemap
  sky installs on every camera. A dropped view's camera stays, off.
- **Routing:** `ViewRouting::{hovered, active}`; presses, the wheel and
  gestures go to the hovered view, a drag keeps its view, keys go to the
  hovered else active view; the gizmo shows and picks in the active view
  (its layer), and its drag and readout keep their view.
- **Layout:** `viewport::views`: the five presets as a flex grid of view
  nodes with share dividers (drag within `viewport_min`, double-click
  reset, accent), `LayoutCommand` from the Layout popover (thumbnails,
  Maximize/Restore, Close view) and ⇧Space, ⌘W, ⌥⌘→/↓; Close view
  renumbers the cameras; the floor rule's width follows the preset
  (`ShellLayout::floor_w`), then Single when it still doesn't fit.
  `gzweb --layout` and `?layout=`.
- **Per view:** rail, cube (its own cube scene and layer), pills, outline,
  marquee and the accent frame on the active view; `ViewCameras` replaces
  `MainView`.
- **Live check** (`--release`, Jetty `diff_drive` on 9011, `--ui full
  --layout quad --screenshot`): four views of the world with their own
  compact rails and cubes and the active view's frame; Single unchanged.
  Pointer and keys in a split layout not tried by hand.
- **Tests:** two views' pointer and key routing, new and respawned view
  cameras, the gizmo's layer; presets and placement, divider drag, clamp
  and reset, hover and active, a rail and cube per view, Maximize, Close
  view and splits, the floor; workspace 701 → 710 passing; coverage 98.3%
  lines. check.sh passes.
- **PR review fixes:** a rail's Close view and Maximize act on its own
  view (the commands carry it, and a press on a view's controls is that
  view's even over the next one); the active view stays among those
  showing (Maximize makes it the maximized one); views keep
  `viewport_min` as the area narrows (`fit_share` each frame, the gap left
  out of the length); Close and splits wait for the preset to show and for
  the mouse; the gizmo grabs only where it was drawn; layout changes show
  the views' images at once (after placement); the grid hides without a
  world; views clip; one studio cubemap; a cube sky isn't re-made; new
  views start free; the floor while maximized is one view's; thumbnails
  are buttons (Enter, Space); §9.3 amended by D-90. 710 → 716 tests.

## 2026-10-23: M6b inactive views, per-view overlays, Link cameras (D-93 to D-95)

- **Inactive views:** with several showing, a view neither active nor
  hovered shows a 34 px face marker (the face it looks at) at 80% for its
  cube and hides its rail and chip; hovering fades them back over 100 ms
  (`views::Reveal`; the connect card's subtree fade is now
  `widgets::fade`).
- **Overlays:** each view camera has its own `OverlaySet`; `OverlayCommand`
  acts on one view or all; the `Overlays` resource is what any showing
  view has on; each overlay draws on its own render layer (a gizmo group
  per overlay, collision meshes, wireframe lines) that a view sees while
  it's on there. The rail's menu and badge are its view's; Apply to all
  views. Wireframes are line meshes everywhere.
- **Link cameras:** pans, zooms, fly, focus and home carry to the other
  views' pivots and reach; orbits and snaps don't. Shared selection stays
  on (disabled).
- **Quality:** every view keeps the full stack (see the review fixes for
  the measurement). `gzweb --no-vsync`.
- **Live check** (`--release`, a fresh patched Jetty `diff_drive` on 9011,
  `--ui full --layout two-columns --overlays frames,joints --screenshot`):
  both views draw frames, joints and the grid; the right view shows its W
  face marker and no rail. Not tried by hand: the hover reveal, the
  per-view menu, Link cameras.
- **Tests:** the face marker and reveal, per-view overlay commands and
  layers, a rail per view's overlays and Apply to all, Link cameras (pan,
  zoom, orbit, home; the checkbox), the active view's cascades, line-mesh
  wireframes on their layer; workspace 716 → 722 passing; coverage 98.3%
  lines. check.sh passes.
- **PR review fixes:** Link cameras carry a wheel zoom's zoom about the
  new pivot, land a running animation before carrying a pan or zoom, carry
  pivots and reach for pans and flies, leave a focus every view got alone,
  move a view's own animation with a carry, and unlink back to one view;
  a hidden rail restores its widgets' opacities, closes its menus and
  stops its cube camera; collisions are layered before visibility; the
  cascades span every showing view. **Measured again** with Bevy's render
  diagnostics (the window's frame rate is paced by the compositor):
  Single 2.56 ms High / 2.27 Low, Quad 5.55 / 3.84; dropping the
  environment light saves ~0.6 ms in Quad (refiltered every frame per
  camera), SSAO, bloom and TAA nothing measurable. 722 → 729 tests.

## 2026-10-23: M6c one splitter (D-96)

- **One splitter:** the §5.18 widget is controlled. Its owner keeps the
  pane's size and range in the `Splitter` and applies `SplitterMoved`
  (drags, arrow keys, Home, End) and `SplitterReset` (a primary
  double-click without a drag). The side splitters, the slot splitter,
  the view dividers and the gallery own one each; `ShellLayout::drag` is
  now `width_range` and `resize`, and the three copies of the drag and
  double-click code are gone.
- **What changes for the user:** every splitter is a tab stop, moved by
  the arrows, Home and End; the slot splitter's double-click resets the
  split; a view divider's drag starts from the share as laid out; the
  slot splitter leaves the tab order with one slot.
- **Tests:** the widget asks and a test owner records (drags from their
  start, at UI scale, keys, double-clicks, the control bundle); the side
  splitters' and a divider's keys and ranges; the slot splitter's reset,
  range and visibility; the gallery's pane; workspace 729 → 735 passing;
  coverage 98.3% lines. check.sh passes. Not tried by hand.
- **PR review fixes:** a press focuses a splitter (kept: the usual
  behaviour), so its keys take plain presses only and Ctrl+→ steps the sim
  again after a drag; a key steps from the size it last asked for (two in
  a frame both count); a drag or key during start-up clears `startup` as
  before; the slot splitter's own `Visibility` is gone (`hide_undisplayed`
  already hides it), and the wrong note that collapsed panels stay in the
  tab order is dropped; the side-key test reaches the floor, the divider
  tests check the laid-out drag start and the taken key; §5.18 and §3.3.2
  amended. 735 → 737 tests.

## 2026-10-23: M7a panel hosts, footers, shared subscriptions, Topics (D-97 to D-99)

- **Topics groundwork (D-97):** gz-proto embeds the `gz.msgs` descriptors
  (85 KB) and `gz_core::echo` prints any message by its type name in
  text format. gz-bevy keeps the server's topic list (`GzTopics`,
  `GzCommand::RefreshTopics`), shares subscriptions by holder
  (`GzSubscriptions`; a world topic the client takes is tapped), and
  counts each received topic's messages and rate (`GzTraffic`). The joint
  states hold their topic through it.
- **Hosts and footers (D-98):** the slots are `PanelHost`s, rebuilt the
  same way, and `ShownPanels` replaces the layout's slot checks in
  `panel_shown`, the activity bar and the header keys. A panel's footer
  (`footer_action`, `footer_content`) shows under every state, its
  actions follow `PanelStatus`, and an unavailable one says why; the
  Scene footer moved onto it.
- **Topics (D-99):** filter, chips, the list with rates, the echo while
  shown ("Waiting for first message…", "Topic ended"), Copy, Publish and
  Services unavailable with a reason, `gzweb --echo`.
- **Native clipboard:** gzweb turns on Bevy's `system_clipboard`, so
  Copy (and the text fields) reach the system clipboard natively too;
  `deny.toml` allows BSL-1.0 for the two Windows-only crates under it.
- **Review fixes (PR #46):** the echo prints a long `bytes` field as its
  size and cuts lines at 2,000 characters (a point cloud's data was one
  1 MB line); Copy copies the whole message, and is unavailable in a
  browser off https or localhost (Bevy's web clipboard throws there);
  rates redraw when measured (`GzTraffic::measured`), not on every
  arrival, and the list recomputes only when its inputs change; a key
  move scrolls its row into view; `GzWebPlugin` adds `TopicsPlugin`
  only if it isn't there; BSL-1.0 is an exception for the two crates,
  not a global allowance.
- **Flaky test fixed:** a hidden view's cube camera could stay on when
  its scene spawned in the frame the reveal reached 0 (one long frame);
  the camera is now checked every frame. It failed about one run in four
  on main under load; 10 runs in a row pass now.
- **Live check** (debug build, Jetty `diff_drive` on :9011): `--echo
  /model/vehicle_blue/odometry` (light) and `--echo
  /world/diff_drive/stats` (dark, the tapped world topic): 14 topics,
  rates for the topics received, the echo in text format, the footer.
- **Tests:** workspace 737 → 754 passing; coverage 98.3% lines. check.sh
  passes. Not tried by hand.

## 2026-10-24: M7b Plot (D-100, D-101)

- **Fields:** `gz_core::fields` lists a message's numbers by path and
  reads one (by index, or by `name` in a list of named messages), and
  its header stamp; `gz_core::echo` shares its decoding.
- **Recording:** `PlotData` is always on; each series holds its topic at
  30 Hz through `GzSubscriptions` and records at its message's stamp or
  the sim clock (`SimClock`, from the stats); a reset drops the samples;
  a topic that leaves the list marks its series.
- **Charts:** stacked, up to four series each in the fixed colours, a
  legend with live values, y ticks on round steps, the window chips,
  Pause, the crosshair readout. The plot is drawn on the CPU (`raster`:
  distance-based anti-aliasing, one blend a series, an sRGB table) into
  an image at its physical size, at most 30 times a second and only on
  a change.
- **Picker:** inline, topics then a topic's numbers with live values,
  and the chart a field goes to; Add field…, the header's +, and Topics'
  Plot field (now enabled: it opens the picker on the echoed topic in
  the second slot).
- **Shell:** header toggles (D-101: `HeaderAction::toggle`,
  `set_action_on`).
- **Export CSV:** `gz_bevy::save::save_text`: a Blob download in the
  browser, the working directory natively.
- **`gzweb --plot TOPIC:FIELD,…`** (repeatable) and `--plot TOPIC` (the
  picker on it); under Topics with `--echo`.
- **Live check** (debug build, Jetty `diff_drive` on :9011): odometry
  and stats charts (light), Topics over Plot (dark), the picker on the
  stats topic (light). Its odometry comes at 1 Hz (the world's
  `odom_publish_frequency`), and the vehicle isn't driven, so its lines
  are flat; the real-time factor's line moves.
- **Review fixes (PR #47):** a reset is the reported sim time going
  back, not the run-on clock (a stalled server wiped the samples), and
  the clock runs at most half a second ahead; tick labels keep the
  digits a small step needs; Pause lets go with the last chart and on a
  reset; a chart redraws only when its window or the samples it shows
  change, not on every message; a rebuilt header keeps a toggle
  pressed; the picker's rows rebuild only on a change; a field reads by
  reference, without copying the message; names that look like numbers
  or hold dots go in quotes in a path (`joint["3"]`); `--plot` splits at
  the first colon; the browser download keeps its URL until it starts.
- **Tests:** workspace 754 → 780 passing; coverage 98.2% lines. check.sh
  passes. Not tried by hand.

## 2026-10-24: M7c Assets (D-102)

- **Inserting:** `gz_core::insert` (shapes and lights as SDFormat, a
  model by URI as an `<include>`, the `create` request's
  `EntityFactory`); `gz_bevy::edit::create_request` and `create_reply`.
- **Placing:** `gz_bevy::view::place`: while `Placing`, a click in the
  views places at the surface under it (`PlaceAt`), Shift keeps it,
  drags still move the camera; `PlaceHover` follows the pointer for the
  preview. The shell's Esc waits while placing; the status bar's hint
  says how.
- **Fuel:** `gz_core::fuel` (search URL, models, URIs, thumbnails, sizes)
  and `gz_bevy::fuel` (`FuelSearch` → `GzFuel`, fetched off the main
  thread, a newer search replacing one under way; `FuelFetch` swaps in
  canned replies for tests).
- **The panel:** Shapes, Lights, Fuel and Local tabs; tiles and Insert
  arm; a translucent preview; each insert's row from sent to shown in
  the scene (or failed, dismissable); the Fuel search with thumbnails,
  Load more, "Fuel is unreachable" and Retry; inert, saying why, without
  a world that takes edits. `gzweb --assets TAB[:QUERY]`.
- **Live check** (debug build, Jetty `diff_drive` on :9011, the patched
  server): `create` of a box and of the Fuel `Euro pallet` (the server
  downloaded it into its cache), both answered true and showed in the
  scene; removed again after. Captures: the Fuel tab searching
  "warehouse" (light, with thumbnails) and the Shapes tab (dark).
  Placing by clicking in the view is tested, not tried by hand.
- **Review fixes (PR #48):** a native Fuel search runs on its own
  thread with a 10 s timeout (a superseded one held an IO thread for up
  to 40 s, and meshes load there); each new scene name settles one
  insert, the oldest; a reply after its insert timed out isn't taken as
  the next one's; a place moves off the surface along its normal (a box
  clicked onto a wall sat half inside it); an Esc a gizmo drag takes
  leaves placing armed; the settle check, the hover cast, the bars and
  the Fuel rows no longer run or rebuild every frame; the armed tile is
  pressed to AccessKit and each Insert names its model; a local model is
  named after its directory, past `model.sdf` or a Fuel version; the
  preview follows a theme change.
- **Tests:** workspace 780 → 801 passing; coverage 98.1% lines. check.sh
  passes.

## 2026-10-24: M7d Sensors (D-103)

- **Sensors:** the selected model's sensors (nested models' too) with
  their type badges and rates; each sensor's topic from its `<topic>`
  (or Gazebo's derived one) among the server's; the chosen one's
  preview, held only while the panel shows: camera frames (PNG, decoded
  on the async compute pool, the last dimmed until a new sensor's
  first), a lidar's scan from above (`crate::raster`, now shared with
  Plot), or the message as text; "Waiting for first message…" and "No
  data for 3.2 s". `gz_core::sensor` decodes a scan's middle ring and a
  PNG's size.
- **SDFormat wanted by holders:** `SdfWanted` is a set (the Scene panel,
  the Sensors panel), so one doesn't turn the other's off.
- **Inspector:** View in Sensors shows that sensor; Echo topic and Plot
  work once the sensor's topic is known.
- **`gzweb --sensors [NAME]`**, with `--select MODEL`.
- **Live check** (debug build, Jetty `sensors_demo` in a second container
  on :9012): `camera_with_lidar`'s camera streaming (light) and its
  lidar's scan (dark).
- **Teleop** isn't in yet: the websocket server takes no publications
  (D-88), so it waits on that decision.
- **Tests:** workspace 801 → 808 passing; coverage 98.1% lines. check.sh
  passes. Not tried by hand.

## 2026-10-24: M7d Teleop (D-104)

- **Server:** `docker/websocket-server/client-publish.patch` adds a
  `pub_in` operation to the websocket server (raw publish through a
  gz-transport publisher, its payload kept whole); `Dockerfile.gazebo`
  applies it after binary-requests.patch. Built as
  `gz-websocket-jetty-pub`.
- **Client:** the publish probe (an empty `gz.msgs.Empty` on
  `/gzweb/publish_probe`, then the topic list), `GzStatus::publishes`,
  `Transport::set_publishing` (whole `pub_in` payloads), and
  `GzCommand::Publish` going out only to a server that publishes.
- **Teleop panel:** targets from the SDFormat's drives and joint
  controllers (`gz_core::sdf` reads their topics) and the server's Twist
  topics; Keyboard (pad and keys), Sliders, Gamepad; arming, X and Stop;
  the Twist at 10 Hz (`gz_core::teleop`); `DriveArmed` stands the fly
  keys and the gizmo down. The Joint inspector's command slider.
- **Copy** goes through `CopyText`: the Topics tests wrote the real
  clipboard (the workspace build has `system_clipboard`) and two of them
  raced on it.
- **`gzweb --teleop`.**
- **Live check** (debug build, the new image with `diff_drive` on
  :9013): the probe says the server publishes; a scratch client's
  `pub_in` of a Twist on `/model/vehicle_blue/cmd_vel` drove the vehicle
  (odometry rising at its 0.5 m/s limit); a capture of the panel with
  `vehicle_blue` selected. Driving from the panel is tested, not tried
  by hand.
- **Tests:** workspace 808 → 820 passing; coverage 98.0% lines. check.sh
  passes.

## 2026-10-24: PR #49 review fixes

- **Teleop stops:** X, Stop, a new mode or target, hiding or a dropped
  world disarm, and nothing arms again until the pad and the stick let
  go (a held stick re-armed through X and Stop). The robot driven last
  gets a zero whenever the drive leaves it, a new target included; after
  a drop the zero waits until the server takes publications again (for
  the same world).
- **Keys:** a command chord (Ctrl, Alt, ⌘) doesn't drive, and the keys
  drive from the views or the panel only (`TeleopKeys`: a press or
  focus in the panel), not from another panel's list.
- **Analog:** the stick's command goes out early only once it moves 2%
  of a limit; "Driving: …" reads the command going out; Stop zeroes the
  slider thumbs.
- **Joints:** JointPositionController topics as gz-sim names them (the
  first joint, `<sub_topic>`, Actuators left out); command sliders sit at
  the joint's live position until moved; the Joint inspector builds its
  slider once the probe answers.
- **Sensors:** the topic list is asked for every 2 s while Sensors or
  Teleop shows; a topic whose type becomes known starts the preview
  again and subscribes again (a camera's `image`); the list no longer
  rebuilds on each stats tick.
- **Server patch:** publishers are kept per client, at most 64, and
  dropped with the last client that used them; a client that goes gets
  an empty Twist on each Twist topic it published on. Live check on the
  rebuilt `gz-websocket-jetty-pub` (diff_drive, :9013): a scratch client
  drove `vehicle_blue` at 0.5 m/s and left without a zero; the vehicle
  stopped (x held at 2.374). The earlier container had aborted in ODE
  hours after a scratch client left it driving.
- **Merged** M7c's CI fixes (clipboard-win 5.4.3).
- **Tests:** workspace 820 → 825 passing.

## 2026-10-24: PR #49 re-review fixes

- **Server freeze:** OnDisconnect published the zero Twist while holding
  `subscriptionMutex`; the server's own subscriber on that topic (a
  client echoing or plotting cmd_vel) runs on the same thread and takes
  the lock, so the server hung. The zeros now go out before the lock.
  Live check on the rebuilt `gz-websocket-jetty-pub` (diff_drive,
  :9013): one client watched `/model/vehicle_blue/cmd_vel`, another
  drove and left without a zero; the watcher got the zero and a new
  client then connected and read the vehicle stopped.
- **X** stops wherever the keys are (only not while typing) and zeroes
  the sliders, like Stop; the drive keys keep their routing.
- **Latch:** only the mode's own input holds re-arming off, so a resting
  stick doesn't block the pad.
- **Joints:** Teleop wants its target's joint states while it shows
  (deferring to the Joint inspector), so its sliders start at the live
  positions; `use_actuator_msg` is read as gz-sim does (a bool, with an
  `actuator_number`).
- **One topic refresh** for Topics, Sensors and Teleop.
- **Tests:** focus routing, X from another panel, a resting stick, a
  reconnect through Disconnected (and to another world), Teleop's joint
  states and the single refresh.

## 2026-10-24: M7e Console and pop-out windows (D-105, D-106)

- **Console:** the client's own log, newest first (`SystemMessages`
  now keeps our info and debug events, numbered, with the local time),
  the filter and level chips, Clear and Pause in the header, the badge
  for problems not seen yet, and a command line in the footer:
  `gz_core::cli` reads `gz topic` (-l, -i, -e, -p) and `gz service -s`;
  `gz_core::echo::encode_text` turns text format into a message;
  replies show as text, with "service not found" and timeouts. `topic
  -e` opens Topics beside the Console (`TopicsStart::beside`).
  `gzweb --console`.
- **Pop-outs:** `ShellLayout::popouts` (back to front) with pop out,
  Dock, Close, raise, move and resize, kept in the viewport area; a
  window is one more `PanelHost`, so the sidebar's host rebuild fills
  it; its header gets a grip, Dock and Close; edges and corners resize;
  a press raises. Pop out on every slot header and Plot's footer;
  Sensors' disabled header pop-out is gone. A popped-out panel's
  activity button shows a hollow ring. `gzweb --pop-out PANEL,…`.
- **Live check** (debug build, :9013 `diff_drive`): the Console with the
  connection's lines; Plot (charting `vehicle_blue`'s x) and the
  Console popped out over the view, with the rings on their buttons.
  The command line and the window drags are tested, not tried by hand.
- **Base:** this branch builds on M7d's. #47 to #49 merged into the
  branches beneath them, not main, which has only M7a (#46).
- **Tests:** workspace 829 → 840 passing.

## 2026-10-24: PR #51 review fixes

- **Dock** keeps the panel that moved up into the primary: it moves back
  down.
- **`topic -e`** opens Topics in the slot the Console isn't in
  (`TopicsStart::beside` is the asking panel).
- **Window drags** that start on a header button don't move the window
  (it carried the button along and clicked it: Close closed).
- **Raise** reads bevy_picking's press messages, so a press on a button,
  chip or field in a lower window raises it.
- **↑ / ↓** set the recalled line after the text input's own arrow
  handling, so the caret ends the line.
- **Pause** holds the log back, not the command line's answers.
- **Status chip:** the latest problem is kept apart from the 500-line
  list (info lines pushed it out); the Console's unread count comes
  from the log's totals.
- **Late replies:** within 30 s of a timeout, a call's reply is dropped,
  not taken for the next call; matching by service stays a documented
  limit (D-105).
- **Rows:** every held line keeps a row, shown or hidden by the chip and
  filter, so typing in the filter no longer rebuilds them.

## 2026-10-24: M8a command palette (D-107)

- **Palette:** ⌘K or the top bar's search; a 44px field; results under
  Commands, Entities, Topics, Panels and Settings, fuzzy-ranked, with
  the Gazebo GUI plugins' names as keywords; ↑/↓, Enter, click, Esc and
  a press outside; unavailable commands dimmed with their reason; the
  results follow the world while open. `gzweb --palette QUERY`.
- **Running:** a `shortcuts.json` command is a `RunBinding`, which
  `shortcut_keys` and gz-bevy's `key_input` read beside their keys;
  shown panels' actions trigger `PanelActionActivated`; an entity is
  selected and focused; a topic echoes in Topics. `sim.reset`,
  `ui.command_palette` and the `ui.theme_*` bindings now do something.
- **Live check** (debug build, :9013 `diff_drive`): the palette open
  empty, and on "veh" listing `vehicle_blue`, `vehicle_green` and their
  topics. Typing and running by hand not tried.

## 2026-10-24: PR #52 review fixes

- **⇧⌘R on the web** (Ctrl+Shift+R too) is the browser's hard reload,
  which still hands the key to the page: it would have reset the shared
  world. `sim.reset` is `web: null` with `web_alt` ⌥⇧R; the reload
  chords are in `web_reserved`.
- **Keys stay in the palette:** a modal tab group, and the field takes
  the keys back after a press on the card that runs nothing; it takes
  Esc as a menu does (`TakesEscape`), so an armed placement stays armed.
- **↑ / ↓** leave the caret at the end; new results scroll to the top
  and the chosen row is revealed once laid out; a ↑/↓ choice is kept by
  what it runs (two `left_wheel`s).
- **Enter** with nothing to run leaves the palette open.
- **Accessibility:** the chosen row is selected to AccessKit; the list
  is named "Results".
- **Refresh** watches what the items read of the status, not each
  stats message.
- **More commands:** the overlays without keys; the layout commands
  work offline; Play says it waits for the clock; more Gazebo names.

## 2026-10-24: review of fixes merged without one (#48, #49, #51)

- **Joint states:** Teleop's want could take the Joint inspector's away
  (hiding Teleop dropped it), and every selection change unsubscribed
  and resubscribed Teleop's. `JointStateWants` keeps one want per
  holder; the inspector's ranks first.
- **Server:** the leaving client's zero went to its own still-registered
  subscription, and its unsent messages stayed in `messageCount`, so the
  service loop never waited again. The locked cleanup now comes first
  (taking the unsent messages off the count), then the zeros. Live
  check on the rebuilt image: a client drove while watching its own
  cmd_vel and left; the server stayed at the sim's ~28% CPU, the
  vehicle stopped and a new client connected.
- **Console:** Clear while paused now clears the command line's lines
  too; a dropped connection ends the calls waiting ("the connection
  dropped") and forgets the late ones.

## 2026-10-24: M8b World menu and Snap popover (D-108)

- **World menu:** header (file, "unsaved changes · last saved", "saved"),
  the §7.1 items; what the server can't do says so; Undo and Redo by
  name; Connect to server… shows the connect card over the world, with
  Cancel.
- **Save:** `gz_bevy::world_save` asks `generate_world_sdf` and saves
  `<world>.sdf` (download in the browser), each top-level model at its
  authored pose (`gz_core::world_file`: last placed on the undo stack,
  else as loaded); marks the world saved. Save as… and Export open a
  dialog with "Use current poses". Export screenshot saves a PNG.
  `StatusNote` shows "Saved …" for 3 s.
- **Snap popover:** translate and rotate steps; scale reserved; World.
- **Flags:** `gzweb --menu world|snap`.
- **Live check** (debug build, :9013 `diff_drive`): both menus open on
  the live world. Saving by hand not tried (tested against a reply).
- **Tests:** workspace 847 → 856 passing.
- **Review fixes (#54):** gimbal lock in `sdf_pose`; the dialog keeps
  the keys; loaded poses survive a reconnect and forget a deleted model
  or another world; ⇧⌘S prevented in the browser; typed names keep their
  last part; "last saved" forgets another world; type-to-search in the
  World menu; long Undo labels end in "…"; hides aren't saved (D-108).

## 2026-10-24: M8c entity context menu (D-109)

- **Context menu:** a right click on a model in a view (no drag) or a
  right press on a tree row; selects it unless it is. §7.9's items;
  Rename, Apply force… and Edit model say why they can't.
- **Model edits:** `gz_bevy::view::model_edits` gets the model's
  SDFormat (`gz_core::world_file::model_sdf`); Copy, Paste and
  Duplicate `create` copies named `<name>_N` beside it, Delete sends
  `remove`. Undo removes or recreates (`Change::Exists`); Delete shows
  a 5 s Undo chip. ⌘C ⌘V ⌘D Delete; the palette.
- **Align ›:** `gz_bevy::view::align`, X/Y/Z by min, centre or max.
- **Submenus:** hover, click, Enter or → opens; ← closes; a choice
  closes the stack.
- **Flags:** `gzweb --menu context` (with `--select`).
- **Live check** (debug build, :9013 `diff_drive`): the menu opens on
  `vehicle_blue`. Deleting, duplicating and aligning by hand not tried
  (tested against server replies).
- **Review fixes (#55):** ↓ moves past a submenu's row; Delete again
  while one is under way does nothing; `remove` by id; undo waits for a
  model it's bringing back, which keeps its pose as loaded; Align holds
  its models; ⇧D doesn't fly on the web; the undo chip follows its
  edit's serial.
- **Tests:** workspace 861 → 879 passing.

## 2026-10-24: M8d message tray and workspaces (D-110)

- **Message tray:** the problem chip opens it; problems newest first,
  counts, Open console ›, Clear (`SystemMessages::clear_problems`).
- **Workspaces:** Build, Run, Debug, Review; Alt+1–4, the status bar's
  switcher, the palette; a switch keeps what it leaves; §8's defaults;
  each view's camera (`ViewOp::Restore`) and overlays.
- **Persistence:** Save current layout; `gz/ui/workspaces.json` in the
  config dir or `localStorage`; current workspace and theme on change;
  read at start (not over `--theme`; none for captures).
- **Flags:** `gzweb --menu workspace`.
- **Live check** (debug build, :9013 `diff_drive`): the switcher opens.
- **Tests:** workspace 879 → 890 passing.
- **Review fixes:** a file it can't parse is kept apart (`.bad`), one
  it can't read or a newer version's isn't written over, unknown
  workspaces are kept, writes are whole; a theme asked for isn't saved;
  the start flags go over the saved layout; an unsaved workspace starts
  as its default (Build as the app starts); a saved grid outlasts the
  scene's flag (`overlays::KeepGrid`); a view mid-animation is saved
  where it ends, with its axis view; the tray closes without its chip;
  Alt+1–4 wait out an edit; the tray counts its rows.
- **Tests:** workspace 890 → 899 passing.

## 2026-10-24: M8e the Workspaces panel (D-111)

- **Panel:** no activity button; Manage…, ⌘K and `gzweb --workspaces`
  open it. Cards (72×44 thumbnails, what each shows, Alt+N, "modified"),
  the current layout's summary, Save to …, Save as new…, Reset; the
  footer's note and Import / export layouts… (Export only).
- **Workspaces:** the user's own join §8's four (`user_N` in the file,
  names and order in `custom`), in the switcher and ⌘K; Delete; Reset
  puts back the layout as saved, then §8's default; Build unsaved takes
  the app's start as its layout; the panel stands over the layout it
  shows (`Beneath`), never saved.
- **Live check** (debug build, :9013 `diff_drive`): the panel's cards,
  summary and buttons.
- **Tests:** workspace 899 → 919 passing (912 before the review fixes).
- **Review fixes (#57):** what the panel stands in for moves with it on a
  swap and never repeats the other slot; a closed sidebar lets it go; the
  switcher keeps the user's workspaces before its separator; cards change
  in place and keep the keys; Save's tip follows; raising a window isn't
  a change; Reset to the default is kept; unique names on load; the
  footer says when the file is left alone; the name row takes its Esc;
  cards are list items.

## 2026-10-25: M9a editing a model in place (D-112)

- **Entering:** Edit model (inspector, context menu), a double click
  with Translate or Rotate, `gzweb --edit`; pauses a running sim (once
  its stats are known), joints and frames on, the selection cleared.
- **While editing:** picks are the model's links; the selection holds
  its parts; other meshes at 25% (faded copies); the tree opens the
  model and dims the rest; the EDITING pill with Done; the status bar's
  hint; the model edits refuse.
- **Leaving:** Done, or Esc (clearing the selection first); overlays
  back, the sim resumed if entering paused it.
- **Live check** (debug build, :9013 `diff_drive`, `--select vehicle_blue
  --edit`): the pill, the dimming, the tree, the pause and the hint. The
  capture left the sim paused; a Play request resumed it.
- **Tests:** workspace 919 → 927 passing.
- **Review fixes:** the editing no longer marks itself changed every
  frame (the tree's open row stays closed when closed, the dimming
  scans only on a change); a dimmed tree row opens no menu; entering
  another model keeps the sim paused; Done turns off only the joints and
  frames it turned on, new views get them, and workspaces leave them
  out; Edit model only for a top-level model in the inspector, and
  unavailable on the edited one; the camera pivots on dimmed models;
  ⌘K's Navigate isn't Esc; double clicks start over after anything else
  and get through the gizmo's handle; faded copies follow their
  materials, terrain fades; undo, redo and inserting refuse while
  editing. Workspace 927 → 937 passing.

## 2026-10-25: M9b the Parts tab (D-113)

- **Parts:** `gz_core::parts`: links, joints, visuals, collisions,
  sensors and plugins as SDFormat elements; what's shown of each is its
  element parsed; joint rules; the draft and its undo.
- **gz-bevy:** `view::editing::parts`: a part's entities (links,
  visuals) and sensors, the extras with the draft's parts (kept across a
  reply), undo and redo while editing, the draft dropped on Done.
  `PlaceAt` carries the mesh clicked.
- **Parts tab:** six tiles and the shape, joint and sensor selects; a
  click adds a link, or a visual, collision or sensor on the link
  clicked; a joint's two clicks with its rubber band; Plugin's fields;
  a Shapes tile adds a link while editing; the World menu names the
  draft's undo; Done says the parts weren't kept.
- **Live check** (debug build, :9013 `diff_drive`, `--select vehicle_blue
  --edit --assets parts`): the tab beside the pill. The capture paused
  the sim; a Play request resumed it.
- **Tests:** workspace 937 → 950 passing.
- **Review fixes** (PR #59):
  - a joint's parent is forgotten when another model is edited or undo
    takes its link back;
  - joints count world-fixed and nested links;
  - a joint or collision waits for the server's SDFormat;
  - place clicks reach the edited model through dimmed ones;
  - plugin text is escaped;
  - undo unselects a removed joint or sensor;
  - armed Parts tiles are pressed to AccessKit;
  - `patch_extras` no longer marks the draft changed every frame;
  - the preview stands in the model's or link's frame;
  - Esc with Plugin armed only disarms;
  - a cone's centre of mass and a capsule's inertia.

  Tests: 950 → 958 passing, each new one failing without its fix.

## 2026-10-25: the server's frame limit (D-109 amended)

- **Found** writing M9c: the websocket server (libwebsockets' default
  receive buffer) hands a frame over 4088 bytes to its parser in pieces.
  Measured on Jetty: 4088 bytes goes, 4089 is lost. A `create` from a
  real model's SDFormat is lost while its `remove` goes through:
  vehicle_blue is 9 KB, 6 KB compacted. Deleting it and undoing lost it.
- **Fix:** the transport refuses frames over `MAX_FRAME`; `model_sdf`
  compacts; Delete, Duplicate, Paste and their undo and redo check the
  `create` first and do nothing, saying why, when it won't fit. Failed
  model edits show in the status bar.
- **Review** (PR #60):
  - 4088 bytes is needed but not enough: the server parses what one
    4096-byte read holds. A frame under the limit sharing a read with
    another, or arriving over several reads, is cut too (shown on a
    libwebsockets 4.3.5 server set up like gz's). The docs say so.
  - Compacting keeps blank text that means something.
  - The Console refuses a call or publish that long, saying why.
  - The reason gives exact bytes.
  - M9c's write-back is paused until the server joins a frame's pieces.
- **Left:** the server should join a frame's pieces
  (`lws_is_final_fragment`, `lws_remaining_packet_payload`); until it
  does, M9c's write-back is limited to models that fit.

## 2026-10-25: M9c write-back (D-114)

- **Server:** `docker/websocket-server/whole-messages.patch`, the third
  local patch: the websocket server joins a message's pieces per
  connection (`lws_is_final_fragment`, `lws_remaining_packet_payload`)
  before it parses the frame, up to 16 MiB. Not upstream.
- **Probe:** after the edit probe's yes, a 12,288-byte `/gazebo/worlds`
  request; the world list back means the server joins pieces
  (`GzStatus::long_frames`). The transport then takes frames up to
  `MAX_LONG_FRAME`, and `edit::too_long` uses the server's limit (model
  edits and the Console benefit).
- **Write-back:** `gz_core::parts::write_parts` and `check_model`;
  `view::editing::parts::write` (Done asks for the world's SDFormat,
  writes and checks the model, removes it, records one "edit X");
  `undo::Replacing` creates it once the old one has gone (Done, and undo
  and redo of it). Done refuses on a server that cuts long messages.
- **UI:** the pill's Discard while there are parts; writing's notes in the
  status bar; "N parts discarded".
- **Live check** (`gz-websocket-jetty-pub-frames` on :9031 and the stock
  `gz-websocket-jetty-pub` on :9032, both `diff_drive`):
  - a 9 KB `create` goes on :9031 and is lost on :9032;
  - 40 1.5 KB requests in one write: 40 good replies on :9031, 26 on
    :9032;
  - `write_parts` on vehicle_blue's live SDFormat (a link, visual, IMU and
    fixed joint, 6.8 KB), then remove and create on :9031: the model came
    back with them, and the client's pill counted 5 links and 4 joints;
  - the debug client's probe: "takes long frames whole" on :9031, "cuts
    long frames" on :9032.
  Not driven live: Done's clicks in the client (no capture flag adds
  parts); the tests cover them.
- **Tests:** workspace 966 → 974 passing.
- **Merged M9d** (#61): `write_parts` writes a reparent as its joint's
  `<parent>`; notes count changes.
- **Review fixes** (PR #62):
  - "Wrote X" waits until the model shows again;
  - an ignored `remove` or a lost `create` says so and points to undo;
  - a pending `create` waits for a world and goes on a Reset or new world;
  - undo waits until the model shows;
  - the long probe is 64 KiB;
  - joint ends are escaped;
  - D-114 says joints come back at rest.

## 2026-10-25: M9d the inspectors' quick actions (D-115)

- **Link inspector:** Add visual, Add collision and Add sensor, a second row
  of quick actions, put the Parts tab's shape or sensor at the link's
  origin while its model is edited.
- **Joint inspector:** Reparent has the next click pick the joint's new
  parent link, with a line from its child to the pointer, the status bar's
  hint, and D-113's tree rules; `Part::Reparent` is a draft step that undo
  takes back.
- Outside editing both are disabled, saying why.
- Built while M9c was paused on the server's frame limit; an agent is
  finishing M9c separately. M9c's write-back needs to handle
  `Part::Reparent` (it has no element: it changes its joint's `<parent>`).
- **Review fixes** (PR #61):
  - a Shapes tile (or another insert) cancels an armed Reparent;
  - a draft joint's armed Reparent is checked after the draft is back in
    the extras;
  - the inspector's actions keep the link selected;
  - a removed subject's buttons say so;
  - the leaving note counts changes.

## 2026-10-25: M10a a static environment filtered once (D-116)

- **Plan:** M10 is five PRs (UI_PLAN): M10a the environment map, M10b
  keyboard and AccessKit, M10c UI scale and reduce-motion, M10d
  layout-aware letters on the web, M10e performance and the §13
  checklist.
- **gz-bevy:** a view lit by the studio or a cubemap sky gets a
  `render::StaticEnvironment` instead of Bevy's
  `GeneratedEnvironmentMapLight`. `render::prefilter_environments`
  spawns one `EnvironmentFilter` entity per cubemap with Bevy's
  generator, gives every view showing it the maps it makes, and drops
  the entity once the render world reports the passes ran
  (`report_filtered`, a render-world system after Bevy's
  `filtering_system`). Views added later and respawned cameras reuse
  the maps; a cubemap no view shows is forgotten. The atmosphere's
  environment is unchanged (real-time).
- **Live check** (`--release`, a fresh Jetty on 127.0.0.1:9031, a
  temporary diagnostics patch printing Bevy's render diagnostics):
  - The lightprobe passes ran once per cubemap and then not at all
    (0 measurements a second, 120 before), through a world change from
    `diff_drive` (studio) to `sky.sdf` (atmosphere: every frame, as
    before) to a `diff_drive` with a generated 64 px DDS cubemap sky
    to `diff_drive` again, in Quad. No warnings or validation errors.
  - Captures before and after match: mean difference 0.03 (Single),
    0.04 (Quad) and 0.07 (Quad, cubemap sky) of 255, the UI's clock
    aside.
  - Frame times (median of three alternating runs, `--no-vsync`):
    Quad 6.38 → 5.46 ms, Single 3.13 → 3.04 ms. Before, the radiance
    and irradiance passes took about 0.6 ms of GPU per camera per frame.
  - Not tried by hand: adding or closing a view at runtime (the tests
    cover it), the browser (wasm32 builds; Bevy's generator needs
    compute, so WebGL2 has no environment light, as before).
- **Tests:** a static environment filtered once for every view (the
  filter's maps shared at each view's intensity, the filter dropped on
  a report and kept on an early one, a later view lit without a new
  filter, an old cubemap forgotten); the environment tests now check
  that no view carries Bevy's generator. Workspace 967 passing; coverage
  97.4% lines. check.sh passes.
- **Review fixes** (PR #63):
  - a view's light follows its environment's intensity and cubemap, and
    goes with it;
  - a filter despawned before its report is made again;
  - a test checks `report_filtered` is in the render world's schedule;
  - D-116 notes that a cubemap changed in place isn't refiltered.

## 2026-10-25: M10b keyboard and AccessKit (D-117)

- **Audit:** a test walks every tab stop and every node with a control's
  role in each panel with content, each inspector and the palette. It
  found a leaf's caret and an unhideable row's eye as nameless buttons,
  the Workspaces panel's scroll area as a nameless tab stop with no keys,
  and no focus ring on the Topics list, the Console log or a tree with no
  cursor. All fixed; the test keeps them so.
- **Arrows within groups:** `focus::arrow_keys`. The activity bar is an
  `ArrowGroup` (↑ ↓, Home, End; moving opens nothing). Radio groups (the
  tool group, the rail's drag group, the chips) move from the focused
  button and check where they land; Bevy's own group checked from the
  checked one and left focus behind. A modified arrow stays a shortcut:
  ⌘→ on a focused tool used to be taken by Bevy's radio group.
- **The views:** each view is a tab stop before its rail, an AccessKit
  application ("View 1"); focus there gives the views the keys and makes
  it active; its ring is `FocusFrame`, 4 px inside its edge. Bevy renames
  a node whose image changes, so `keep_view_access` puts the view's back.
- **Flag:** `gzweb --keys tab,shift+tab,down` presses chords once live.
- **Live check** (debug build, own `gz-websocket-jetty-pub` `diff_drive`
  on :9041, `--keys`): 32 Tabs land on the view with its ring (light);
  then G turns the grid off (dark); 12 Tabs and ↓↓↓ move along the
  activity bar with the Scene panel still open; Tab Tab → → focuses and
  checks Translate.
- **Tests:** workspace 981 → 990 passing.
- **Review fixes** (PR #64):
  - the view's AccessKit role and name are put back in place, keeping its
    bounds;
  - a view Tab focuses, or its rail, takes the single-key shortcuts from
    the hovered one, and is active;
  - `--keys` sends logical keys and text, releases a chord on leaving
    Live, and holds `--screenshot` until done.

## 2026-10-25: M10c UI scale and reduce-motion (D-118)

- **UI scale:** ⌘K's Settings group lists "UI scale: 80%" to "200%" (the
  one in effect says "current"); `gzweb --ui-scale PERCENT` (80 to 200)
  and `?ui_scale=` pick it at start. `Theme::set_scale` keeps any value to
  80 to 200%.
- **Pixels:** the context menu, the selection marquee and the outline
  overlay took window pixels as UI pixels, so above 100% they landed the
  scale times too far; they divide by `UiScale` now. The Scene tree's
  first row pool counts UI pixels. The views' placement already followed
  its node; tests now hold it through scale changes.
- **Reduce-motion:** "Motion: reduce / full / follow system", `--motion`
  and `?motion=`. Follow system reads the browser's
  `prefers-reduced-motion` (natively: full). Reduced, the camera's snaps,
  focus, home, look-from and mode turns land at once
  (`gz_bevy::view::ReduceMotion`), the card's fade and the views' reveal
  are instant, and the Assets panel's bars hold still.
- **Persistence:** `ui_scale` and `motion` in `workspaces.json`; one asked
  for by a flag wins and isn't saved.
- **Live check** (debug build, own `gz-websocket-jetty-pub` `diff_drive` on
  :9051, `--screenshot`): 80%, 100% and 200% at 1440x900, text and icons
  sharp; 200% at 2880x1800 with `--menu context --select vehicle_blue`
  looks as 100% does at 1440x900, the outline on the model (before the fix
  it was drawn a view away from it); 125% with `--palette "ui scale"`;
  150% with `--motion reduce --view top --palette motion`.
- **Left:** the full UI needs 1024x640 UI pixels, so above 100% a smaller
  window (200% at 1440x900) clips the top bar's right end as a narrow
  window does.
- **Tests:** workspace 981 → 993 passing; coverage 97.5% lines. check.sh passes.
- **Review fixes** (PR #65):
  - the gizmo's snapping readout divides by the UI scale;
  - minimal mode keeps the motion setting in its views (`--motion`, the
    browser's preference) and refuses `--ui-scale`;
  - the file's scale reads leniently (a hand-written percentage, or a bad
    value dropped without losing the file).

## 2026-10-25: M10d layout-aware letters (D-119)

- **gz-bevy:** `view::layout::KeyLayout` learns what each key typed as it
  went down (`KeyboardInput.logical_key`, PreUpdate after Bevy's input),
  and `layout::Keys` reads `ButtonInput<KeyCode>` through it. The input
  map matches a letter by the key that types it, and digits, arrows and
  named keys by place. A Latin letter in either case is that letter, other
  ASCII is no letter, and anything else (Cyrillic, a dead key) falls back
  to the key's US letter; with Ctrl, Alt or ⌘ held a non-letter keeps what
  the key typed before.
- **Positional:** the fly keys and `teleop.drive` (`input::POSITIONAL`)
  keep W A S D Q E's places and win a plain press of their keys (Colemak's
  F flies up; ⇧ with it follows). A gizmo drag holds the fly keys, so
  AZERTY's Z constrains without flying. The gizmo's X, Y and Z and
  Teleop's stop X follow the layout.
- **Browser:** the canvas listener swallows chords by the same rule
  (`InputMap::web_key`, from `code` and `key`). Chromium's
  `navigator.keyboard.getLayoutMap()` seeds the layout at start.
- **Labels:** letters read as before. The HUD's fly hint
  (`hud::update_hint`), the Teleop pad's keys, their accessible names and
  the pad's note show the layout's caps once known (`teleop::pad_caps`).
- **Live check:** Xvfb (`:97`, lavapipe) with a fresh Jetty
  `diff_drive` on 127.0.0.1:9047: the full UI with `--teleop` renders and
  drives as before (the pad reads W A S D on an unlearned layout). The
  Xvfb server ignored keymap changes (`setxkbmap`, `xkbcomp` and
  `xmodmap` left it US), so no real layout switch was tried. The layouts
  are checked with synthetic `KeyboardInput` carrying another logical
  key, through Bevy's input and the app's systems: Dvorak's `KeyU` typing
  g toggles the grid and its `KeyG` typing i toggles inertia; AZERTY's
  `KeyW` typing z flies forward and its `KeyZ` typing w doesn't; a drag
  constrains to Z on AZERTY's `KeyW` with the camera still; Dvorak's
  `KeyN` typing b picks Box select and with Ctrl toggles the sidebar,
  `KeyB` typing x does neither; Teleop stops on `KeyB` typing x, not on
  `KeyX` typing q; the pad and its note read Z Q S D after AZERTY's
  presses. With the learning system removed, the app-level tests fail.
  Not tried: a real browser (the wasm32 build passes clippy).
- **Tests:** what a press types (a Latin letter in either case, other
  ASCII, a fallback, a command chord), the layout learned from presses,
  the browser's keydown, and every letter-row key on six layouts (US,
  AZERTY, QWERTZ, Dvorak, Colemak, Russian) plain, with Shift and with
  Ctrl: no press fires two bindings and the fly keys keep their places;
  the app-level checks above; the hint's and the pad's caps. Workspace
  999 passing; coverage 97.5% lines (`view/layout.rs` 100%). check.sh
  passes.
- **Review fixes:**
  - a letter of another script typed without Alt, or a plain Latin
    letter after one, is a layout switch that forgets what was learned
    (Dvorak then Russian: Ctrl and `KeyS` is Save, not Open);
  - the canvas listener keeps its own `KeyLayout`, fed the same
    keydowns, so `web_key` and the app agree (a test runs both on one
    stream: a switch, Option chords, non-Latin keys with Ctrl);
  - caps are learned unshifted only; a key with no character keeps its
    letter; the modifiers come from the frame's events in order;
  - labels drop a plain letter only fly keys type (Colemak's F and R:
    the HUD hint, the rail's and the tools' tooltips, which now follow
    the map);
  - a fly key held as a drag ends needs a fresh press;
  - the layout sweep adds Alt and Ctrl+Alt (AltGr); D-119 records the
    Option limit in Firefox and Safari.
  Workspace 1006 passing; coverage 97.5% lines. check.sh passes.
