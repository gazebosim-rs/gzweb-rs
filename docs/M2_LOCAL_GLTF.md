<!-- Copyright (C) 2026 Open Source Robotics Foundation -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Local glTF visuals on moving primitive bodies

This candidate extends the accepted M1 viewer at `0db1f4e` in an isolated
worktree. M2 is not yet qualified: final runtime/coverage, WASM bundles and actual browser
qualification remain pending. Partial normal-hook and runtime precursors are
recorded below. The original M1 worktree, binaries,
controls, protocol, default Gazebo backend and evidence remain intact.

Use the existing explicit primitive backend, for example
`?backend=primitive&ui=full&url=ws://127.0.0.1:9002/`. There is no second viewer,
mesh-collision implementation, remote Fuel loader or texture conversion.
Rapier still simulates primitive collisions; only the displayed geometry changes.

## Narrow scene and resource contract

Primitive-only complete frames retain version 1. A complete frame containing a
mesh uses version 2; all existing integer identity, epoch, sequence, time,
coalescing and control/committed-ack rules remain unchanged. Controls and
acknowledgements remain version 1. The additive shape is:

```json
{"kind":"mesh","uri":"http://127.0.0.1:9003/assets/0/models/arrow.gltf","scale":[1.5,0.75,2.0]}
```

The URI names a loopback HTTP server with a nonzero 16-bit port, a numeric asset
scope, and a `.gltf` or `.glb` entry. URI length is at most 2048 bytes. Path
segments contain only ASCII letters, digits, `.`, `_` and `-`; empty, `.` and
`..` segments, encoding, query strings, fragments and backslashes are refused.
`file://`, `model://`, HTTPS, other hosts and other mesh formats are refused on
this bounded route. This does not change the default Gazebo backend's formats or
URI rules. Scale must be positive, finite and representable as positive finite
f32 renderer values. Existing 64 KiB/128-visual/name/ID/opaque-color bounds stay.

The simulator resolves local SDF resources, validates the maintained glTF
manifest and exposes an immutable declared-file allowlist. Its HTTP service
preserves relative buffer/image paths within one scope; it does not serve
arbitrary filesystem paths or remote resources. The viewer's dedicated
`m2mesh://` source retains a scope guard, fetches direct HTTP and never retries a
failure through Gazebo asset RPC. The maintained glTF parser validates the entry, followed by an explicit
`asset.version == "2.0"` and absent-or-`"2.0"` minimum-version guard;
external and scope-escaping buffer/image references fail before that document
is given to Bevy's existing loader. The viewer defense permits relative `..` only while remaining inside the
declared scope, but the current simulator deliberately rejects *all* dot and
dot-dot dependency segments: those wider viewer checks do not promise server
admission. Inline data stays with the maintained loader. Undeclared files get
actual HTTP errors, not synthesized geometry. M2-only native fetches follow no
redirects and accept HTTP 200 only; browser fetch uses maintained typed
`RequestRedirect::Error`. The existing Gazebo HTTP fallback behavior is unchanged.
M2 accepts at most 16 MiB per resource; browser bytes are checked after its
ArrayBuffer arrives, so the simulator's declared-size guard remains the prefetch
boundary for this trusted local endpoint.

No mesh file, mesh handle or asset request is created for a primitive-only
scene. Mesh URI and scale are layout identity: unchanged complete frames use the
existing pose lane and retain handles/entities rather than loading every 30 Hz.
Reconnect/reset uses the existing authoritative scene replacement and ownership
retirement. No global asset cache or new plugin framework is introduced.

## Frames, scale and materials

The wire translation and XYZW quaternion are the authoritative visual WORLD
pose in Gazebo Z-up, including any SDF model/link/visual offsets. Mesh scale is
in the SDF/Gazebo geometry axes. Bevy 0.20's maintained glTF loader normally keeps
glTF vertices and local node transforms unchanged; M2 explicitly retains that
setting and disables imported cameras, lights and animation playback assets.
Embedded materials and textures stay on the maintained loader path. Wire palette
color does not replace a mesh's embedded material (`material.present=false`).

For meshes only, the outer visual rotation is `worldQ * Rx(+90 degrees)` and the
outer scale is `[sx, sz, sy]`. Because Bevy uses an `R*S` local matrix (scale acts on the local point first),
this gives
`worldQ * Rx90 * Scale(sx,sz,sy) = worldQ * Scale(sx,sy,sz) * Rx90`.
Thus intrinsic glTF Y-up becomes Gazebo Z-up exactly once, with nonuniform scale
in the requested Gazebo axes. Subsequent pose messages use the same corrected
quaternion. The existing single `GzWorldRoot` still converts all Gazebo geometry
to Bevy's display basis; primitives receive no new correction.

## Errors and authored controls

A missing HTTP entry/dependency, malformed glTF or valid document without a
scene produces one URI-bearing error through the existing viewer message UI.
Only M2 entities receive the load-failure marker; legacy glTF behavior remains
unchanged. No fallback mesh, artificial successful status or fake simulation
metrics are supplied. The simulation control connection remains independent of
asset failure, so its real clock/acknowledgements stay authoritative.

