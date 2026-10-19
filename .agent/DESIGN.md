# gzweb-rs: Architecture & Design

Plan and parity rationale: [`PORT_PLAN.md`](./PORT_PLAN.md). Test harness:
[`PARITY.md`](./PARITY.md). History: [`WORKLOG.md`](./WORKLOG.md).

## Crates

```
crates/
  gz-proto/   gz.msgs types generated at build time (prost-build + protox, no protoc)
              from the vendored gz-msgs 12 (Jetty) .proto files in proto/.
  gz-core/    No Bevy. Builds for native and wasm.
                frame      `op,topic,type,payload` codec
                transport  sans-IO client state machine (handshake, subs, assets)
                scene      gzweb's rules: gz.msgs.Scene → SceneDesc node tree
                uri        Fuel / file:// / model:// resolution (gzweb createFuelUri)
                snapshot   gz-parity-snapshot/1 types
  gz-collada/ No Bevy. COLLADA documents as a node tree (vendored parsers,
              see "Assets").
  gz-bevy/    Bevy plugin: drivers, asset sources, spawning, poses, camera, HUD,
              snapshot exporter, COLLADA loader; `view`: camera controller,
              key map and minimal-mode controls.
  gzweb/      The app: native binary (+ headless --snapshot mode) and wasm entry.
  gz-parity/  Native tool: record / replay / compare / compare-protocol.
parity/       run.sh driver + js/ (gzweb reference exporter, Playwright).
tests/corpus/ Recorded websocket sessions. tests/parity/allowlist.toml.
spikes/       Standalone experiments, not in the workspace (spikes/collada).
```

Dependency direction: `gz-proto ← gz-core ← gz-bevy ← gzweb`,
`gz-collada ← gz-bevy`, and `gz-core ← gz-parity`. Protocol and scene rules belong in `gz-core`, where they
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

Frames in both directions have the form `op,topic,type,payload`. The
server is Gazebo Jetty's websocket system, `gz-sim-websocket-server-system`
(gz-sim 10, `src/systems/websocket_server/WebsocketServer.cc`), a port of
gz-launch's `WebsocketServer` plugin, which Jetty deprecates:

```
open → auth,,,<key> (if key) → "authorized" → protos,,,
     → protos,,,
<protos text> → topics-types,,,  worlds,,,          status Connected
pub,worlds    → scene,<world>,,
pub,scene                                         status Ready
Ready → sub,/world/<w>/dynamic_pose/info,,   sub,/world/<w>/scene/info,,
asset,,,<uri> → asset,<uri>,gz.msgs.Bytes,<Bytes>   (or gz.msgs.StringMsg error code)
```

The server sends every frame as *binary*, the proto definitions and the
`authorized`/`invalid` replies included. Asset payloads are serialized
`gz.msgs.Bytes`, not raw file bytes.

Frames go out as text when they are UTF-8 (every gzweb frame is), and as
binary otherwise; the server accepts both.

**Jetty vs gz-launch.** Diffed against gz-launch 7 (Harmonic) at
gz-sim10_10.5.0 and probed against the `gz-websocket-jetty` image. The ops,
the framing, the `asset` resolution and allowed paths, `req`, `image`,
`throttle`, the error codes (`service_not_found`, `asset_not_found`,
`asset_uri_missing`, `asset_access_denied`) and the defaults (port 9002,
60 Hz, no connection or queue limit) are unchanged. What changed:

- `auth` is always answered. With no key configured every connection is
  authorized, so any key gets `authorized`; gz-launch never answered a
  server without a key, and the client waited for its handshake timeout.
- The `protos` text now includes each file's top-level enums
  (`PixelFormatType`, `SphericalCoordinatesType`). gz-launch left them out,
  so gzweb's protobufjs parse failed on any message using them (the `sky`,
  `visibility` and `fuel_textured_mesh` worlds; see PARITY.md).
- `scene,<world>` and the particle-emitter request pass the world name
  through `TopicUtils::AsValidTopic` (see "World topics" below).
- It is a system loaded with the world: the port opens only once the world
  (and every Fuel model it includes) has loaded, and a crash takes the
  simulation down with it.
- Its options come from the `<plugin>` element (SDF), not a launch file.

**Server limits (gz-launch 7 to `main` and gz-sim 10, probed against
Harmonic and Jetty).** The server reads each frame as a C string and splits
it with `strtok` on `,` (`gz::common::split`), which merges empty fields:

