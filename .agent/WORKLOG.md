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
- CI's HTTP cache doesn't have the image yet: the Fuel job fetches it from
  fuel.gazebosim.org and fails while that is down.

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
