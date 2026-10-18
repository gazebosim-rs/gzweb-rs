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
│   │   │   └── geometry child (Mesh3d / WorldAssetRoot; primitive-specific local transform)
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
| `https://…` | `gzhttps://…` | HTTP (ureq / fetch), then the websocket `asset` request with the URL |
| `/abs/path` | `wsasset://abs/abs/path` | websocket `asset` request |
| `rel/path` | `wsasset://rel/rel/path` | websocket `asset` request |

The fallback matters when Fuel is unreachable: the server resolves a Fuel
URL to its local Fuel cache (`/gazebo/resource_paths/resolve`). Local paths
skip HTTP: gzweb fetches them relative to the page, which only finds files
the site happens to serve. Concurrent requests for one URI share one
websocket request (`WsAssetStore`); Bevy's asset server dedups by path.

Keeping the directory structure in the Bevy path makes relative references
inside meshes (glTF buffers, OBJ `.mtl`, textures) resolve to the right
server-side file. Both sources are registered by `GzAssetSourcesPlugin`, which
**must be added before `DefaultPlugins`**.

| Format | Loader |
|---|---|
| glTF/GLB | Bevy |
| OBJ + MTL | `gz_bevy::obj::ObjLoader` on `tobj` (below) |
| STL | `gz_bevy::stl::StlLoader`: ASCII and binary, told apart as three.js does (flat shaded; zero face normals come from the winding) |
| Collada (`.dae`) | `gz_bevy::collada::ColladaLoader` on `gz-collada` (below) |

**Collada.** `gz-collada` vendors mesh-loader's XML, geometry and effect
parsers (`src/vendor/`, Apache-2.0, excluded from the coverage gate) and
replaces its scene assembly (`src/assemble.rs`): the instantiated visual
scene as a node tree with names and local transforms (parent first), one
mesh per primitive, per-instance material binding, `<instance_node>`, and
normal maps from `<extra><technique><bump>`. The loader emits a Bevy
`Scene`: a root scaled by `<unit>` (no `<up_axis>` rotation, like gz-common
and gzweb), an entity per node with its `Name`, and a child per primitive.
Textures load through the `.dae`'s own source, relative to it (or through
`asset_path` when absolute). Effects map onto PBR: diffuse (white under a
texture) × opacity, roughness `sqrt(2/(n+2))` from the Phong exponent,
emission in scene-referred units, `constant` = unlit. On the 14 Fuel meshes
in `spikes/collada/corpus`, triangle counts and world bounds match gzweb's
three.js loader (`cargo test -p gz-collada -- --ignored`).

**OBJ.** `tobj` parses the OBJ and its `mtllib`s (read through the OBJ's
own source first, since tobj asks for them synchronously). Each `o`/`g` is a
named entity with a child per material, as three.js builds it. Diffuse and
ambient maps follow gzweb's `GzObjLoader` path rules (`model://` beside this
model, paths with `/materials/textures` kept, bare names in
`<model>/materials/textures/`); other maps are relative to the OBJ. A
missing `.mtl` leaves the default material (gzweb never shows the mesh).
MTL → PBR: `Kd` × `map_Kd`, alpha `d` or `1 - Tr`, roughness from `Ns` or
`Pr`, `Pm`, `Ke`/`map_Ke`, `map_Bump`/`bump`/`norm` as normal maps.

**Submeshes.** A mesh file that loads as a scene (COLLADA, OBJ, glTF) with a
`<submesh>` gets `gz_bevy::submesh::GzSubmesh` on its `WorldAssetRoot`; an
observer on `WorldInstanceReady` keeps the first node (depth first) with
that name and geometry of its own, drops everything else (its child nodes
too), and with `center` shifts the root so the bounds' centre is the
visual's origin. These are Gazebo's rules; gzweb zeroes the ancestors'
translations but keeps their rotations, which only differs when an ancestor
rotates. An unknown name leaves nothing, as in both. Snapshots wait for
`GzSubmesh::applied`.

