# Parity Harness Spec

This is how gzweb (`../gzweb`, the reference) and gzweb-rs are compared. See
[`PORT_PLAN.md`](./PORT_PLAN.md) §4 for the rationale and test layers.

```
                 ┌────────────── tests/corpus/<name>/ ──────────────┐
 real Gazebo ──► │ gz-parity record → manifest.json + frames.bin     │
                 └───────────────────────────┬──────────────────────┘
                                             ▼
                              gz-parity replay (ws://localhost:PORT)
                               │  logs every client→server frame
                 ┌─────────────┴─────────────┐
                 ▼                           ▼
  parity/js/snapshot.mjs              gzweb --snapshot out.json
  (Playwright + real gzweb)           (headless Bevy, no GPU)
                 │                           │
                 └──────► gz-parity compare ◄┘   (snapshots + client logs)
```

## 1. Corpus format: `tests/corpus/<name>/`

Record one with `parity/record.sh <world.sdf> [name]` (needs the
`gz-websocket-jetty` image from `Dockerfile.gazebo`; `RECORD_PORT` picks the
host port, default 9010). It waits for a websocket upgrade, not just a TCP
accept, because Jetty's websocket system only starts once the world, Fuel
models included, has loaded, and gives up at once, printing the server's
log, if the server exits first. Then it lets the world run for
`RECORD_WARMUP_S` (30) seconds. That wait starts after the world has
loaded, so recordings begin at sim time ≈ 33 s for an installed world and
≈ 50 s for one with Fuel models. (The Harmonic script's 30 s started at the
TCP accept, before the world loaded, so those recordings began at
t ≈ 12 s.) Record dynamic worlds with a short warmup. `gz-parity record`
prints the sim time the pose streams cover and warns about non-finite
positions or positions beyond 10 km.

Every corpus was recorded from **Gazebo Jetty** (gz-sim 10.5.0).
`source.gazebo` is the image's release label and gz-sim version
(`jetty 10.5.0`); corpora recorded before the script read them say
`jetty`. Those were also recorded while the image ran as root, so their
scenes name Fuel files under `/root/.gz/fuel/…`; later ones under
`/home/gazebo/.gz/fuel/…`. Both clients map either to the same Fuel URL.
Status:

| Corpus | gzweb reference | Rust vs gzweb | In `cargo test` |
|---|---|---|---|
| empty, shapes, lights, nested_model, diff_drive, polylines, visibility | committed | PASS | yes |
| heightmap | committed | PASS with allow-listed terrain | `--ignored` (Fuel) |
| import_mesh | committed | PASS (Collada) | `--ignored` (Fuel) |
| sky | committed | PASS (allow-listed sky and camera material) | yes (headless gzweb-rs never fetches the cubemap) |
| fuel_textured_mesh | committed | PASS (allow-listed materials) | `--ignored` (Fuel) |
| particle_emitter | none: gzweb throws on a null `albedo_map` | — | no (no test uses it) |

gzweb couldn't load `sky`, `visibility` and `fuel_textured_mesh` from the
Harmonic recordings: gz-launch's `protos` text left out the top-level enums,
and protobufjs threw on `PixelFormatType`. Jetty's server includes them, so
these now have references.

`heightmap` is dynamic: four spheres roll on the bowl's slopes. It was
recorded with `RECORD_WARMUP_S=5`, so its pose stream covers sim
t ≈ 9.3–11.0 s, with every sphere on the terrain (z ≈ 3–8 m), close to the
Harmonic recording (t ≈ 12–13 s). The first Jetty recording (default
warmup) began at t ≈ 50 s, the spheres still on the terrain but rolling
outward. Left running, they roll off the edge of the 128 m terrain and fall,
with the ode collision detector as with bullet, so they don't fall through
it; after about 10 minutes of free fall they are at z ≈ −1.6 × 10⁶ m. This
is server-side physics; both clients just show the poses they're sent.

`particle_emitter`'s scene names the emitter's `color_range_image` by an
absolute Fuel cache path (`/root/.gz/fuel/…/fog generator/2/materials/textures/fogcolors.png`),
which only exists inside the container that recorded it.

Optional files next to `manifest.json`:

- `gaps.toml`: features the world needs that gzweb-rs doesn't have yet, in
  the allow-list format. Delete an entry when the feature lands.
- `http.lock.json`: URL → SHA-256 of every web asset (Fuel meshes and
  textures) the world loads. The files themselves are **not** committed.

**Web assets.** Both clients fetch Fuel files over HTTP, not the websocket.
`gz-parity mirror` serves them as `http://127.0.0.1:<port>/<host>/<path>`
from a local cache (`target/http-cache`, or `GZ_HTTP_CACHE`), fetching from
the real host on a miss. Each body is checked against the lock; a URL not in
the lock is added to it. `GZ_HTTP_OFFLINE=true` makes a miss a 404.
`parity/run.sh` always runs the mirror: gzweb's HTTPS requests are rerouted
by Playwright, gzweb-rs gets `--http-mirror`.

