<!-- Copyright (C) 2026 Open Source Robotics Foundation; SPDX-License-Identifier: Apache-2.0 -->
# M10 candidate: explicit world lifecycle editor

Source implementation is complete and awaiting independent review and the single
consolidated verification/build/browser pass. Nothing here claims M10 acceptance,
compiled API compatibility, coverage, bundle budgets or actual browser operations.
Accepted M1–M9 source, release bundles and evidence remain unchanged.

Use the primitive backend explicitly, full UI and the lifecycle opt-in:

`?backend=primitive&ui=full&lifecycle=1&url=ws://127.0.0.1:9010/`

Native application options are `--backend primitive --lifecycle --url URL --ui full`.
The simulator must separately enable its configured `--web-lifecycle` capability.
The **World lifecycle** sidebar panel is registered only for this selected viewer
mode. Without a current server catalog, editing remains unavailable. The generic
Gazebo editing/service route remains disabled for the primitive backend. Default
Gazebo and ordinary primitive clients keep their existing behavior.

The panel lists declared template labels and server model names/public IDs. Labels
are display text, never identity. Select a template and edit the spawn position
and xyzw quaternion, then **Spawn at edited pose**. No XML, user name, file path or
URI can be sent. To edit an existing model, select its catalog entry and **Read
selected state** before **Apply edited state**. The thirteen fields are world
Z-up origin position in metres, xyzw unit quaternion, world origin linear velocity
in m/s and world angular velocity in rad/s. New lifecycle state validates quaternion squared norm within the server’s 1e-12
unit domain (legacy Scene validation is unchanged). Wire values receive no viewer basis
conversion. Rendering keeps the existing single Z-up conversion and glTF local
basis correction. Fixed models require zero velocities; unsupported models and
bad numeric inputs remain server refusals. **Remove selected model** addresses
that model's opaque ID, never a guessed visual ID.

One pending slot and strictly increasing request allocator are shared with
pause/resume/reset. Buttons disable while pending. Neither a send nor a reply
optimistically inserts/removes or moves a render entity: only the committed full
Scene does so. Catalogs and replies are bound to that Scene's exact sequence,
world and epoch, including paused same-timestamp edits. State reads display their
actual committed sample stamp. Model selection rebinds the catalog's actual
visual IDs through `GzIndex` after scene replacement; duplicate names are safe.
Disconnect, reset/epoch replacement and capability loss clear catalog, state and
selection. Timeout/send failure is an unknown outcome, visibly reported; mutations
are never automatically replayed after reconnect. Inspect the fresh snapshot.

The version-1 lifecycle side messages are separate from unchanged Scene v1/v2,
control ACK and measured-scan messages. The WebSocket query appends lifecycle=1
only after the viewer opt-in and retains independent scan=1. Bounds remain 64 KiB
per message, 128 templates/models/aggregate visual IDs, 256-byte display/error
text and 4 KiB client requests. Visible catalog models have nonempty unique visual
ownership, with no world/model/visual ID alias. Templates are predeclared by the
server; immutable local glTF dependencies remain on the accepted guarded loader.
No extra loader, network worker, dependency or asset policy is added.

Source regressions cover typed ownership/numeric failures, required nullable reply
fields, same-timestamp sequence fences, shared pending requests and ID allocation,
non-optimistic refusal, unknown outcomes, epoch/reconnect clears, raw world state,
actual ECS selection rebinding and selected-state editing. Required normal hooks,
exact hosted checks, both unchanged 14,000,000-byte release budgets and actual
browser primitive/local-mesh spawn/remove/read/apply plus reconnect remain to run
only after complete cross-repository source review. No new parity, performance,
general articulated editing, remote resources or universal GPU claim is made.

## First normal hook boundary

The first complete-source normal commit hook stopped after 38.010 seconds with
no capacity stop or commit. Generic hooks and rustfmt passed. Native Clippy
reported an unnecessary qualification and a redundant clone in the new ECS test;
rustdoc additionally reached a panel marker missing the Default bound required by
the existing PanelDef API. The corrections only shorten the imported namespace,
move the last-use test value and derive Default on the zero-sized marker. Existing
assertions, protocol, scheduling and gates are unchanged. The corrected source is
awaiting independent review and the affected normal hook; runtime tests, coverage,
Wasm bundles and actual M10 browser operations remain unrun.

The affected normal hook then stopped after 25.005 seconds, again without a
capacity stop or commit. Rustdoc passed. The shortened method signature required
its normal formatter wrapping, and native Clippy found a default-then-field
assignment in a new numeric negative test. The correction uses the same explicit
State initializer; the identical source pattern in a new UI test was batched too.
All field values and assertions remain unchanged. Source formatting is a mutation,
not a passing check; this corrected batch remains unrun pending source review and
coordinated execution. Both actual failed packets remain separate.

The next normal hook reached the lifecycle panel and stopped after 46.010 seconds
on two unreachable-public-item lints. Its generic, formatter and rustdoc checks
passed; no Wasm/runtime or coverage result was reached. The panel ID and plugin
now have parent-module visibility, retaining the existing PanelPlugins use
without adding a public API. No behavior, assertion or gate changes accompany
this correction; it remains unrun pending independent source review. All three
failed attempts stay preserved with their actual outcomes.
