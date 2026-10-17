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

Text frames from the client, binary frames from the server, both in the form
`op,topic,type,payload` (gz-launch / gz-sim `WebsocketServer.cc`):

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

## Rendering defaults (from gzweb)

| Setting | Value |
|---|---|
| Camera position | (-6, 0, 6), looking at the origin |
| Field of view | 60° vertical |
| Near / far planes | 0.01 / 1000 |
| Clear colour | `0xb2b2b2`, unless the scene sets a background |
| Ambient light | `0x191919` until the scene sets it |

Light intensities use gzweb's three.js semantics in `LightDesc`. The Bevy unit
mapping in `spawn::spawn_light` is provisional and gets calibrated in Phase 2.

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

## Conventions

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