`sky/http.lock.json` pins gzweb's hard-coded default skybox,
`…/openrobotics/models/skybox/tip/files/…`. `tip` is mutable: if the model
gets a new version, a cold-cache `parity/run.sh sky` fails with a SHA-256
mismatch (502) until the lock is refreshed (delete the entries, rerun with
Fuel up). Pinning a version would mean rewriting gzweb's URLs in the mirror
or Playwright, so the lock stays on `tip` and only detects the change. The
headless test is unaffected: gzweb-rs never fetches it.

Fuel-world tests are `#[ignore]`d, so `cargo test` and the coverage gate stay
offline. CI runs them in the `fuel` job (`cargo test -p gzweb --test headless
-- --ignored`) with the cache restored from a key on the lock files, so Fuel
is contacted only on a cache miss, and only that job fails if Fuel is down.

- `manifest.json`:
  ```json
  {
    "format": "gz-parity-corpus/1",
    "world": "shapes",
    "source": {"gazebo": "jetty", "world_file": "shapes.sdf"},
    "protos": 0,
    "responses": {"topics-types": 1, "worlds": 2, "scene": 3},
    "assets": {"file:///…/mesh.dae": 7},
    "streams": {"/world/shapes/dynamic_pose/info": [10, 11, 12]}
  }
  ```
  Numbers are indices into `frames.bin`. `protos` is the reply to `protos,,,`.
  `responses` keys are request ops. `assets` keys are requested URIs, and each
  value is a full `asset,…` reply frame, either Bytes or a StringMsg error.
- `frames.bin`: a concatenation of records. Each record is `u32 LE length`
  followed by that many bytes of a server→client websocket frame, sent as binary,
  verbatim.
- `gzweb.snapshot.json` and `gzweb.client.log`: gzweb's output for this corpus,
  copied from `target/parity/<name>/gzweb.json` and `gzweb.client.log` after a
  `parity/run.sh` run. `crates/gzweb/tests/headless.rs` diffs gzweb-rs against
  them in `cargo test`, so parity regressions fail without a browser. Refresh
  them whenever the corpus or the pinned gzweb changes.

Tests that need worlds no recording has use `gz_parity::corpus::Builder`. It
builds a corpus from `gz.msgs` values: a scene, assets and stream messages.

## 2. Replay server

`gz-parity replay --corpus <dir> --port <p> [--client-log <file>] [--stream-limit N] [--connections N] [--key KEY]`

In tests, `gz_parity::replay::serve_on` serves on a listener bound to port 0.

- Serves one connection at a time, with state reset per connection.
- How it handles client text frames:

  | Client frame | Replay server action |
  |---|---|
  | `protos` | Send the recorded protos frame. |
  | `topics-types`, `worlds`, `scene,<world>,,` | Send the recorded reply for that op. |
  | `asset,,,<uri>` (also `asset,<uri>,,`) | Send the recorded reply. If `<uri>` was not recorded, send `asset,<uri>,gz.msgs.StringMsg,<StringMsg{data:"asset_not_found"}>`. |
  | `topics,,,` | Send the names of the recorded streams (`StringMsg_V`). |
  | `req,<service>,<type>,<payload>` | `/world/<world>/control` and `/server_control` answer `Boolean { data: true }`; anything else gets `StringMsg { data: "service_not_found" }`, as gz-launch does. |
  | `sub,<topic>,,` | Stream that topic's recorded frames in order, at most `N` of them (default: all), 1 ms apart. |
  | `auth,,,<key>` | Reply `authorized`, or `invalid` if `--key` is set and differs, as a binary frame like every reply (both real servers do; gzweb reads it with `FileReader.readAsText`). |
  | anything else | Ignore. |

- `--client-log` writes one line per client frame, in arrival order: the exact
  text frame, or `binary:<hex>` for a binary frame.

## 3. Snapshot format: `gz-parity-snapshot/1`

All vectors use **Gazebo world axes (Z-up)**. Quaternions are `[x, y, z, w]`.
Colours are the raw floats stored on the material or light, with no colour-space
conversion.

