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

The first normal pre-push coverage hook then stopped after 96.023 seconds without
publishing the review branch. The new ECS replacement control was the sole
emitted failure (393 other gz-bevy tests passed): it observed three mesh assets
where its immediate count expected one. Pinned Bevy collects dropped handles in
PreUpdate, while scene replacement drops the old Mesh3d components in Update.
The test now advances one ordinary application frame for that maintained
collection pass, retaining the exact original asset count and adding explicit
retired-ID absence/current-ID presence checks. No production scheduling or asset
policy changes. This source-supported ordering correction remains unrun pending
independent review and affected normal hooks; later workspace tests and final
coverage export were not reached. No review publication or hosted run occurred.


## First actual browser boundary and diagnostic correction

The qualified e276f55 candidate passed its normal local hooks, independent local
and official CI audits, and both unchanged release budgets. The first actual M10
WebGL2 browser attempt briefly displayed real geometry, then disconnected and
retried. The previous bare `Connecting to ...` log precedes backend selection and
query augmentation; it does not identify a Gazebo-backend selection failure.

Source inspection identifies a possible overload: each opted-in snapshot sends
Scene, catalog and scan messages, while the bounded browser queue retains 64
events and the ECS drains at most 64 per frame. A low frame rate can therefore
fill that queue. The observed close cause was not retained, so this mechanism is
not claimed as the experimentally established cause of the browser failure.

This isolated correction retains one bounded diagnostic slot outside that queue:
the first local oversize/type/queue failure and the latest close code plus at most
256 UTF-8 bytes of close reason. Failure still closes the socket with the same
bounds; no pacing, coalescing, queue capacity, wire or retry policy changes.
Primitive retirement logs the cause once for that driver, and the lifecycle panel
shows the existing connection error even when its catalog has been retired.
New source regressions cover full-queue terminal-delivery failure, disconnected
receiver retention, bounded multibyte reason storage, backend retirement and the
actual panel text. These corrections are unrun pending independent source review
and the affected mandatory verification; no new bundle or browser result is
claimed. Original qualified source, bundles, failure observations and audits
remain preserved.

## Opt-in publication credit (source candidate)

The qualified 74496593 diagnostic browser recorded the actual failure:
`event queue full; requested close 4002; WebSocket close code 1006`.
This confirms the local event-queue overflow; it does not establish a particular
GPU performance rate or a general rendering limit. Qualified e276/744 sources,
bundles and observations remain unchanged.

Lifecycle clients now explicitly negotiate `lifecycle=1&lifecycle_credit=1`.
The server sends one coherent normal Scene/catalog/optional scan followed by
`{"version":1,"kind":"publication_credit","token":N}`. The viewer validates
that fresh matching Scene/catalog and the strictly increasing nonzero u64 token,
then, only after validated inbox flush, sends exactly one
`{"version":1,"kind":"publication_ready","token":N}` using the existing
bounded driver. Unknown, missing, duplicate, null, fractional and stale fields or
tokens fail closed. Failed flush or disconnect suppresses Ready; a failed send
closes without retry. Queue capacity 64, per-frame drain 64, frame bytes 64 KiB,
command IDs, eight request slots and both 14 MB release budgets are unchanged.

Credit is transport state, separate from world/epoch/control state. A successful
same-socket reset or higher-priority committed lifecycle reply can supersede the
scene while the older validated Ready is waiting for flush: that old Ready must
still be sent, once. It does not retry or acknowledge a mutation. Ordinary control
and exact committed lifecycle reply packets bypass the producer credit gate;
Ready must also be accepted while such a command is pending. A disconnect/new
socket resets credit tracking independently. No physics, simulation or GPU step
waits for Ready. Default Gazebo and non-lifecycle primitive clients do not
negotiate credit and keep their existing protocols.

New regression source covers strict query/metadata negatives, fresh-tuple and
single-flight admission, command-ID independence, old credit across reset and
priority commits, malformed-tuple suppression, failed bounded send, and a real
bounded native WebSocket with delayed event consumption across paused/advancing
snapshots. That controlled socket fixture proves viewer transport behavior, not
the production simulator or a browser rate. The complete cross-repository source
batch and independent review must precede required verification. No credit
candidate checks, bundles, runtime/browser success or M10 acceptance are claimed.


### First publication-credit verification boundary

The complete credit candidate's normal commit hook passed in 79.016 seconds.
Its first normal pre-push coverage hook then stopped after 95.023 seconds without
a capacity stop or publication. The first target reported 400 passing tests and
one failure: the new reset/old-Ready control expected the UI pending flag to clear
immediately after reset ACK, before providing the fresh reset-epoch Scene.
The old Ready and empty-scene retirement assertions had already passed. The
existing backend intentionally holds that UI fence until a current scene arrives;
the command itself is already acknowledged. The other three new backend credit
controls, including the real bounded socket, passed. Later workspace targets and
final coverage export were not reached.

The test-only correction keeps every previous assertion and supplies the missing
fresh epoch-one Scene/catalog and next credit before checking that UI controls
unlock. It additionally checks the acknowledged-command versus waiting-for-scene
distinction and the next Ready's exact-once delivery. No production, timing bound,
assertion threshold, scheduling, protocol or gate changes. This completed source
correction remains unrun pending independent review and affected normal hooks;
there is no new build, hosted or browser success claim.
