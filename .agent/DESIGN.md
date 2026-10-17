# gzweb-rs: Architecture & Design

Plan and parity rationale: [`PORT_PLAN.md`](./PORT_PLAN.md). Test harness:
[`PARITY.md`](./PARITY.md). History: [`WORKLOG.md`](./WORKLOG.md).

## Crates

```
crates/
  gz-proto/   gz.msgs types generated at build time (prost-build + protox, no protoc)
              from the vendored gz-msgs Harmonic .proto files in proto/.
  gz-core/    No Bevy. Builds for native and wasm.
                frame      `op,topic,type,payload` codec
                transport  sans-IO client state machine (handshake, subs, assets)
                scene      gzweb's rules: gz.msgs.Scene → SceneDesc node tree
                uri        Fuel / file:// / model:// resolution (gzweb createFuelUri)
                snapshot   gz-parity-snapshot/1 types
  gz-bevy/    Bevy plugin: drivers, asset sources, spawning, poses, camera, HUD,
              snapshot exporter.
  gzweb/      The app: native binary (+ headless --snapshot mode) and wasm entry.
  gz-parity/  Native tool: record / replay / compare / compare-protocol.
parity/       run.sh driver + js/ (gzweb reference exporter, Playwright).
tests/corpus/ Recorded websocket sessions. tests/parity/allowlist.toml.
spikes/       Standalone experiments, not in the workspace (spikes/collada).
```

Dependency direction: `gz-proto ← gz-core ← gz-bevy ← gzweb`, and
`gz-core ← gz-parity`. Protocol and scene rules belong in `gz-core`, where they
can be tested without a GPU. `gz-bevy` only instantiates them.

## Data flow

```
socket ─► driver (native thread / web_sys callbacks)
       ─► DriverEvent ─► connection::pump ─► Transport::on_message
                                               │ events
            ┌──────────────────────────────────┼───────────────────────────┐
            ▼                                  ▼                           ▼
     GzInbox.scene / scene_updates      GzInbox.poses              WsAssetStore.deliver
            ▼                                  ▼                           ▼
     spawn::apply_scene                 spawn::apply_poses          WsAssetReader wakes
     (SceneDesc → entities)             (set local Transform)       (Bevy loaders)
```

Outgoing frames go the other way: `Transport::poll_outgoing` → `Driver::send`.
Asset requests start in `WsAssetReader`, which queues the URI in the store.
`pump` then turns it into an `asset,,,<uri>` frame.

## Coordinate frames

There is one `GzWorldRoot` entity, rotated `Rx(-90°)`, so gz `(x, y, z)` maps to
Bevy `(x, z, -y)`. Everything Gazebo is a descendant of it and carries **raw
Gazebo local poses**:

```
GzWorldRoot
├── model (GzEntity kind=model, Transform = model pose)
│   ├── link
│   │   ├── visual (GzVisual, Transform = visual pose, scale = <mesh><scale>)
│   │   │   └── geometry child (Mesh3d / SceneRoot; primitive-specific local transform)
│   │   └── light (GzLight) └── emitter child (Bevy light, looks along direction)
│   └── nested model …
└── top-level light
```

This mirrors gzweb's three.js tree, so:

- Mesh files load untouched. gzweb does not rotate Collada for `up_axis`, and
  neither do we.
- `dynamic_pose/info` updates are plain local-transform writes. They match by
  entity id first, then by name.
- Snapshots compare Bevy local transforms directly with three.js locals.

Primitive conventions follow gzweb `Scene.ts`:

| Primitive | Convention |
|---|---|
| Cylinder, capsule, cone | Axis along +Z (child rotated +90° about X) |
| Plane | `Plane3d` facing +Z, rotated by `from_rotation_arc(Z, normal)` |
| Ellipsoid | Unit UV sphere scaled by the radii |

## Websocket protocol

Frames in both directions have the form `op,topic,type,payload` (gz-launch /
gz-sim `WebsocketServer.cc`):

```
open → auth,,,<key> (if key) → "authorized" → protos,,,
     → protos,,,
<protos text> → topics-types,,,  worlds,,,          status Connected
pub,worlds    → scene,<world>,,
pub,scene                                         status Ready
Ready → sub,/world/<w>/dynamic_pose/info,,   sub,/world/<w>/scene/info,,
asset,,,<uri> → asset,<uri>,gz.msgs.Bytes,<Bytes>   (or gz.msgs.StringMsg error code)
```

The proto definitions arrive as a *binary* frame. Asset payloads are
serialized `gz.msgs.Bytes`, not raw file bytes.

Frames go out as text when they are UTF-8 (every gzweb frame is), and as
binary otherwise; the server accepts both.

**Server limits (gz-launch 7 to `main`, probed against Harmonic).** The
server reads each frame as a C string and splits it with `strtok` on `,`,
which merges empty fields:

