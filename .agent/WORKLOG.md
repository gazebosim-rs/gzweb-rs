# Worklog

Dated entries: what changed, and what was verified. Newest first.

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
- **Leftover:** the old `src/` is still on disk, because removing it was not
  permitted in this session. It is no longer built.

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
  keeps three's [0, 1] range. There is no spot light in the corpus yet.
- **Ignored fields:** gzweb ignores `Visual.scale`, `Visual.transparency` and
  model-level visuals. gzweb-rs matches the first; the other two are not
  exercised yet.
- **Camera-sensor worlds don't load in gzweb.** In worlds with a camera
  sensor (e.g. `sky.sdf`), gzweb's scene decode throws
  `no such Type '.gz.msgs.PixelFormatType'`, so it never reaches `ready`.

### Next

- **Corpus:** grow it with `lights.sdf`, `nested_model.sdf`, a mesh-heavy Fuel
  world and a PBR world. That gives the comparator something to say about
  meshes, spot lights and materials.
- **Phase 1:** reconnect, `wss://` (the native driver has no TLS yet), and
  sim-control services.
- **Collada:** implement it per the spike's recommendation.