**Heightmaps.** Gazebo never puts heights in the scene message (gzweb's
terrain is therefore empty); it sends the image `filename`. `gz_bevy::heightmap`
loads it through the asset chain (linear, not sRGB), samples it `sampling`
times per pixel (bilinear, at most 1025 vertices a side), and lays it over
`size.x` × `size.y` centred on `origin`, black at `origin.z`, white at
`origin.z + size.z`, the image's top row at +Y. Smooth normals and tangents;
the first texture layer repeats every `size` metres (repeat sampler, 16×
anisotropy). A terrain whose image fails is an empty mesh that still carries
its material. Not done: blending layers by height (`<blend>`), DEM (GeoTIFF)
heightmaps, mipmaps for the tiled textures.

## Rendering

gzweb-rs does **not** copy gzweb's look (three.js r141, legacy lights, no tone
mapping, flat ambient). It uses Bevy's physically based pipeline at its best
(`gz-bevy/src/render.rs`). Scene structure and light *parameters* still follow
Gazebo, and the parity snapshot compares those, not pixels.

| | Low | High (default) | Ultra |
|---|---|---|---|
| Camera | HDR, TonyMcMapface, fixed exposure | + natural bloom | same as High |
| Anti-aliasing | 4x MSAA | TAA | TAA |
| Ambient occlusion | none | SSAO (high) | SSAO (high) |
| Shadows (sun / point) | 1024 / 512 px | 2048 / 1024 px, 4 cascades to 150 m, temporal filtering | 4096 / 2048 px |
| Fill light | flat ambient | image-based | image-based |

Pick it with `--quality` or `?quality=`. A 4096 px sun shadow costs 256 MB
of GPU memory (4 cascades), 4× the 2048 px one, for sharper distant
shadows; the first cascade (5 m) is already ~2.5 mm per texel at 2048.

**Units.** A Gazebo directional light of intensity 1 is 10 000 lux
(`render::units::SUN_LUX`). The exposure renders a white surface facing it at
1.0 before tone mapping, opened half a stop. Point and spot lights of
intensity 1 light a surface 1 m away as brightly as the sun (4π sr × 10 000
lux), with Bevy's inverse-square falloff and `range` as its cutoff, so they
are dimmer than Gazebo's near-flat attenuation further out. Intensities are
the message's `intensity` (unset means 1), not gzweb's attenuation-derived
value; spot cones use `spot_inner_angle`/`spot_outer_angle`. The clear and
fog colours go through the tone mapper too, so a 0xb2b2b2 background shows a
little darker.

**Environment.** Set from each new scene (`render::GzEnvironment`,
`render::plan`); the camera is respawned for it (keeping its view), because
Bevy keeps private per-camera environment-map and atmosphere state that only
goes away with the entity, and a camera switching environment-map sources
fails wgpu validation. An identical scene (a reconnect) changes nothing.

- No `<sky>`: the background colour, plus image-based fill light from a 64 px
  procedural studio cubemap in the scene's `<ambient>` colour (brighter
  above, darker below): a white ambient adds half of full sun.
- `<sky>` without a cubemap, and a directional light: Bevy's physically based
  atmosphere, which also lights the scene. Without a sun it would be black,
  so the studio is used. The atmosphere is a planet entity
  (`render::GzAtmosphere`, replaced with each scene) with no `Transform`, so
  Bevy puts its centre `inner_radius` below the origin; the camera carries
  `AtmosphereSettings`. All planets share one `ScatteringMedium`.
- `<sky>` with a cubemap (header `cubemap_uri` as gzweb reads it, or the
  field): loaded, then checked (six square power-of-two faces, ≤ 8192 px)
  and used as the skybox and the scene's light at its own brightness. A
  cubemap that fails to load or doesn't qualify falls back to the studio.
  Its orientation is not adjusted (unverified against a Gazebo cubemap).
- `<fog>`: Bevy distance fog (linear with `end > start`, exponential,
  exponential squared); none otherwise.
- Without a GPU (`--snapshot`) or at `Quality::Low`, no environment maps:
  a flat ambient light at the fill strength instead.

`GlobalAmbientLight` takes the scene's ambient colour (the snapshot reports
it; gzweb's 0x191919 until a scene sets one) and adds 5% of full sun under
image-based light, so shadows never go black.

**Grid.** Gazebo's 20 × 20 × 1 m grid, drawn with gizmos (their own
`GridGizmos` group, depth-biased off the ground) when the scene's `grid` flag
is set; G toggles it. Dark lines, because gizmos aren't exposed.

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
PNG (creating its directory) and exits: 1 if it couldn't save, 2 if no frame
arrived before the timeout. It needs a GPU. Use it to review rendering
changes.

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
