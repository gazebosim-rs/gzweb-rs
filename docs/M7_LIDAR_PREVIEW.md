<!-- Copyright (C) 2026 Open Source Robotics Foundation -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# M7 measured planar scan preview

M7 was accepted and published on 2026-11-04 at viewer
`19be842d3c42c2f7b2c827e8fdd54c8603114acc` and simulator
`9a652063d85938143ac298f428b55dd68a7f22da`. The normal local hooks,
[exact viewer CI 41748284641](https://github.com/gazebosim-rs/gzweb-rs/actions/runs/41748284641),
[exact simulator CI 41747544467](https://github.com/gazebosim-rs/gz-sim-rs/actions/runs/41747544467)
and independent source, local/official data and actual browser reviews passed
before Main promotion. See the simulator's
[canonical M7 acceptance record](https://github.com/gazebosim-rs/gz-sim-rs/blob/main/docs/M7_ACCEPTANCE.md)
for the lidar model, sensor/transport tests and complete delivery limits.
This documentation closeout receives its own normal hooks and exact CI; it
changes no runtime, tests, dependencies or accepted bundle bytes. M8 is the next
bounded simulator delivery.

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

Passing regression controls cover malformed/count/time/pose/range admission,
exact u64 times, latest-only delivery, stale/reset/null/disconnect fences,
preserved world/pose identities, opt-in URLs and independent rotated acquisition
frame expectations through the actual ECS world-root transform. Existing default
Gazebo and M1/M2 contracts, assertions, Cargo graph and quality gates are retained.

## Accepted viewer evidence

The normal pre-commit passed in 87.019694 seconds and the normal pre-push
coverage/review push passed in 185.060307 seconds, without capacity stops.
Retained local LCOV has 223 unique source records and **54,686 / 56,247** lines
(97.224741%). The successful local hook log does not retain an exact Cargo test
total; hosted counts are not substituted for that gap.

Exact viewer CI 41748284641 attempt 1 passed all six jobs and 66 steps, with only
the unchanged optional Fuel cache-save step skipped. Actual hosted results are
**1,083 passing native tests**, zero failures and five unchanged ignored tests,
plus three separate Fuel controls and one Node startup control. All seven
protocol/snapshot corpus pairs passed with 25 existing allowlisted differences.
The allowlist and gates are unchanged.

Official LCOV has the same 223 source records and **54,685 / 56,247** lines
(97.222963%): one fewer hit than the local run. The separate counts are preserved;
there is no profile union or invented equality requirement. Independent audit
verified all 692 official Git blobs and executable modes and the ten local build
source bindings. Official artifacts retain LCOV and parity results, without raw
coverage profiles or coverage executables.

The single maintained two-flavor release build passed in 1710.407249 seconds
without a guard stop. Independently recomputed gzip payloads were:

| Local release bundle | Raw WASM bytes | gzip bytes | Unchanged budget |
| --- | ---: | ---: | ---: |
| WebGPU | 47,820,672 | 12,892,886 | 14,000,000 |
| WebGL2 | 47,726,029 | 12,868,306 | 14,000,000 |

The exact WASM SHA-256 values are
`7d59eae03227bfabe33840341228cb9724891cd59d910864011cf2218f157b9e`
(WebGPU) and
`3a9b8069bd04a5e60d08a35aae63f9de26b64ed0106affe025e4066102ab2fb9`
(WebGL2). Source/bindgen/build records and copied preview WASM/JS bytes were
independently reviewed. Hosted Wasm checks debug compilation; the release-bundle
budgets and copies are separate local evidence. Accepted M1/M2 served files and
historical build evidence were preserved.

## Actual scan inspection and limits

The new-only preview used the existing HTTP server on port 8081 and the qualified
native-free simulator on port 9007:

`http://127.0.0.1:8081/artifacts/m7-preview/?backend=primitive&ui=full&url=ws://127.0.0.1:9007/`

The actual browser used WebGL2 on ANGLE/NVIDIA RTX2070. Images show two actual
visual entities and three measured rays with wall-contact markers. Pause
acknowledged epoch 0 at 46.361 seconds and kept the last fan visible. Reset
acknowledged epoch 1 at zero, paused, and cleared every measured ray/marker while
retaining geometry. Resume restored advancing time and the fan; the saved resumed
image is at 38.743 seconds (the earlier thread-only 20.791-second observation is
separate). The final review view was paused in epoch 1 at 42.539 seconds.

Five saved images and exact captions passed independent review alongside the
retained console record. The qualified CLI, fixture and served GL bytes have
separate reviewed hash bindings; the browser manifest does not bind a console
hash.
The first non-PTY backend exited cleanly before attachment; a persistent-PTY
relaunch of the same qualified executable enabled the observation, without a
rebuild or repeated test qualification. Screenshots prove this adapter's actual
scan/control lifecycle; exact range, frame, cadence and canonical wire semantics
are established by the separate passing simulator controls.

No WebGPU runtime observation, universal physical-GPU guarantee, browser
performance result, C++ lidar parity, intensity model, multilayer/GPU lidar or
other sensor family is claimed. The existing Sensors panel's model/topic UI is
not part of this measured world-space overlay. This closeout does not manually replay
accepted M1–M7 runtime/browser qualification; ordinary mandatory CI checks run
unchanged. It does not advance M8 acceptance.
