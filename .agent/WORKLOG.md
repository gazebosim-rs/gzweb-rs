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
