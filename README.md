# gzweb-rs

A Rust/Bevy port of [gzweb](https://github.com/gazebo-web/gzweb). It renders a
live [Gazebo](https://gazebosim.org/) simulation from the Gazebo websocket
server. The same code runs as a native desktop app (Vulkan, Metal or DX12) and
in the browser (WebAssembly with WebGPU, falling back to WebGL2).

Status: early. Primitive shapes, glTF/OBJ/STL meshes, lights, basic materials
and live pose updates work. Collada, textures and most of gzweb's UI are still
to come. See [`.agent/PORT_PLAN.md`](.agent/PORT_PLAN.md).

## Layout

| Path | What |
|---|---|
| `crates/gzweb` | The app (native binary + wasm entry) |
| `crates/gz-bevy` | Bevy plugin: connection, asset sources, scene spawning, camera, HUD |
| `crates/gz-core` | Protocol, gzweb's scene rules, URI resolution (no Bevy) |
| `crates/gz-proto` | `gz.msgs` types generated from vendored gz-msgs protos |
| `crates/gz-parity` + `parity/` | Differential tests against the original gzweb |

Architecture: [`.agent/DESIGN.md`](.agent/DESIGN.md).

## Running Gazebo (Docker)

`Dockerfile.gazebo` builds Gazebo Harmonic with the websocket server on port 9002:

```bash
docker build -t gz-websocket -f Dockerfile.gazebo .
docker run --rm -p 9002:9002 gz-websocket shapes.sdf     # default world: empty.sdf
```

## Native

```bash
cargo run -p gzweb -- --url ws://localhost:9002 [--key KEY]
```

`--port N` and `GZ_WEBSOCKET_PORT` are shorthands for `ws://localhost:N`.

`--snapshot out.json` runs headless, with no window or GPU. It writes a scene
snapshot once everything has loaded, then exits.

## Browser

```bash
rustup target add wasm32-unknown-unknown
./build.sh     # release wasm + wasm-bindgen (installs the matching CLI if needed)
./run.sh       # serves the repo on http://localhost:8000
```

Open `http://localhost:8000/?url=ws://localhost:9002` in a WebGPU-capable
browser. `?key=KEY` and `?port=N` are also accepted. Without `url`, the page
connects to port 9002 on its own host.

## Tests

```bash
cargo test --workspace
```

To compare against gzweb, check out gzweb next to this repo, then:

```bash
(cd ../gzweb && npm ci && npm run build)
parity/run.sh shapes      # or: empty
```

This replays a recorded Gazebo session (`tests/corpus/<name>`) to both gzweb
(in headless Chromium) and gzweb-rs (headless). It then diffs their scene
snapshots and the messages each one sent. See
[`.agent/PARITY.md`](.agent/PARITY.md).