- every field is cut at its first `0x00` byte;
- a `,` (`0x2c`) in any field makes the frame invalid (dropped, no reply);
- an empty field the server indexes (topic of `sub`/`image`/`unsub`/
  `throttle`; service, type or payload of `req`) is an out-of-bounds read,
  which can crash it (`std::length_error` in `OnRequest` for an empty `req`;
  on Jetty `sub,,,` kills gz-sim itself);
- `throttle` with rate 0 divides by zero (a `1.0 / 0` period cast to
  nanoseconds, undefined behaviour);
- there is no `adv` or `pub_in` handler, so publishing from the client does
  nothing;
- `image` is only honoured for `gz.msgs.Image` topics;
- `ignition.msgs.*` request types are rejected (the reply is an empty
  `gz.msgs.Boolean`);
- with no key configured, `auth` is never answered (gz-launch only; see
  above).

`Transport` refuses frames that would hit the first four
(`SendError::Unsendable`). `gz_core::control` (play, pause, step, reset,
stop) stamps a header (1 s) on every request so `WorldControl { pause:
false }` isn't empty and has no zero bytes, and splits step counts whose
varint contains `0x00` or `,` (44 is `0x2c`) into several requests.

**World topics.** Decided 2026-10-19: the client sanitizes the world name
as gz-sim does. The `worlds` reply carries the raw SDF name, but gz-sim
builds every world topic and service from it with gz-transport's
`TopicUtils::AsValidTopic` (spaces become `_`; `@`, `~`, `:=` and `//` are
dropped; an invalid result is empty), and Jetty's server does the same to
the name in `scene,<world>`. `gz_core::topic` ports `AsValidTopic` and
`IsValidTopic` from gz-transport 15 exactly (tested on upstream's cases), and
`world_topic(world, leaf)` builds `/world/<world>/<leaf>` with it, as the
server builds its `scene` service name. The pose, scene-info and stats
subscriptions, the world control service, `WorldTopic` routing,
`gz-parity record` and the replay server all use it. `scene,<world>` keeps
the raw name, like gzweb (the server sanitizes it). gz-sim's own systems
sanitize `/world/<world>` and append the leaf, SimulationRunner (control,
stats) after sanitizing the bare name first; these agree with
`world_topic` unless the name contains a `/`. gzweb uses the raw name, so on
Jetty a world named `my world` gets its scene but no poses, scene updates or
control (the server subscribes to `sub` topics as sent, and gz-transport
refuses one with a space); PORT_PLAN §2.5 lists the divergence.

**Sending.** As in gzweb's `sendMessage`, only handshake ops go out before
`Connected`, and nothing after an error (`SendError`). As in gzweb, messages
on topics the client isn't subscribed to are dropped.

**Reconnect.** On a close or failed connect, `connection::pump` reconnects
after 0.5, 1, 2, 4, 8, then 10 s (reset on `Ready`). It doesn't retry a
rejected key or an invalid URL, and keeps that reason in the status. TCP
connect and the websocket/TLS handshake time out after 10 s
(`net::CONNECT_TIMEOUT`); the gz handshake (until the proto definitions
arrive) times out after 10 s too (`connection::HANDSHAKE_TIMEOUT`), which
covers a key sent to a gz-launch server without one (Jetty answers it). The next `scene` reply replaces
the whole scene. It is on by
default, off in `--snapshot` mode (`--reconnect` / `--no-reconnect`). gzweb
doesn't reconnect.

**Messages.** Send a `GzCommand` (`Play`, `Pause`, `Step(n)`, `Reset`,
`Stop`, `Subscribe`, `Unsubscribe`, `Throttle`, `Request`); one the
connection can't send right now is dropped and logged, not queued.
Messages on subscribed topics other than the world's own pose, scene and
stats topics arrive as `GzPublish`; every service reply as
`GzServiceReply`. In
interactive mode (`UiMode` other than `None`) the client also subscribes to
`/world/<w>/stats` (throttled to 5 Hz) for the HUD's sim line and the
play/pause key.

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

A host that fails to connect or times out (not one answering with an error
status) is skipped for 5 minutes (`HOST_DOWN_FOR`): its other assets go
straight to the websocket instead of each waiting up to 40 s. An `asset`
request the server doesn't answer fails after 60 s (`WS_ASSET_TIMEOUT`,
checked by `connection::pump`).

Keeping the directory structure in the Bevy path makes relative references
inside meshes (glTF buffers, OBJ `.mtl`, textures) resolve to the right
server-side file. Both sources are registered by `GzAssetSourcesPlugin`, which
**must be added before `DefaultPlugins`**.