- every field is cut at its first `0x00` byte;
- a `,` (`0x2c`) in any field makes the frame invalid (dropped, no reply);
- an empty field the server indexes (topic of `sub`/`image`/`unsub`/
  `throttle`; service, type or payload of `req`) is an out-of-bounds read,
  which can crash it (`std::length_error` in `OnRequest` for an empty `req`);
- `throttle` with rate 0 divides by zero;
- there is no `adv` or `pub_in` handler, so publishing from the client does
  nothing;
- `image` is only honoured for `gz.msgs.Image` topics;
- `ignition.msgs.*` request types are rejected;
- with no key configured, `auth` is never answered.

`Transport` refuses frames that would hit the first four
(`SendError::Unsendable`). `gz_core::control` (play, pause, step, reset,
stop) stamps a header (1 s) on every request so `WorldControl { pause:
false }` isn't empty and has no zero bytes, and splits step counts whose
varint contains `0x00` or `,` (44 is `0x2c`) into several requests.

**Sending.** As in gzweb's `sendMessage`, only handshake ops go out before
`Connected`, and nothing after an error (`SendError`). As in gzweb, messages
on topics the client isn't subscribed to are dropped.

**Reconnect.** On a close or failed connect, `connection::pump` reconnects
after 0.5, 1, 2, 4, 8, then 10 s (reset on `Ready`). It doesn't retry a
rejected key or an invalid URL, and keeps that reason in the status. TCP
connect and the websocket/TLS handshake time out after 10 s
(`net::CONNECT_TIMEOUT`); the gz handshake (until the proto definitions
arrive) times out after 10 s too (`connection::HANDSHAKE_TIMEOUT`), which
covers a key sent to a server without one. The next `scene` reply replaces
the whole scene. It is on by
default, off in `--snapshot` mode (`--reconnect` / `--no-reconnect`). gzweb
doesn't reconnect.

**Messages.** Send a `GzCommand` (`Play`, `Pause`, `Step(n)`, `Reset`,
`Stop`, `Subscribe`, `Unsubscribe`, `Throttle`, `Request`); one the
connection can't send right now is dropped and logged, not queued.
Messages on subscribed topics other than the world's own pose, scene and
stats topics arrive as `GzPublish`; every service reply as
`GzServiceReply`. In
interactive mode the client also subscribes to `/world/<w>/stats`
(throttled to 5 Hz) for the HUD's sim line; Space toggles play/pause and N
steps.

**TLS.** Native `wss://` uses rustls with Mozilla's roots
(`gz_core::net`, feature `native`); `--ca-cert PEM` adds a private CA. The
browser handles `wss://` itself.

## Assets

`gz_core::uri::mesh_load_uri` strips `file://` and `model://`, then applies
`createFuelUri`. `gz_bevy::assets::asset_path` maps the result to a source:

| Load URI | Bevy path | Source |
|---|---|---|
| `https://…` | `gzhttps://…` | HTTP (ureq / fetch) |
| `/abs/path` | `wsasset://abs/abs/path` | websocket `asset` request |
| `rel/path` | `wsasset://rel/rel/path` | websocket `asset` request |

Keeping the directory structure in the Bevy path makes relative references
inside meshes (glTF buffers, OBJ `.mtl`, textures) resolve to the right
server-side file. Both sources are registered by `GzAssetSourcesPlugin`, which
**must be added before `DefaultPlugins`**.

| Format | Loader |
|---|---|
| glTF/GLB | Bevy |
| OBJ | `bevy_obj` |
| STL | `bevy_stl` |
| Collada (`.dae`) | Not yet: pink placeholder, excluded from snapshots. See [`spikes/collada/REPORT.md`](../spikes/collada/REPORT.md). |

## Rendering

gzweb-rs does **not** copy gzweb's look (three.js r141, legacy lights, no tone
mapping, flat ambient). It uses Bevy's physically based pipeline at its best
(`gz-bevy/src/render.rs`). Scene structure and light *parameters* still follow
Gazebo, and the parity snapshot compares those, not pixels.

| | High (default) | Low (`--quality low`, `?quality=low`) |
|---|---|---|
| Camera | HDR, TonyMcMapface tone mapping, fixed exposure, natural bloom | HDR, tone mapping, exposure |
| Anti-aliasing | TAA | 4x MSAA |
| Ambient occlusion | SSAO (high) | none |
| Shadows | 4096 px, 4 cascades to 150 m, temporal filtering | 2048 px, Gaussian |

**Units.** A Gazebo directional light of intensity 1 is 10 000 lux
(`render::units::SUN_LUX`). The exposure renders a white surface facing it at
1.0 before tone mapping, opened half a stop. Point and spot lights of
intensity 1 light a surface 1 m away as brightly as the sun (4π sr × 10 000
lux). Intensities are the message's `intensity`, not gzweb's
attenuation-derived value; `range` is Bevy's cutoff.