Authored regressions cover version/URI/scale refusal, external and escaping
resources, known nonuniform-scale/rotated-visual transforms, initial-versus-pose
quaternion continuity, unchanged mesh handles on motion, layout replacement on
URI/scale changes, actual loopback HTTP + maintained glTF loading with a relative
binary buffer, actual instantiated Mesh3d descendant transforms and retained
entity/handle after movement, embedded material provenance/name preservation and
no SDF material override, scene retirement, redirects, document versions, and missing,
malformed and no-scene error reporting exactly once. The headless imported mesh carries the maintained glTF material name/provenance;
this is not a substitute for the full PBR material binding/browser check. These
tests have not run.
Compilation, retained coverage, both maintained renderer bundles, actual moving
mesh/browser controls and exact hosted CI remain required before acceptance.

## First normal-hook failure and reviewed correction

The first normal commit hook stopped after 26.01 s without a capacity stop or
commit. Generic checks and rustfmt passed. Native all-target Clippy found four
issues: the removed Bevy 0.20 `AssetServer::load_with_settings` call, an
unnecessary `AssetId` qualification, and two temporary reader-path lifetimes.
Rustdoc reported the same loading-API error; the WASM Clippy phase was not reached.
The unchanged dependency license/bans/sources audit passed. No runtime tests,
coverage, bundles or browser results were produced by this attempt.

The correction uses the maintained `load_builder().with_settings(...).load(...)`
API with identical glTF settings, shortens only the redundant type path, and
binds both path strings for the lifetime of their readers. Geometry, inputs,
assertions, material/URI/redirect guards and all quality thresholds are unchanged.
The corrected hook and all subsequent verification remain unrun until independent
source review and coordinated release.

The affected hook also stopped after 26.01 s without a capacity stop or commit.
The previous four issues were resolved; native all-target Clippy then reported
one redundant clone at the final use of the new test's scene. Generic checks,
rustfmt, rustdoc and the dependency audit passed. The narrow correction moves
that final-use value instead of cloning it, with every assertion and production
byte unchanged. WASM Clippy, runtime tests, coverage and bundles remain unrun;
the next affected hook requires independent source review first.

The next affected hook stopped after 25.01 s, again without a capacity stop or
commit, on two newly emitted `manual_let_else` lints in the missing/invalid
resource negative tests. Both bindings now use equivalent `let Err(..) .. else`
patterns with identical failure messages and assertions. Production code remains
unchanged; generic checks, rustfmt, rustdoc and dependency audit passed. WASM
Clippy and runtime/coverage verification still have not run. This correction is
subject to independent source review before the next affected normal hook.

The next normal commit hook passed in 85.02 s, including native/WASM Clippy,
rustfmt, rustdoc and the dependency audit, creating `03cfd37e`. That result did
not provide runtime, coverage, bundle or browser acceptance.

The first normal pre-push run failed after 146.03 s without a capacity stop or
publication. The gz-bevy target reported 388 passing tests and one failure: the
new three-error UI control captured zero messages after its unchanged 30 s wait.
The other local glTF HTTP, version/redirect and instantiated-mesh controls passed;
Cargo stopped before the remaining workspace targets or final coverage export,
so this attempt provides no 95% coverage verdict.

That test installs a thread-local tracing subscriber, which maintained tracing
does not propagate to Bevy's worker threads. The application instead installs
Bevy's global subscriber. The correction configures only this test's Update
schedule with the maintained `SingleThreadedExecutor::new()`, keeping its actual
reporting systems in the local subscriber scope and retaining deferred command
application. Production scheduling/logging is unchanged. All three actual
missing/invalid/no-scene error assertions, exactly-once checks and the 30 s
wait remain unchanged. The correction requires independent source review and
affected normal hooks. Actual browser verification must additionally display a
corrupt-image/asset failure using production global logging; source logs or
this headless test alone do not satisfy that acceptance condition.

The affected normal pre-push run stopped after 139.04 s without a capacity stop
or publication. All 389 gz-bevy tests, including the corrected three-error UI
control, passed. The later gz-ui target had 458 passing tests and one failure in
its existing unreadable-workspace-file warning/tray control (zero captured
warnings versus one). Its existing PreUpdate executor and actual file operation
were already correct; no final coverage verdict was produced.

Pinned tracing-core's one-live-dispatch interest-cache fast path consults the
registering thread's default subscriber. Other parallel UI tests can register
that warning while using a NOP subscriber, racing the control's existing cache
rebuild. The failed interleaving was not instrumented, so this source-supported
mechanism is not claimed as an experimentally established cause. The narrow
test-only correction retains a second scoped public tracing
Dispatch while the actual file read runs, selecting the maintained multi-dispatch
registry path. No global subscriber or logging framework is installed. The
existing file read, subscriber/channel, UI/persistence assertions, scheduler and
frame count remain unchanged. This is the sole existing UI test observer adjustment;
no legacy predicate is relaxed. The correction requires independent source review
and the affected normal hooks before any qualification claim.