| Format | Loader |
|---|---|
| glTF/GLB | Bevy, loaded as a `Gltf` and instanced by `spawn::instance_gltf` |
| OBJ + MTL | `gz_bevy::mesh_file::SceneFileLoader` → `gz_bevy::obj` on `tobj` (below) |
| STL | `gz_bevy::stl::StlLoader`: ASCII and binary, told apart as three.js does (flat shaded; zero face normals come from the winding) |
| Collada (`.dae`) | `gz_bevy::mesh_file::SceneFileLoader` → `gz_bevy::collada` on `gz-collada` (below) |

Extensions match in any case, as in Gazebo (`.DAE`, `.Obj`, `.GLB`). Bevy
picks a loader by extension case-sensitively when more than one loader
makes the asset type, or when a label is asked for (`x.glb#Scene0`), so
COLLADA and OBJ share one `WorldAsset` loader that picks the parser from the
lower-cased extension, and glTF loads by type. `mesh_file` also holds what
the two loaders share: texture paths and loading (a path climbing out of
its source is refused, since Bevy panics on it inside a loader), the Phong
roughness and the mesh attributes (`MeshParts`).

**Collada.** `gz-collada` vendors mesh-loader's XML, geometry and effect
parsers (`src/vendor/`, Apache-2.0, patched where marked `gz-collada:`,
excluded from the coverage gate) and replaces its scene assembly
(`src/assemble.rs`): the instantiated visual scene as a node tree with
names and local transforms (parent first), one mesh per primitive,
per-instance material binding, `<instance_node>` (cycles cut, at most 1M
nodes and primitives), and normal maps and `<double_sided>` from any
`<extra>` (best effort). A malformed document is an error, never a panic:
XML nesting is capped at 300 levels and `<node>`s at 256 before the
recursive parsers run, and `src/validate.rs` checks each instantiated
primitive's sources, accessors and index counts. As in three.js, dangling
geometry/material/effect/node references fail the load, unused geometry is
not checked, and VERTEX always reads the mesh's own `<vertices>`.

The loader emits a Bevy `WorldAsset`: an unnamed root scaled by `<unit>` (no
`<up_axis>` rotation, like gz-common and gzweb), an entity per node with its
`name` as its `Name` (never the `id`: gz-common names submeshes after
`name`), and a child per primitive. Textures load through the `.dae`'s own source,
relative to it (or through `asset_path` when absolute), repeating unless
the file's `wrapU`/`wrapV` say otherwise. `collada::drop_failed_textures`
removes a texture that failed to load from its material (any
`StandardMaterial`), since Bevy would otherwise never draw it. Effects map
onto PBR:
- base colour: diffuse (white under a texture), alpha = opacity (three.js
  ignores diffuse alpha); blended below 1, a 0.5 alpha mask for a
  `<transparent>` texture;
- Phong/Blinn roughness `(2/(n+2))^¼` (the GGX α `sqrt(2/(n+2))`, which
  Bevy gets by squaring perceptual roughness), reflectance scaled by the
  specular luminance; Lambert roughness 1;
- emission (white under a texture) through `units::emissive`, so 1 renders
  at full brightness (SDF materials use it too);
- every colour, vertex colours included, linear (see "Materials");
- `constant` = unlit.

On the 14 Fuel meshes in `spikes/collada/corpus`, triangle counts and world
bounds match gzweb's three.js loader (`spikes/collada/run.sh`, then
`cargo test -p gz-collada -- --ignored`; manual, not in CI). Not done:
skinned meshes (`<instance_controller>`, skipped with a warning). A
visual's SDF material replaces the file's (see "Materials").