**Environment.** Set from each new scene (`render::GzEnvironment`):

- no `<sky>`: the background colour, plus image-based fill light from a
  procedural studio cubemap (horizon = background, brighter above, darker
  below, tinted by the scene's ambient colour) at 35% of the sun's strength;
- `<sky>` without a cubemap: Bevy's physically based atmosphere, lit by the
  scene's directional lights, which also lights the scene;
- `<sky>` with a cubemap (header `cubemap_uri` as gzweb reads it, or the
  field): a skybox that also provides the image-based light (DDS supported);
- `<fog>`: Bevy distance fog (linear, exponential, exponential squared).

`GlobalAmbientLight` keeps the scene's ambient colour (the snapshot reports
it) but has zero brightness; the environment map is the fill light. Bevy
0.18's environment-map generator breaks if a camera switches sources within
a few frames, so nothing is installed until the first scene, and the studio
cubemap is 512 px like the atmosphere's.

**Grid.** Gazebo's 20 × 20 × 1 m grid, drawn with gizmos when the scene's
`grid` flag is set (G toggles). Dark lines, because gizmos aren't exposed.

**Defaults kept from gzweb:** camera at (-6, 0, 6) looking at the origin,
60° vertical FOV, near/far 0.01/1000, clear colour `0xb2b2b2` unless the
scene sets a background.

## Camera controls

`camera::GzOrbitControls` ports gzweb's patched three.js `OrbitControls`
(`gzweb/include/OrbitControls.js`, mouse handlers in `Scene.ts`). The camera
is never re-aimed at its pivot:

- Any button press or wheel step sets the pivot to the point under the cursor.
  It uses the nearest visible mesh (`MeshRayCast`), else the Gazebo z = 0 plane.
  A miss keeps the old pivot; gzweb resets it to the origin (PORT_PLAN §2.5).
- Left drag: `rotate_around(pivot)`, yaw about world up then pitch about the
  camera's right axis, by `2π·Δpx / height`. The offset's polar angle is
  clamped to (0, π).
- Right drag: screen-space pan, scaled by `distance·tan(fov/2)`.
- Wheel / middle drag: dolly along camera→pivot by `0.95` per step.

Not used in headless mode.

## Headless snapshot mode

`gzweb --snapshot out.json` runs `DefaultPlugins` with no window, no winit and
`WgpuSettings { backends: None }`, so it needs no GPU. Meshes stay in the main
world, which is what makes AABBs computable. Once the scene has settled, it
writes the snapshot and exits:

- status is `Ready`;
- the scene is spawned and every asset load has finished;
- glTF/OBJ scenes are instanced;
- the settle time has passed since the last structural message.

Pose streams don't count as activity, because they never stop on a live
simulation.

`--screenshot out.png` instead opens a fixed 1280×720 window at scale 1 (the
parity harness's viewport), with the same settling rules and no HUD, saves a
screenshot and exits. It needs a GPU. Use it to review rendering changes.

## Conventions

- **Quality gate:** `scripts/check.sh`, run by the git hooks and CI (coverage
  runs at pre-push, the rest at pre-commit). It covers:
  - rustfmt;
  - clippy `-D warnings` on native (all targets) and wasm32;
  - rustdoc `-D warnings`;
  - cargo-deny;
  - tests under cargo-llvm-cov, at **≥ 95% line coverage**.
- **Lints:** `[workspace.lints]`; every crate sets `[lints] workspace = true`.
  `unsafe` is forbidden, and `unwrap()` is only allowed in tests.
  Integration-test crates need `#![allow(clippy::unwrap_used)]`.
- **Testing Bevy code:** in a minimal `App` (see `gz_bevy::test_util`), not
  with a GPU. Anything end to end goes in `crates/gzweb/tests/headless.rs`,
  which runs the real binary against `gz_parity::replay::serve_on`.

- Platform code uses `#[cfg(target_arch = "wasm32")]` / `not(...)`, never runtime
  flags. Platform-specific dependencies go under `[target.'cfg(...)'.dependencies]`.
- `GzConnection` is a **non-send** resource: the wasm socket is a JS object.
- Keep the Bevy `webgpu` feature; WebGPU is a hard requirement. On wasm,
  Bevy's `webgpu` feature *overrides* `webgl2`, so the current browser build is
  WebGPU-only and panics with "Unable to find a GPU" where WebGPU is missing.
  A WebGL2 fallback needs a second wasm build that the page picks when
  `navigator.gpu` is absent (PORT_PLAN.md Phase 7).
- Use bevy_ui for all UI. The host page only carries the canvas and a status
  line.
- When gzweb-rs deliberately differs from gzweb, list it in PORT_PLAN.md §2.5
  and in `tests/parity/allowlist.toml`, with a reason.
