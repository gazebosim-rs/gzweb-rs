# parity/js: gzweb snapshot

This directory holds the gzweb (reference) side of the parity harness. It loads
the real, unmodified gzweb in headless Chromium, connects it to a Gazebo
websocket server (or to `gz-parity replay`), waits for the scene to settle, and
writes a `gz-parity-snapshot/1` JSON file. The format is in
[`.agent/PARITY.md`](../../.agent/PARITY.md) §3.

## Setup (once)

```sh
# gzweb checkout next to gzweb-rs. node_modules and dist are gitignored there.
(cd ../gzweb && npm ci && npm run build)
# Playwright 1.60.0 pins chromium revision 1223.
(cd parity/js && npm ci && npx playwright install chromium)   # skip install if ~/.cache/ms-playwright/chromium*-1223 exists
```

## Run

```sh
node parity/js/snapshot.mjs --url ws://localhost:9012 --out shapes.json \
  [--settle-ms 1000] [--timeout-ms 60000] [--screenshot shot.png] \
  [--console-log console.txt] [--precision 6] [--gzweb ../gzweb] \
  [--key <auth key>] [--allow-pending] [--headed]
```

To run it against real Gazebo:

```sh
docker run --rm -d --name gz-parity-js -p 127.0.0.1:9012:9002 gz-websocket-jetty shapes.sdf
sleep 10
node parity/js/snapshot.mjs --url ws://localhost:9012 --out shapes.json
docker stop gz-parity-js
```

The exit code is 0 on success, 1 on a failure or timeout (the message says
which phase timed out: `connect`, `loading` or `settle`), and 2 on bad usage.
The browser console goes to `--console-log`, including page errors and failed
requests.

## How it works

- `snapshot.mjs` uses esbuild to bundle `../gzweb/dist/gzweb.module.js`
  together with gzweb's own `node_modules`, so there is a single copy of
  three. The bundle is an in-memory IIFE. The script serves it, plus
  `harness.html` and `dump.js`, from an in-process HTTP server on a random
  port. It then drives Chromium (SwiftShader WebGL) with Playwright.
- `harness.html` creates `new gzweb.SceneManager({elementId})` without a URL,
  hooks into it, then calls `mgr.connect(url)`. The hooks only observe:
  - `transport.getAsset` is wrapped to count the websocket asset requests
    still waiting for a reply.
  - `THREE.DefaultLoadingManager.itemStart/itemEnd` are wrapped to count
    pending loads, such as the default sky's CubeTextureLoader.
  - The connection status history is recorded.
- Readiness check: wait for the status to reach `ready` and a scene message
  to arrive. Then wait until there are 0 pending websocket assets, 0 pending
  items in every gzweb loader manager (`WsLoadingManager.itemsTotal -
  itemsLoaded`), and 0 default-manager items. Finally, wait until the
  scene-graph fingerprint (object, mesh and vertex counts) stays unchanged for
  `--settle-ms`.
- `dump.js` walks `transport.sceneInfo$.getValue()`, the decoded `gz.msgs.Scene`,
  in spec order and looks up each three.js object by name under its parent.
- Floats are rounded to `--precision` decimals (default 6) and `-0` is written
  as `0`. Numeric arrays go on one line. Key order is fixed.

## gzweb quirks (the Rust side must know these)

- **Axes:** gzweb's three.js scene is already Z-up (`camera.up = (0,0,1)`,
  grid rotated into XY). No axis conversion is applied.
- **Proto access:** the scene message is a protobufjs `Message` instance (from
  `Type.decode`, not `toObject`). Enums are numbers, and unset sub-messages are
  `null` on the prototype. The geometry type name comes from the field's
  `resolvedType.valuesById`.
- **Light wrapping:** every light is a plain `Object3D` named after the light.
  It carries the SDF pose and has one child, the `THREE.*Light`, with the same
  name and an identity local transform. Spot and directional lights have a
  `target` Object3D child at local `direction` (default `(0,0,-1)`). The node
  `local`/`world` values are the wrapper's. The `light` block reads the child.
  Link lights also get `userData = {type: "light"}` and
  `visible = enableLights`.
- **Light colour is always white:** `new THREE.PointLight(protoColor)` ignores
  a `gz.msgs.Color` object, because `Color.set()` only accepts Color, number or
  string. So `color` is `[1,1,1]` whatever `<diffuse>` says. For example,
  shapes.sdf's sun has diffuse 0.8 but shows `[1,1,1]`.
- **Light intensity** is
  `1/(1+linear) * 1/(1+quadratic)`, using the attenuation terms. The sun in
  shapes.sdf gives 0.98911. `<intensity>` is ignored. Directional lights have
  no `distance`, so `range` is `null`. Point lights keep `distance = range`
  and `decay = 1` (three r141's default).
- **Spot penumbra** is `max(1, (outer-inner)/((inner+outer)/2))`, so it is
  always ≥ 1 and is likely meant to be `min`. lights.sdf gives 1.333333.
  `decay = spot_falloff`, and `angle = spot_outer_angle`.
- **Scene background** is ignored. The clear colour is always `0xb2b2b2`,
  which is `0.698039` per channel.
- **Ambient** is the scene message's `ambient`. It is 1,1,1 in shapes.sdf and
  0.4 in empty.sdf. The default before the message arrives is `0x191919`.
- **Sky:** gzweb calls `addSky()` whenever `scene.sky` is set. With no
  `cubemap_uri` header, it loads the Fuel skybox JPEGs over the internet. The
  snapshot's `sky` comes from the proto: `{cubemap: <cubemap_uri header> |
  null}`, or `null` when there is no sky.
- **Camera:** the camera is at `(-6,0,6)`, looking at `(0,0,0)`, with fov 60,
  near 0.01 and far 1000. The OrbitControls target is `(0,0,0)`.
- **Model-level visuals** (`model.visual`) are never created by gzweb's
  `spawnModelFromSDF`. They are still emitted, as all-null nodes (`local`,
  `world` and `visible` are `null`), and a warning goes to stderr. Any other
  proto node with no renderer object is emitted the same way.
- **Visuals without geometry** get an unnamed Object3D, so they show up as
  missing.
- **Collisions** are siblings of visuals under the link, added after all
  visuals, with `visible = false`. The lookup is by name in child order, and
  separates light wrappers from everything else, so a collision with the same
  name as a visual can't be matched by mistake.
- **Mesh placement:** simple shapes are a single `THREE.Mesh` that is a direct
  child of the visual Object3D. Meshes loaded from files sit deeper.
  `material` comes from the first `isMesh` descendant in `traverse()` order.
- **Pose stream:** `dynamic_pose/info` is applied with
  `scene.getByName(name)`, which returns the first match anywhere in the graph.
  Against live Gazebo, objects that are still moving make runs differ
  (shapes.sdf's ellipsoid rolls for a while). Use `gz-parity replay` for
  deterministic results.
- **Worlds with camera sensors fail to load in gzweb:** decoding the Scene
  message throws `no such Type or Enum '.gz.msgs.PixelFormatType' in Type
  .gz.msgs.CameraSensor`, so the status stays `connected` and the scene never
  arrives. sky.sdf fails this way. The snapshot times out in phase `connect`
  and reports the page error.
- **Asset callbacks** are keyed by URI in `transport.assetMap`, so a second
  request for the same URI replaces the first callback. The harness counts
  requests per URI.
- **Geometry vertex counts** are three.js tessellations, for example box 24,
  cylinder 196, sphere 1089, capsule 2210, cone 131 and plane 4. The
  comparator ignores them.