```jsonc
{
  "format": "gz-parity-snapshot/1",
  "source": "gzweb" | "gzweb-rs",
  "world": "shapes",
  "scene": {
    "ambient": [r, g, b],
    "clear_color": [r, g, b],
    "camera": {"position": [x,y,z], "target": [x,y,z], "fov_deg": 60.0, "near": 0.01, "far": 1000.0},
    "grid_visible": false,
    "sky": null | {"cubemap": null | "uri"}
  },
  "nodes": [
    {
      "path": "box/box_link/box_visual",
      "kind": "model" | "link" | "visual" | "light",
      "local": {"pos": [..], "quat": [..], "scale": [..]},
      "world": {"pos": [..], "quat": [..], "scale": [..]},
      "visible": true,
      "geometry": null | {
        "type": "box",
        "aabb_world": {"min": [..], "max": [..]} | null,
        "mesh_count": 1,
        "vertex_count": 24
      },
      "material": null | {
        "color": [r,g,b], "opacity": 1.0, "transparent": false,
        "emissive": [r,g,b], "metalness": null | f, "roughness": null | f,
        "maps": {"albedo": null | "uri", "normal": null | "uri"}
      },
      "light": null | {
        "type": "point" | "spot" | "directional",
        "color": [r,g,b], "intensity": f, "range": f,
        "angle": null | f, "penumbra": null | f, "decay": null | f,
        "cast_shadow": bool, "direction_world": null | [x,y,z]
      }
    }
  ]
}
```

### Node selection and paths

The node set comes from the **scene protobuf message**, not from the renderer's
graph. That keeps both sides' node sets identical by construction.

- Walk the tree in this order:
  1. For each `scene.model`: model → its `link`s → each link's `visual`s and `light`s.
  2. The model's own `visual`s.
  3. Nested `model`s, recursively.
  4. Each `scene.light`, at top level.
- `path` is the names joined with `/`. If a sibling name repeats, the second one
  gets `#1`, the third `#2`, and so on.
- Collisions and sensors are excluded for now.
- `local` is the renderer node's transform relative to its parent renderer node.
  For gzweb that is `Object3D.position/quaternion/scale`. For gzweb-rs it is the
  entity's `Transform`.
- `world` is decomposed from the world matrix and expressed in Gazebo axes.

### Field rules

- **geometry:**
  - `type` is the protobuf `Geometry.Type` name, lowercased.
  - `aabb_world` is the world-space bounding box over every mesh under the visual:
    three.js `Box3().setFromObject(visualObj, true)` (precise, from vertices; the default transforms the local box and over-estimates rotated meshes), Bevy mesh positions ×
    `GlobalTransform`, mapped to Gazebo axes. It is `null` if the meshes have not
    loaded.
  - `mesh_count` and `vertex_count` cover every renderable mesh under the visual.
- **material:** taken from the first renderable mesh under the visual. That is
  the mesh gzweb applies the SDF material to.
- **light:**
  - `intensity`, `range`, `angle`, `penumbra` and `decay` use **three.js
    semantics**. gzweb-rs stores those values next to the Bevy light it
    derives from them.
  - `direction_world` is the normalised (target − light) world direction for
    spot and directional lights.
- **visible** is the node's own visibility flag. It does not include ancestors.

### When to take the snapshot

1. The connection reaches `ready`, meaning the scene message has been processed.
2. Every mesh and texture load has finished, either loaded or failed.
3. The streams the replay server is configured to send have been processed.
4. Then a further settle delay passes (default 1000 ms).

## 4. Comparator

`gz-parity compare --expected <gzweb.json> --actual <rs.json> [--allow tests/parity/allowlist.toml] [--report out.json]`

- Nodes are matched by `path`. Every missing or extra node is reported as a diff.
- Default tolerances:

  | Field | Tolerance |
  |---|---|
  | `pos`, `scale` | 1e-5 absolute |
  | `quat` | `1 - |dot| < 1e-6` (sign-agnostic) |
  | `aabb_world` | 1e-3 absolute + 1e-3 relative |
  | colours | 1/255 |
  | light scalars | 1e-4 relative |
  | `vertex_count` | ignored, because tessellation differs |

- Allow-list entries look like
  `{ id = "spot-penumbra-clamp", path = "*", field = "light.penumbra", reason = "..." }`.
  `path` is a glob. A diff that matches an entry is reported as *suppressed*,
  not hidden.
- `gz-parity compare-protocol <gzweb.client.log> <rs.client.log>` diffs the
  client-frame sequences.
- The exit code is non-zero when there are unsuppressed diffs.

## 5. Driver

`parity/run.sh <corpus-name>` does the following:

1. Builds gz-parity and the gzweb-rs headless binary.
2. Starts the replay server, runs the gzweb snapshot, and stops the server.
3. Restarts the replay server and runs the gzweb-rs snapshot.
4. Compares both snapshots and both client logs.
5. Writes everything to `target/parity/<corpus>/`.

CI's `parity` job runs it for every corpus without web assets against
gzweb pinned at `519d4cf` (`.github/workflows/ci.yml`); the Fuel corpora
are checked against their committed references in the `fuel` job.
