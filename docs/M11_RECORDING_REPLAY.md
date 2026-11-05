<!-- Copyright (C) 2026 Open Source Robotics Foundation -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M11: read-only recording playback viewer

This is implemented source under review, not a passing verification or acceptance
claim. The isolated feature starts at accepted viewer `952a5202`. M11 requires
passing mandatory verification, release budgets and actual browser playback before
acceptance. The simulator's
`docs/M11_RECORDING_REPLAY.md` owns file admission, recording, playback pacing,
asset lifetime and the bridge contract. The viewer adds no file reader, recorder,
physics runtime, dependency or second renderer. Existing parity record/replay
commands keep their historical test-tool behavior.

## Explicit mode

Native: `gzweb --backend primitive --lifecycle --replay --ui full --url ws://127.0.0.1:PORT/`.
Browser: `?backend=primitive&lifecycle=1&replay=1&ui=full&url=ws://127.0.0.1:PORT/`.
Use the actual simulator playback endpoint's port, not an unrelated live server.
Default Gazebo and ordinary live primitive selection remain unchanged.

Replay negotiates exactly one `replay=1`, `lifecycle=1` and
`lifecycle_credit=1` on its WebSocket. It does not automatically request scans;
any explicit `scan` query is refused. A live viewer refuses a replay query without
its own explicit opt-in. Sensor streams and recorded images are outside this
slice. The existing 64-event queue/drain, 64 KiB message limit, ten-second
connection/idle and seven-second pending-ACK deadlines are retained.

## Coherent provenance and credit

A normal publication is Scene, read-only lifecycle catalog, strict version-1
`replay_status`, then publication credit. Status must exactly match the current
Scene world, transport epoch/sequence and integer seconds/nanoseconds, and must
agree that `playing == !Scene.paused`. Its one-based frame/count is bounded by
4096, count is immutable for the connection, and at most eight outcomes appear.
Finished means paused at the final frame. Source recorded epoch/sequence and
source paused state remain separate from the playback transport epoch and cursor
state; no iteration count, physics rate or real-time factor is invented.

Recorded outcomes contain only the fixed seven action names, `ok`, and a required
nullable model ID. World actions require null. Successful lifecycle actions
require a nonzero ID in the existing u32 render domain; refused lifecycle actions
may retain any nonzero requested u64 or null. A removed model need not remain in
the current catalog. Outcomes are annotations, never submitted commands or ACKs.
Unknown/duplicate/missing fields, contradictory status, writable replay catalog,
scan payload, unsolicited status and incomplete credit tuples fail closed.

Ready is sent once after validated Scene/catalog/status delivery to the coalesced
inbox. Existing priority Scene/catalog/status/ACK commits remain separate from an
outstanding normal credit; reset does not discard an already validated old Ready.
Disconnect retires scene, catalog and playback status and resets connection-scoped
credit/count state. No queue enlargement, arbitrary event dropping or request
retry is introduced.

## Rendering and controls

The full UI's World group exposes **Recording playback** in the existing lifecycle
panel position. It identifies read-only playback, original recorded epoch/time,
frame/count, original paused/running state, current cursor state and bounded
accepted/refused outcomes. Existing error and pending notices remain visible.
The scene tree and rendering use actual recorded visual IDs and the existing
single world-basis conversion. Unchanged layout updates retain entities/meshes;
spawn/remove layout changes use normal complete-scene replacement and retirement.
The existing scoped glTF loader consumes simulator-admitted captured asset URLs;
no historical URI is downloaded or rewritten by this viewer feature.

Play/Pause control only the playback cursor. Reset restarts at the recorded
initial source epoch/time zero while advancing the transport epoch; existing
exact Reset ACK epoch+1/paused/zero-time validation is unchanged. Play at EOF is
refused without consuming a request ID until Reset. All lifecycle methods,
including GetState, refuse in replay mode. Spawn/Read/Apply/Remove controls are
absent in the playback panel and remain disabled even if stale editable state is
present before the first status arrives. Recorded actions are never reissued.

## Regression source and pending evidence

New core controls cover strict DTO/outcome domains, exact status/Scene/catalog
binding before credit, read-only method refusal, immutable frame count, EOF,
reset priority ordering and connection retirement. Backend controls cover exact
opt-in query behavior, actual Session-to-ECS pose/layout/removal/restart behavior
with retained meshes for unchanged layout and one world basis, and fail-closed
missing-status/outgoing-Ready refusal. The UI control inspects real Text and
InteractionDisabled components; CLI controls retain default backend behavior.
Existing live test bodies, dependencies, quality gates and both maintained
14,000,000-byte release-bundle budgets are unchanged.

These are authored assertions, not results. Independent source review precedes
the single coordinated required verification/build batch. Actual file recording,
archive corruption/resource bounds and playback pacing belong to simulator
regressions; real short-session movement/spawn/remove/reset playback and actual
browser control observations remain acceptance requirements. No historical log
format parity, full sensor recording, seeking, arbitrary assets or deterministic
physics re-execution is claimed.