**OBJ.** `tobj` parses the OBJ and its `mtllib`s (read through the OBJ's
own source first, since tobj's material callback is synchronous). Each
`o`/`g` is a named entity with a child mesh per material (three.js builds
one mesh per object with a group per material, gz-common a submesh per
material); a later object with the same name is another entity. Diffuse
maps follow gzweb's `GzObjLoader` path rules (`model://` beside this model,
paths with `/materials/textures` kept, bare names in
`<model>/materials/textures/`); URIs and absolute paths, and other maps,
resolve like COLLADA textures (through the sources, else relative to the
OBJ). MTL keys match in any case (three.js lower-cases them), and
`map_Kd`'s `-s`/`-o` become the material's `uv_transform` (three.js's
repeat/offset). `map_Ka` is ignored, as in three.js and gz-common. A
missing `.mtl` leaves the default material (gzweb never shows the mesh).
MTL → PBR as for COLLADA: `Kd` × `map_Kd`, alpha `d` or `1 - Tr`, roughness
`(2/(n+2))^¼` from `Ns` or `Pr` as given, `Pm`, `Ke`/`map_Ke` through
`units::emissive`, `map_Bump`/`bump`/`norm` as normal maps; textures repeat
(three.js's MTLLoader default); colours, vertex colours included, are
linear.

**Submeshes.** A mesh file that loads as a scene (COLLADA, OBJ, glTF) with a
`<submesh>` gets `gz_bevy::submesh::GzSubmesh` on its `WorldAssetRoot`; an
observer on `WorldInstanceReady` applies Gazebo's rules (gz-common's
loaders, gz-rendering's `Ogre2MeshFactory`):
- every primitive (a `Mesh3d` entity) is a submesh named after its nearest
  ancestor node with a name (an unnamed node's geometry joins its named
  ancestor's; a primitive's own name, like glTF's `Mesh.Material`, and
  Bevy's `GltfNode<n>` for an unnamed glTF node, never count);
- **every** submesh with the requested name is kept, under its ancestors'
  transforms; everything else goes;
- with `center`, each kept submesh moves so its own bounds (under those
  transforms) centre on the visual's origin.

Running again on a respawned scene is safe (it starts from the fresh
entities; a centred submesh is marked). An unknown name leaves nothing.
Snapshots wait for `GzSubmesh::applied`. gzweb differs (PORT_PLAN §2.5).

**Heightmaps.** Gazebo never puts heights in the scene message (gzweb's
terrain is therefore empty); it sends the image `filename`. `gz_bevy::heightmap`
loads it through the asset chain (linear, not sRGB, kept on the CPU only),
samples it `sampling` times per pixel (bilinear, at most 1025 vertices a
side), and lays it over `size.x` × `size.y` centred on `origin`, black at
`origin.z`, the brightest pixel at `origin.z + size.z` (gz-rendering
divides by `ImageHeightmap::MaxElevation`, the brightest pixel's red; the
minimum is 0), the image's top row at +Y. Smooth normals; tangents, from
the grid itself (mikktspace takes seconds at 1025²), only under a normal
map (any drawn layer's). Snapshots wait for the image and the drawn
layers' textures, and record the terrain's corners
(`GeometryInfo::corners`). A terrain whose image fails is an empty mesh
that still carries its material. Not done: DEM (GeoTIFF) heightmaps,
mipmaps for the tiled textures.

*Texture layers* follow gz-rendering8's ogre2 terrain (`Ogre2Heightmap.cc`,
`media/Hlms/Terra/gz/100.gz_CustomWeights_piece_ps.any`):

- Layer 0 covers the terrain. Layer `i > 0` is laid over the layers below
  it with weight `smoothstep(min, min + fade_dist, h)` from `blends[i-1]`,
  `h` being the height above the terrain's lowest point (Gazebo's
  `localHeight`: `origin.z`, as an image's minimum elevation is 0). Colours
  are `mix`ed in order, so the top layer wins. ogre1 fades linearly; ogre2,
  Harmonic's default, is the reference.
- Each layer repeats every `size` metres (repeat sampler, 16× anisotropy).
- A layer without a blend never shows, and blends past the last layer do
  nothing (ogre2's weight stays 0). A `fade_dist` of 0 is a hard step
  (ogre2 hides that layer; ogre1 steps). A layer without a diffuse map
  leaves the colour below it.
- Normals are blended with the same weights (world-space normals of each
  layer, `mix`ed and renormalised); a layer without a normal map is flat.
  Ogre adds each layer's tangent-space normal on top of layer 0's instead,
  so layer 0's bumps never fade.
- At most `heightmap::MAX_LAYERS` = 4 layers (ogre2's limit; it takes a
  5th only when layer 0 has no normal map and spans the terrain once).

`heightmap::TerrainMaterial` is `ExtendedMaterial<StandardMaterial,
TerrainExtension>`, used for every terrain, so there is one path and the
snapshot reports layer 0 from `base`. Layer 0 is the StandardMaterial's
base colour and normal map (`uv_transform` tiling); layers 1–3 are six
individually bound textures (so sizes may differ) sharing one sampler,
plus a uniform of tilings, min heights, fades and the base height. The
extension adds 6 sampled textures and 1 sampler per fragment stage
(WebGPU's default limit is 16 of each, shared with the view's and the
StandardMaterial's six). Shaders are WESL, embedded
(`gz-bevy/src/shaders/`): `terrain.wesl` (forward and deferred: Bevy's
`pbr_input_from_standard_material`, then the layers, then
`apply_pbr_lighting` and `main_pass_post_lighting_processing`, or
`deferred_output`), `terrain_prepass.wesl` (writes the blended normal, which
SSAO and the forward pass read under `LOAD_PREPASS_NORMALS`), and the shared
`terrain_layers.wesl`. The height comes from the fragment's position in the
mesh's frame (`get_local_from_world`), so the terrain may move. The weight
maths is mirrored in Rust (`heightmap::layer_weight`/`layer_weights`) for
tests; keep the two in sync.

## Materials

`gz_bevy::material` turns a visual's `gz.msgs.Material` into a Bevy
`StandardMaterial` by **Gazebo's** rules (gz-sim `SceneManager::CreateVisual`
and `LoadMaterial`, gz-rendering `Ogre2Material`, ogre-next's PBS shaders),
not gzweb's. The parity snapshot still compares the scene structure; where
the material differs from gzweb's, PORT_PLAN §2.5 says so.

**Colour space: linear.** ogre2 hands the message's floats straight to its
PBS shaders, which light in linear space and write to sRGB targets
(`PFG_RGBA8_UNORM_SRGB`, `gamma = Yes`), so an SDF `0.5` grey is linear 0.5
(sRGB ≈ 0.74 on screen). gzweb feeds the same floats to three.js r141 with
no colour management, which shows them as if they were sRGB (darker). We
follow Gazebo: `Color::linear_rgb` for SDF materials, light colours, COLLADA
effects, MTL colours, vertex colours and `units::emissive`. Colour textures
(albedo, emissive, light map) are sRGB; normal, metalness, roughness and
ambient-occlusion maps linear. Snapshots report the linear floats, the
message's (and gzweb's) values. The scene's background, ambient and fog
colours are unchanged (still read as sRGB).

| Gazebo | Bevy |
|---|---|
| diffuse RGB (ogre2 never uses `ambient`) | `base_color`, × albedo map |
| opacity `diffuse.a × (1 − transparency)` | below 1: `Premultiplied`, colour × α², reflectance × √α (ogre's `Transparent` mode: diffuse fades, reflections stay); under an albedo map plain `Blend` |
| albedo map's alpha, alpha test ≥ 0.5 | `Mask(0.5)`; its shadow is cut too |
| specular (`kS`, scales every reflection) | dielectric F0 = ogre's 3% × specular (`reflectance` 0.433, `specular_tint` √kS); no `<specular>` in the message: Bevy's 4% |
| emissive | `units::emissive`, × emissive map (white when only the map is set, as ogre does) |
| metal workflow: metalness, roughness | `metallic`, `perceptual_roughness` (both perceptual, as in ogre) |
| metalness + roughness maps (red channel) | one `metallic_roughness_texture` (G roughness, B metalness), combined on the CPU |
| normal map (PBR, else the legacy `normal_map`) | `normal_map_texture`; tangents generated (mikktspace) where the mesh has none |
| ambient-occlusion map | `occlusion_texture` (Gazebo ignores it) |
| light map (+ UV set) | a `Lightmap` on the mesh, UV1 (copied from UV0 if missing), `lightmap_exposure` so 1 adds the albedo at full |
| `double_sided` | `double_sided`, `cull_mode: None` |
| `render_order` | `depth_bias = render_order × 100` (Gazebo's scale for reverse depth) |
| visual `cast_shadows` false | `NotShadowCaster` |
| no material: primitive | gz-sim's `gz-grey`: diffuse 0.7, specular 1, roughness 0.2, metalness 1 |
| no material: STL | ogre2's default: white, specular 0.2, roughness 0.5 |

Textures load through the usual chain (`uri::mesh_load_uri`, then
`assets::asset_path`: `model://`, `file://`, absolute paths and Fuel URLs
alike), repeating (ogre's `TAM_WRAP`) with 16× anisotropy. Snapshots wait
for them and for the combined maps.

**Metalness and roughness.** Bevy wants glTF's combined image; Gazebo has
two greyscale maps. `material::combine_metal_rough` waits until both have
loaded (or failed), then writes an `Rgba8Unorm` image at the larger map's
size (the other sampled nearest): G = roughness red, B = metalness red, 1
where a map is missing so its scalar applies alone, as Bevy multiplies map
and scalar like ogre. One image per pair of maps, shared by every material
that uses it. A block-compressed map can't be read on the CPU; it falls
back to the scalars with a warning. An `ExtendedMaterial` sampling two maps
would avoid the copy but needs its own shader, prepass and shadow variants;
the copy is once per pair.

**Mesh visuals.** gz-sim calls `geom->SetMaterial(material)` on a mesh visual
with a `<material>`, and gz-rendering's `BaseMesh::SetMaterial` puts that one
material on **every** submesh, replacing the file's materials and textures
(SubT's cave tiles rely on it for their PBR maps). Without a `<material>` the
file's materials stay, with opacity × `(1 − transparency)` and the visual's
`cast_shadows`. A mesh file's `WorldAssetRoot` carries
`material::GzMeshMaterial`; an observer on `WorldInstanceReady` (beside the
submesh one) applies it: one new material per visual for an SDF material,
one faded copy per shared file material for a transparency. The file's own
material assets are never changed. Meshes do get tangents (normal map) or a
UV1 (light map) in place when they lack them: extra attributes change
nothing for other users.

**Not done, on purpose:**
- the specular workflow's glossiness and specular maps (Gazebo renders it as
  metal with default scalars, keeping the other maps; so do we);
- `environment_map`: Bevy's environment maps are per camera or light probe,
  not per material;
- `lighting: false` (gz-sim never applies it either);
- glTF and COLLADA materials keep their files' own rules (Phase 3).

Gazebo itself only honours `double_sided` together with an albedo map (it is
passed through `SetAlphaFromTexture`); we honour it always, as SDF
specifies.

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

Pick it with `--quality` or `?quality=`. Orthographic views trade TAA and
SSAO for 4× MSAA and re-aim the shadow cascades and fog (see "Views, input
and UI modes", DECISIONS.md D-19). A 4096 px sun shadow costs 256 MB
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

## Views, input and UI modes

**UI modes** (`gz_bevy::UiMode`, `.agent/UI_PLAN.md` §2, DECISIONS.md D-10)
replace the old `GzConfig.interactive` flag; `GzConfig::interactive()` keeps
its meaning (`ui != None`).

| Mode | Runs | Selected by |
|---|---|---|
| `None` | scene, rendering, connection; no input, HUD or stats subscription | `--snapshot`, `--screenshot` |
| `Minimal` | + `view::ViewerControls` and the status text (`--no-hud` / `?hud=0` hide it) | `--ui minimal`; the browser's default (`?ui=`) |
| `Full` | for now the same as `Minimal` (`TODO(M1)`: `gz-ui`'s `GzUiPlugin`) | `--ui full`, the native default |

**`view::ViewCamera`** sits on the main camera next to its `Transform` and
`Projection`: pivot, plain left-drag action (`DragAction`, Orbit by
default), the snapped axis view, the perspective it returns to, speeds and a
running animation. Only `view::apply_view_commands` changes it, from
**`ViewCommand { view, op }`** messages, so the minimal mode's raw input and
the full UI's rail and cube (M3) share one controller. It works in Bevy space;
the camera is not under `GzWorldRoot`. When `render` respawns the camera for a
new environment it carries the `ViewCamera` and `Projection` over.

`ViewOp`s, following gzweb's patched three.js `OrbitControls` where the spec
agrees (the camera is never re-aimed at its pivot). Distances below are
Bevy metres; "depth" is distance along the view axis.

- `PivotAt { at, orbit }` at every drag start: the nearest visible mesh
  under `at` (`MeshRayCast`), else the Gazebo z = 0 plane within the far
  plane. On a miss, an orbit drag falls back to the selection's centre,
  else the origin (spec §6.2), but only if that is in front of the camera;
  everything else (pan and zoom drags, a fallback behind the camera) puts
  the pivot on the pointer's ray at the old pivot's depth (its distance if
  it was at or behind the near plane). Rays come from the `Projection`
  itself, so a projection change and a pick in one frame agree.
- `Orbit(Δpx)`: yaw about world up, then pitch about the camera's right
  axis, `2π·Δ/height`. The pitch stops 1e-4 rad short of straight down or
  up, so the camera stays upright (`up.y ≥ 0`) and can always look straight
  down, wherever the pivot is on screen (clamping the pivot offset's polar
  angle, as three.js does, only works for a pivot at the centre). Orbiting
  a snapped view first returns it to perspective.
- `Pan(Δpx)`: the plane through the pivot, facing the camera, follows the
  pointer exactly: perspective moves `2·depth·tan(fov/2)/height` per px,
  with the pivot's depth (gzweb uses its distance, which drifts for an
  off-centre pivot); orthographic, view height / viewport height.
- `Zoom(steps)`, `ZoomAt { at, steps }`: dolly along camera→pivot by
  `0.95^steps`; orthographic views scale the view height and keep the pivot
  fixed on screen. Between 2 cm from the pivot (`MIN_DISTANCE`) and four
  whole-world framings (`ZOOM_OUT_LIMIT`; no world: 400 m); at a limit
  nothing moves, so the pivot doesn't drift. `ZoomAt` re-pivots like a pan
  drag: on sky it zooms along the ray at the old depth, never towards the
  origin.
- `ToggleProjection` / `SetProjection`: orthographic first re-centres the
  pivot on what is at the view centre, at that point's depth (else the old
  pivot's, else the reach), kept between 2 cm and a whole-world framing, so
  the last click doesn't decide the scale. The height is `2·depth·tan(fov/2)`
  so the plane through the pivot keeps its scale; perspective returns to
  the matching depth and clears `snapped`.
- **Orthographic clip range** (`fit_clip`, after every op and animation
  frame; DECISIONS.md D-20): the camera stands at least the world's extent
  (the world's bounds with ground planes and the 20 m grid, measured from
  the pivot; 50 m without a world) plus the view's reach behind the pivot,
  the near plane is at the reach (> 0, which keeps the atmosphere's sky
  rays sensible) and the far plane just past the world. Perspective far
  starts at 1000 and grows to the world's far side.
- `Snap(face)`: about the selection centre, else the point at the view
  centre, else the pivot; distance kept; animates 250 ms (smoothstep on
  pivot, rotation and the pivot's camera-frame offset), then switches to
  orthographic. Axes follow gz-sim's `ViewAngle`: top looks down with gz +X
  up the screen, front looks along −X, right along −Y (DECISIONS.md D-13).
- `Focus { at }`: frames the selection, else the top-level model under `at`
  without its plane visuals (over a ground model that leaves nothing, so it
  falls through), else the world's meshes without plane visuals (or with
  them if there is nothing else), else goes home. Hidden subtrees don't
  count. Radius × 1.1 fitted to the narrower FOV; orthographic views fit
  the height. Animated, rotation kept; the far plane grows with it.
- `Home`: animates to gzweb's (−6, 0, 6) looking at the origin, in
  perspective.
- `Fly { dir, secs }` at `fly_speed` (0.5) × reach per second, where reach
  is the depth of what the view centre sees (as snaps pick it; else the
  pivot's depth), kept between 0.5 m and a whole-world framing, so a far
  click doesn't make the keys race. Perspective: W/S along the view
  direction flattened onto the ground (looking straight down: up the
  screen), A/D sideways, Q/E world down/up. Orthographic (no depth to fly
  along): W/S up/down the screen, A/D left/right, Q/E zoom in/out. Camera
  and pivot move together.
- Ops that move the view at once (all but Snap/Focus/Home/SetDragAction)
  first land a running animation at its end, so a click mid-snap finishes
  the snap (orthographic, snapped) instead of freezing it half-turned.
  Snap, focus and home start their own animation from wherever the view
  is.
- `apply_view_commands` does nothing on a frame without commands or
  animations, so idle frames leave the camera's `Transform`, `Projection`
  and `ViewCamera` unchanged (change detection stays quiet).

**Orthographic rendering** (`render::projection_effects`, DECISIONS.md
D-19): Bevy 0.20's depth-dependent effects assume perspective, so an
orthographic main camera swaps TAA (with its `TemporalJitter` and
`MipBias`) and SSAO for 4× MSAA and Gaussian shadow filtering, the sun's
cascades span the world's depth slab instead of 0.1–150 m from the camera,
and distance fog is shifted to read as from the matching perspective eye
(linear: exact; exponential: matched at the pivot). Back in perspective
(`5`, an orbit away from a snap, or H) the effects return with TAA's
history reset. The cascades are per light, so the first view decides
them. The old note of a top snap at High rendering only sky colour: the
orthographic near plane at 0 made the atmosphere's centre sky ray a zero
vector, a likely cause; 2026-10-19, after the change, top snaps against
the all-mesh world came out right in 3 of 3 runs at High and at Ultra.

`GzCameraTarget` publishes the first view's pivot in Gazebo coordinates
(the snapshot reports it). `view::Selection` is empty until the full UI
selects things (M5).

**Key map.** `view::input::InputMap` parses the crate's copy of
`design/ui/shortcuts.json` (`include_str!` + serde, so `cargo package`
works; a test keeps it identical to the spec's) plus `MINIMAL_JSON` (the
fly keys, scope `minimal`, DECISIONS.md D-12). Unknown fields are errors
(the documentation fields `$note`, `modifier_vocabulary`, `note`, `also`
are allowed), as are reversed key ranges. Keys are physical `KeyCode`s
named for a US layout; digits also answer to the keypad. `mod` resolves per
`Platform`: ⌘ on macOS, Ctrl on Linux/Windows, either in the browser, where
`web: null` bindings use their `web_alt`. A `Press` binding sharing its
chord with a `Hold` binding is a *tap* (Space): it fires on release within
200 ms with no mouse button pressed in between, or at once if it went down
and up in one frame. `Held` bindings (fly) don't fire while Ctrl/Alt/⌘ is
down. The spec §13 test (`shortcuts_have_no_conflicts…`) checks every
platform: no shared keys in overlapping scopes (all overlap except
`minimal`/`teleop`), except the documented `EXCLUSIVE` pairs
(`tool.constrain_x`/`teleop.stop`, Q5) and a hold with its own tap; and
every chord the browser would see as reserved (`ctrl+W` is `mod+W` there)
is `web: null` with a non-reserved `web_alt`. The HUD's key hint is built
from the map.

**`view::ViewerControls`** (Update, chained, before
`render::apply_environment` so a camera respawn keeps the frame's
commands): `drop_command_chord_keys` (macOS browsers send no key-up for a
key released while ⌘ is held, so on ⌘'s release the keys pressed under it
are released; ⌘→ would otherwise leave → down and fly the camera),
`mouse_input` (left drag = drag action, Space held → pan; right = pan;
middle = zoom; wheel and trackpad), `key_input` (actions from the scopes
the mode enables: `global`, `viewport`, `minimal`; G toggles
`render::GzGrid`; Space tap → `GzCommand::Play/Pause` from the stats, and
losing keyboard focus cancels a tap rather than ending it; ⌘→ → `Step(1)`
when paused), `fly_input` (at most 0.1 s a frame), then
`apply_view_commands`. Bindings without an action (tools, panels, files,
`sim.reset`, which needs the full UI's undo) parse but do nothing yet.
Pointer routing around UI nodes is M2/M3.

Wheel and trackpad (DECISIONS.md D-11, D-18), summed per frame into one
`ZoomAt` about the view centre (plain wheel) and one about the cursor
(Ctrl+wheel, pinch), each capped at 5 steps so momentum flicks don't throw
the view: native line deltas are steps; pixel deltas are divided by the
window's scale factor, then natively a two-finger scroll pans (the pivot
under the cursor when it starts) and in the browser 100 px is a step
(Chrome's notch); browser lines are 3 per step (Firefox's notch). A pinch
(`PinchGesture`, or the browser's flag below) zooms to the cursor by its
scale; `PanGesture` (iOS) pans.

**Browser input** (`view::web`, spec §9.6): listeners on the page's canvas,
added when `ViewerControls` builds (they don't wait for the renderer):

- `wheel` (`passive: false`): a `ctrlKey` event is cancelled (no page zoom)
  and flags the frame in `controls::BrowserWheel`; with no Control key
  down that is a pinch.
- `gesturestart`/`gesturechange` (Safari's pinch): cancelled, the scale
  change passed on as a pinch.
- `contextmenu`: cancelled on the canvas only, so right-drag pans.
- `keydown`: `preventDefault` only for the chords in `shortcuts.json`'s
  `prevent_default_when_canvas_focused` (⌘/Ctrl+K, S, O, F, P, Space, Tab,
  and ⌘/Ctrl+→ step, DECISIONS.md D-17). The listener is on the canvas, so
  it acts only while the canvas has focus; text fields elsewhere on the
  page keep every browser shortcut. Text inputs inside the canvas (bevy_ui,
  M1+) will need the same rule.

`prevent_default_event_handling` stays off on the window (`gzweb` main):
winit would otherwise cancel every key and wheel event. With `?hud=0` the
page's `#webgpu-status` and `#bevy-status` lines are removed
(`hud::remove_dom_status`), since nothing would update them. Checked in
headless Chromium against the replay server: the cancellations above, the
chords left alone (Ctrl+R, ⌘W, plain →, Shift+Space, keys in a text
field), a CDP ctrlKey wheel zooming to the cursor without page zoom, and
the status lines with and without `?hud=0`.

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
- **On every Bevy upgrade:**
  - bump `rust-toolchain.toml` and the `dtolnay/rust-toolchain@` versions in
    `.github/workflows/ci.yml` if Bevy's MSRV changes;
  - revisit the advisory ignores in `deny.toml`;
  - update the Bevy and wgpu versions in CONTEXT.md and PORT_PLAN.md.
