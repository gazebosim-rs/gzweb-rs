<!-- Copyright (C) 2026 Open Source Robotics Foundation
SPDX-License-Identifier: Apache-2.0 -->

# M1 primitive simulation backend

The maintained Bevy 0.20 viewer has an explicit, additive consumer mode for the
local simulator bridge described in gz-sim-rs/docs/MVP_BROWSER.md. The ordinary
Gazebo websocket protocol remains the default:

```
cargo run -p gzweb -- --backend primitive --url ws://127.0.0.1:9002/
# Browser after ./build.sh, served from this repository:
# http://127.0.0.1:8081/?backend=primitive&ui=full&url=ws://127.0.0.1:9002/
```

The existing Bevy workbench owns rendering, camera, connection/retry controls,
Play/Pause and Reset. This is a separate JSON backend, not a replacement for
Gazebo's protobuf websocket server or an implementation of Zenoh in the browser.
Authentication keys are explicitly unsupported for the primitive mode.
It uses Rust serde integer parsing, with no JavaScript JSON-number intermediary.

Complete version-1 scenes are bounded to 64 KiB, 128 visuals and 256 UTF-8 bytes
per name, with opaque alpha exactly one. World and visual IDs are nonzero, unique in their respective scene,
and checked against the viewer's existing u32 identity range. Larger IDs are
explicitly refused instead of truncated. Duplicate display names are valid;
paths use actual IDs. Box full dimensions, sphere radii, world translations and
XYZW quaternions remain in Gazebo Z-up/metres beneath the existing GzWorldRoot.
That root performs the only basis conversion. Diffuse RGBA is already linear.

The backend coalesces full scene input and uses the existing pose-update lane
when admitted IDs/names/geometry/materials/epoch remain unchanged. Reconnection,
world replacement and committed reset retire the previous scene; a new complete
frame reconstructs it with the same public IDs. No hardcoded animation or
fallback scene is used. Socket events are bounded (64 frames, each at most
64 KiB; at most 64 events are handled per render poll). The native socket limits
frame and assembled-message lengths to 64 KiB before reading; outbound commands are bounded and nonblocking. Overflow closes the
connection. Handshake/stream and committed-acknowledgement timeouts close it and
report unknown command outcomes rather than reporting success.

Only one pause/resume/reset request can be pending. IDs increase without reuse
across reconnects. Authoritative paused/time state changes only on a committed
scene or validated acknowledgement. A successful reset requires exactly the next epoch,
paused=true and exactly zero integer time; stale scenes remain fenced out.
Unsupported topics, publishing, services, edits, Stop and Step return explicit
unsupported errors. The Step UI is unavailable. The existing status hint displays
control results/errors. Disconnection clears geometry and clock. Partial clock
information never fabricates iterations, RTF, real time or step size: those
readouts remain unavailable. Full Gazebo statistics retain their existing path.

## Browser graphics

`./build.sh` produces separate preferred WebGPU and maintained WebGL2 bundles,
using Cargo feature fingerprints in the same target directory. Each release
bundle passes the existing 14,000,000-byte gzip budget; no threshold is changed.
The host first requests an actual WebGPU adapter. If none is available, it probes
and releases a WebGL2 context before loading the GL-only bundle. This is startup
capability selection, not a retry of a failed simulation. A renderer panic or
startup failure is visible in the existing host diagnostic; the Bevy canvas is
hidden and primitive browser socket handling stops on the failure marker.

The distinction follows pinned wgpu 30.0.1 `api/instance.rs::Instance::new`
(selection from navigator.gpu before adapter discovery), Bevy PBR 0.20.0
`render/mesh.rs` WEBGL2 shader conditional and `render/light.rs`
MAX_CASCADES_PER_LIGHT=1 for GL-only WASM. The GL bundle uses Low quality (no
compute TAA/SSAO/environment filtering) and one shadow cascade; native and default
WebGPU quality behavior are unchanged. Real graphics availability and software
renderer performance must be observed, not inferred from compilation.

