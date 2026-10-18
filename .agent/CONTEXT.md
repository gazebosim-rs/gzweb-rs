# gzweb-rs — Agent Context

## Project Overview

**gzweb-rs** is a modern Rust application that renders live [Gazebo](https://gazebosim.org/) robotic simulations in 3D. It connects to a running Gazebo instance via WebSocket, receives the scene description as protobuf messages, and renders it using [Bevy](https://bevyengine.org/) with WebGPU.

The application is designed to run **both in a web browser (WebAssembly/WASM) and as a native desktop application** from a single Rust codebase.

### Key Goals

- **WebGPU-first rendering** — performance and high-quality graphics are the top priority.
- **Dual-target**: browser (WASM + WebGPU) and native desktop (Vulkan/Metal/DX12 via wgpu).
- **Real-time** connection to a live Gazebo simulation websocket server.
- **Clean ECS architecture** using Bevy's Entity-Component-System model.
- **Bevy for everything visual**: rendering and UI (bevy_ui). No DOM-built UI.
- **Parity with gzweb** (`../gzweb`, the TypeScript/three.js original), checked by differential tests.

---

## Technology Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust stable (edition 2021) |
| Engine, ECS, rendering, UI | [Bevy](https://bevyengine.org/) 0.20 (wgpu 30, rustc 1.97.1; WebGPU only in the browser for now; bevy_ui) |
| Web target | `wasm32-unknown-unknown`, `wasm-bindgen`, `web-sys` |
| Protobuf | `prost` 0.13, generated with `prost-build` + `protox` from vendored gz-msgs (Harmonic) |
| Native websocket | `tungstenite` 0.26 |
| Camera controls | `bevy_panorbit_camera` 0.34 |
| Mesh loaders | Bevy glTF, own COLLADA (`gz-collada`), OBJ (`tobj`) and STL loaders |
| Parity harness | `gz-parity` (Rust) + Playwright/three.js (`parity/js`) |

---

## Repository Layout

```
gzweb-rs/
├── .agent/          CONTEXT (this), DESIGN, PORT_PLAN, PARITY, WORKLOG
├── crates/
│   ├── gz-proto/    generated gz.msgs types (proto/ = vendored .proto files)
│   ├── gz-core/     protocol, scene rules, URIs, snapshot types (no Bevy)
│   ├── gz-bevy/     Bevy plugin
│   ├── gzweb/       the app (native + wasm)
│   └── gz-parity/   record / replay / compare tool
├── parity/          run.sh + js/ (gzweb reference exporter)
├── tests/           corpus/ (recorded sessions), parity/allowlist.toml
├── spikes/          standalone experiments (not in the workspace)
├── index.html       browser host page
├── build.sh         wasm build (release) + wasm-bindgen
└── Dockerfile.gazebo  Gazebo Harmonic + websocket server
```

---

## Build & Run

```bash
cargo test --workspace                       # unit tests
cargo run -p gzweb -- --url ws://localhost:9002
cargo run -p gzweb -- --url ws://localhost:9002 --snapshot out.json   # headless
./build.sh && ./run.sh                       # wasm; open http://localhost:8000/?url=ws://localhost:9002
```

| Setting | Native | Browser |
|---|---|---|
| Server | `--url ws://host:port` or `--port N` / `GZ_WEBSOCKET_PORT` | `?url=ws://host:port` or `?port=N` |
| Auth key | `--key KEY` | `?key=KEY` |

The default is `ws://localhost:9002`. In the browser the default host is the
page's hostname.

### Gazebo for development

```bash
docker build -t gz-websocket -f Dockerfile.gazebo .
docker run --rm -p 9002:9002 gz-websocket shapes.sdf
```

### Parity check against gzweb

```bash
(cd ../gzweb && npm ci && npm run build)    # once
parity/run.sh shapes                        # uses tests/corpus/shapes
```

See [`PARITY.md`](./PARITY.md).

---

## See Also

- [`DESIGN.md`](./DESIGN.md): architecture, data flow, coordinate frames, protocol
- [`PORT_PLAN.md`](./PORT_PLAN.md): gap analysis and phased plan
- [`PARITY.md`](./PARITY.md): differential test harness spec
- [`WORKLOG.md`](./WORKLOG.md): dated log of what changed and what was verified
