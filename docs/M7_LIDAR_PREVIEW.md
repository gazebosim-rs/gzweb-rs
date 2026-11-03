<!-- Copyright (C) 2026 Open Source Robotics Foundation -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# M7 measured planar scan preview

This source candidate extends the explicit primitive backend on accepted viewer
`5b67f45f`. Compilation, tests, coverage, both browser bundles and actual scan
visualization remain unrun. It does not qualify the simulator's lidar model.

The primitive backend opts into `scan=1` on its existing WebSocket URL, preserving
other query fields. An existing single `scan=1` is idempotent; conflicting or
repeated explicit scan flags are refused. The default Gazebo backend does not
request or parse this stream. M1/M2 world geometry and committed control messages
keep their existing versions and contracts; the bridge sends no scan frames to
legacy clients without this opt-in.

A separate version-1 `kind:"scan"` JSON message carries `world_id` (nonzero u32),
`epoch` and `sim_time_ns` (u64), plus `scan:null` to clear or one measured scan:

- `sensor_id` (nonzero u32), `acquired_ns` (u64, no later than the envelope time).
- `sensor_position` in world metres and `sensor_orientation` in world xyzw.
- `angle_min`, `angle_max`, `angle_step` in radians, ordered inclusive endpoints
  spanning at most one turn, with a consistent positive step. Step consistency
  is bounded against the span/count progression, not absolute angle offset;
  all generated angles must be representably strictly increasing, and the last
  beam uses the declared maximum exactly.
- `range_min`, `range_max` in metres and 2–128 ordered `ranges` entries. A finite
  entry must be inside the inclusive range limits; `null` means no return.

Rust deserializes integer times directly, including values beyond JavaScript's
exact floating-point integer range. One complete Scene must arrive before scans
for that world/epoch. Scan envelope time may be newer than the coalesced scene's
clock; receipt does not advance the control clock. Wrong-world/epoch and older
scan messages are dropped. Null clears retain the time fence, preventing an old
measurement from resurrecting output even under a newer envelope: a return
acquired at or before the null-clear time remains fenced out. Fresh acquisitions
must be later than that clear. Reset acknowledgement, new world/epoch
and disconnection clear the preview. Each message replaces the one pending scan;
there is no scan history or per-beam entity allocation.

An actual finite return is drawn as a line and endpoint cross. Its endpoint is
computed from the acquisition sensor pose and measured angle/range in f64, then
transformed by the existing `GzWorldRoot` exactly once. No-return beams draw
nothing; no maximum-range point or interpolated return is invented. SceneDesc,
Core IDs, physics objects, camera visual scenes, meshes and materials are not
used for scan graphics. The renderer iterates the bounded resource without an
endpoint heap allocation per frame. Sensor/time metadata remains in that resource;
this is a world-space geometric inspection, not the existing Sensors panel's
model/topic UI or a claim of intensities, multilayer or GPU lidar support.

Authored regression source covers malformed/count/time/pose/range admission,
exact u64 times, latest-only delivery, stale/reset/null/disconnect fences, preserved
world/pose identities, opt-in URLs and independent rotated acquisition-frame
expectations through the actual ECS world-root transform. The existing >95%
coverage gates, native/default-backend regressions, exact hosted CI and unchanged
14 MB gzip budget for each WebGPU/WebGL2 bundle remain mandatory. A real browser
scan observation is required after the completed feature passes source review;
no execution or acceptance is claimed by this document.
