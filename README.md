# gzweb-rs

A Rust/Bevy port of [gzweb](https://github.com/gazebo-web/gzweb). It renders a
live [Gazebo](https://gazebosim.org/) simulation from the Gazebo websocket
server. The same code runs as a native desktop app (Vulkan, Metal or DX12) and
in the browser (WebAssembly with WebGPU; a WebGL2 build is planned).

Status: early. Primitive shapes, lights, Gazebo's materials (PBR maps,
transparency, SDF materials on meshes) and live pose updates work, and match
gzweb's scene structure on the recorded test worlds. Collada meshes load with their
textures and match gzweb's loader on Fuel models; glTF/OBJ/STL loading is wired
up but not yet checked against a mesh world. Most of gzweb's UI is still to
come. See [`.agent/PORT_PLAN.md`](.agent/PORT_PLAN.md).

## Layout

| Path | What |
|---|---|
| `crates/gzweb` | The app (native binary + wasm entry) |
| `crates/gz-bevy` | Bevy plugin: connection, asset sources, scene spawning, camera, HUD |
| `crates/gz-core` | Protocol, gzweb's scene rules, URI resolution (no Bevy) |
| `crates/gz-collada` | COLLADA (`.dae`) parsing into a node tree (no Bevy) |
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
`wss://` works; `--ca-cert ca.pem` trusts a private CA. The client reconnects
when the server goes away (`--no-reconnect` to turn that off).

Space toggles play/pause, N steps a paused simulation and G toggles the grid.
`--quality low` trades TAA, SSAO and shadow resolution for speed.
`--screenshot out.png` saves one frame once the scene has loaded, then exits.

`--snapshot out.json` runs headless, with no window or GPU. It writes a scene
snapshot once everything has loaded, then exits.

## Browser

```bash
./build.sh     # release wasm + wasm-bindgen (installs the matching CLI if needed)
./run.sh       # serves the repo on http://localhost:8000
```

Open `http://localhost:8000/?url=ws://localhost:9002` in a WebGPU-capable
browser. `?key=KEY` and `?port=N` are also accepted. Without `url`, the page
connects to port 9002 on its own host.

## Development

One script is the quality gate, for both pre-commit and CI:

```bash
scripts/check.sh            # everything below
scripts/check.sh fmt        # rustfmt --check
scripts/check.sh clippy     # clippy -D warnings, native (all targets) + wasm32
scripts/check.sh doc        # rustdoc -D warnings
scripts/check.sh deny       # cargo-deny: licences, bans, sources (CHECK_ADVISORIES=1 adds RustSec)
scripts/check.sh coverage   # all tests under cargo-llvm-cov; fails below 95% line coverage
```

Lint policy lives in `[workspace.lints]` (`Cargo.toml`) and `clippy.toml`. In
production code, `unsafe` is forbidden and `unwrap()` is denied. Dependency
policy lives in `deny.toml`; every advisory ignore needs a reason.

One-time setup (rustup installs the pinned toolchain, its components and
the wasm32 target from `rust-toolchain.toml` on first use):

```bash
cargo install cargo-llvm-cov cargo-deny --locked
pipx install pre-commit && pre-commit install
```

The git hooks run the same checks. The coverage check runs the full test suite,
which takes a minute or two on a warm build, so it runs on `git push` rather than
on every commit. CI
(`.github/workflows/ci.yml`) also runs RustSec advisories, a wasm build, and the
browser parity check against a pinned gzweb.

### Tests

```bash
cargo test --workspace
```

`crates/gzweb/tests/headless.rs` runs the real `gzweb` binary headless against
an in-process replay server. The cases:

- **Recorded corpora:** checked against the committed gzweb reference output
  in `tests/corpus/<name>/gzweb.snapshot.json` and `gzweb.client.log`, so
  parity regressions fail `cargo test` without a browser.
- **Synthetic world:** covers every primitive, mesh loading over the websocket
  and HTTP, lights, nested models, pose updates and `scene/info` additions.
- **Failures:** bad auth and connection loss.

Worlds that load Fuel meshes or textures are `#[ignore]`d because a cold
cache downloads them (see `.agent/PARITY.md` §1). Run them with
`cargo test -p gzweb --test headless -- --ignored`.

To run the full comparison against a live gzweb, check out gzweb next to this
repo, then:

```bash
(cd ../gzweb && npm ci && npm run build)
parity/run.sh shapes      # or: empty
```

This replays a recorded Gazebo session (`tests/corpus/<name>`) to both gzweb
(in headless Chromium) and gzweb-rs (headless). It then diffs their scene
snapshots and the messages each one sent. See
[`.agent/PARITY.md`](.agent/PARITY.md). After changing a corpus or bumping
gzweb, refresh the committed references from `target/parity/<name>/gzweb.json`
and `gzweb.client.log`.