## Verification status

The final independently Source-reviewed two-flavor build completed successfully
in 1069.11 s with no disk-guard stop. Current WebGPU gzip size is 12,873,810 bytes
(WASM SHA-256 `852ce51c41b547437ae49ecdf8f3f44112980e53d2974bd36db19b63ba5ab23f`);
current WebGL2 is 12,842,420 bytes
(`cfd8ac9c8f6e5107d95de8509d3ff4d76d941fdb2d2fbd04553766304fa2f5fe`).
Both satisfy the unchanged 14,000,000-byte per-bundle budget. Neither bundle is
being relabeled from an older source revision.

Root's actual final full-UI browser probe displayed twenty colored dynamic
boxes/spheres and the ground, with 21 real V rows and scoped names. Play advanced
the actual poses and displayed clock to 2.915 s. Pause acknowledged epoch 0 at
6.905 s with paused=true. Reset acknowledged epoch 1, paused at exactly zero,
restoring the initial poses. The current-tab console filter contained no
`generate_world_sdf` entries; the normal unsupported-query warning was absent.
These bounded observations used actual WebGL2 rendering on ANGLE/NVIDIA RTX 2070.
They are not a WebGPU runtime qualification or a performance result. Unreported
physics metrics remain unavailable. Portable JPEG screenshots and their hashes
are retained in the simulator's `docs/evidence/m1-browser` evidence packet.

The final tree adaptation lists only actual direct primitive visual roots as V
rows, retaining real IDs, stable paths, selection and visibility. Default Gazebo
and nested visual hiding remain unchanged. An explicit world-SDF capability
refusal suppresses automatic extras requests without fabricating SDF/model data;
unknown Gazebo capability retains the original service probe.

Earlier attempts remain separate:

- The first canonical two-flavor build passed in 1661.14 s, but its first browser
  probe rendered only grid/HUD and no live scene: the adapter retired a socket
  during asynchronous Connecting before Open. The correction uses an explicit
  pending send-failure marker; status strings never authorize disconnect.
- The corrected GL build passed in 536.06 s, at 12,841,669 bytes gzipped. Its live
  probe established motion, committed pause at 29.645 s and reset to epoch 1 at
  paused zero, then identified the tree and automatic-query issues fixed above.
  Fresh advancing reconnect and shutdown were also observed on that preceding
  corrected core bundle; no additional such observation is attributed here to
  the later UI-only batch.
- Final-build session 53755 ended with SIGTERM/exit 143 before its runner wrote a
  result. Its log contained no compiler diagnostic, and its minimum free space
  is unknown. The unchanged-source fresh-TTY continuation produced the final
  1069.11 s pass; the interrupted attempt is not a product pass or code failure.
- Earlier non-TTY server and HTTP processes ended before control/reload attempts.
  The reviewed server restarted in a fresh TTY and reset succeeded. These
  process-lifetime events are not reset/application defects.

The unrelated preserved Bevy 0.18 prototype and its initial failed WebGPU browser
probe remain historical evidence, not this maintained frontend's acceptance.

Consolidated required Rust checks, at least 95% line coverage and exact hosted CI
are still pending; the builds and browser observations do not replace them.
`node --test tests/startup.mjs` is wired into the existing Node 24 parity job and
must run once in final consolidated verification. Those pure startup-selection
controls are not a live renderer substitute. Existing parity, coverage, dependency
and payload gates are unchanged.

The first normal commit hook completed its text/metadata checks, rustfmt and
rustdoc, then failed after 168.04 s without a capacity stop. Native all-target
Clippy rejected one unnecessary qualification in the new SDF-capability test;
the correction removes only that path prefix. The WASM Clippy phase was not
reached. Cargo-deny also needed locked foreign-target crate sources absent from
the selectively populated Linux/WASM cache. Completing that cache does not
change Cargo.lock, dependency policy or offline verification. No commit,
coverage, review push or hosted qualification resulted from this attempt. The
completed correction remains subject to independent source review and the
normal affected gate.
