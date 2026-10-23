# Phase 5 — UI implementation plan

Plan for building the Gazebo UI spec (`design/ui/`: DESIGN.md v0.3, tokens, icons, shortcuts,
boards in `design/ui/assets/boards/`) into
gzweb-rs, with a hard requirement that gzweb-rs also runs **without** the UI: just the 3D
view, an orbit camera, and keyboard camera bindings.

Status: approved 2026-10-19. Decisions (also logged in `design/ui/DECISIONS.md`):
- Desktop gz-transport connection (spec Q7): **later**. Both targets use the websocket server,
  behind a `SimBackend` trait.
- Target Gazebo release: **Jetty** (gz-sim 10). See §3a.
- Default UI mode: **`full` on desktop, `minimal` on the web**.
- Visual references: the exported boards in `design/ui/assets/boards/{light,dark}/`.

---

## 1. Where we start

| Spec assumes | gzweb-rs today |
|---|---|
| A new app `gz_ui` with `ui/`, `sim/`, `state/` | Workspace: `gz-proto`, `gz-core` (protocol, scene model), `gz-bevy` (scene, render, camera, HUD), `gzweb` (binary + wasm), `gz-parity`, `gz-collada` |
| `SimBackend` trait, native = gz-transport (ZeroMQ), web = websockets (Q7) | One backend: the Gazebo websocket server, on native and wasm (`gz-core::transport`, `gz-bevy::connection`) |
| Multi-viewport, per-view cameras | One `MainCamera`, full window, quality presets (TAA, SSAO, atmosphere, IBL) |
| Navigate input map §6.2, view cube, rail | `camera.rs`: orbit/pan/zoom about a picked point; `hud.rs`: FPS/status text, Space = play/pause, N = step |
| Editing (translate/rotate, inspector, undo, save) | Read-only viewer. The websocket `req` op already reaches any Gazebo service (`set_pose`, `create`, `remove`, `generate_world_sdf`, …) |
| Bevy 0.20 headless widgets | `bevy_ui_widgets` 0.20 is available: button, checkbox, radio, slider, menu, popover, dialog, modal, tabs, list, scrollarea, scrollbar, **text_input**. No need for a third-party UI crate. |

`GzConfig.interactive` already splits "window with controls" from "snapshot/screenshot".
That flag becomes the UI-mode switch below.

---

## 2. Running with and without the UI

### 2.1 Three modes

| Mode | What runs | Where it's used |
|---|---|---|
| `none` | Scene, rendering, connection. No input, no UI. | `--snapshot`, `--screenshot`, parity tests, thumbnails |
| `minimal` | `none` + **`ViewerControls`**: the Navigate camera (orbit/pan/zoom, pick-point pivot), viewport-scope keys from `shortcuts.json` (F, H, 5, 7/1/3 ± Shift, G, Space tap play/pause, ⌘→ step), WASD/arrow/QE camera fly, and a one-line status text that can be turned off | Embedding in other pages, kiosks, low-end machines, "just look at the sim" |
| `full` | `minimal` + **`GzUiPlugin`**: the spec's workbench (top bar, activity bar, sidebar, inspector, status bar, split views, tools, panels, menus, ⌘K) | The default desktop and web app |

Selecting the mode:
- **Native:** `--ui full|minimal`, default `full`. `--snapshot`/`--screenshot` force `none`.
- **Wasm:** `?ui=full|minimal`, default `minimal`. The embedding API (§2.3 of PORT_PLAN) takes
  the same option.

### 2.2 Crate layout

```
gz-bevy   (exists)  scene, render, connection, assets — no UI, no input
  └─ gz-bevy::view  (new module) camera controller + input map shared by minimal and full
gz-ui     (new crate) GzUiPlugin: theme, icons, fonts, widgets, shell, panels, tools, state
                    (its layout and conventions: §7)
gzweb     (exists)  binary/wasm entry: picks the mode and adds the plugins
```

Rules that keep `minimal` real rather than a stripped-down `full`:
- **The camera lives in `gz-bevy::view`, not `gz-ui`.** It is a `ViewCamera` component with a
  state machine (drag action, projection, mode, pivot, animated snaps) driven by **messages**
  (`ViewCommand::{Orbit, Pan, Zoom, Snap(face), Focus, Home, SetProjection, …}`).
  `minimal` produces those messages from raw mouse and keys. `full` produces the same messages
  from the rail, the cube and the same key map, scoped to the hovered/active view.
- **One input map**, loaded from `shortcuts.json` (embedded at build time), shared by both modes.
  `minimal` enables the `global`, `viewport` and `minimal` scopes; of the global bindings only
  `sim.*` have actions there (DECISIONS.md D-16: no reset). `full` enables everything. The
  CI conflict test (spec §13) runs over the one table.
- **`gz-ui` never reaches into rendering.** It reads `gz-bevy` state (`GzIndex`, `GzStatus`,
  scene entities) and writes messages: `SetTool`, `Select`, `SetPose`, `PlayPause`, `Step(n)`,
  `ResetWorld`, `OpenPanel`, …. Only `gz-bevy::connection` talks to the server.
- **Pointer routing:** in `full`, camera input applies only when the pointer is over a view's
  placeholder node and not over a UI node (rail, cube, popover). `bevy_picking`'s UI hit test
  decides this. `minimal` has no UI nodes, so everything goes to the camera.
- **Per-mode tests:** the headless test app builds `none` (as today), a `minimal` app test drives
  keys and mouse through `ViewerControls`, and `full` has UI layout tests (bevy_ui layout runs
  without a GPU) plus screenshot checks.

### 2.3 Minimal-mode key map

From `shortcuts.json`, viewport and sim scopes only:

| Key | Action |
|---|---|
| Left drag / right drag / middle drag | orbit / pan / zoom (pivot = point under cursor at drag start) |
| Wheel | zoom about the point at the view centre (D-11) |
| Ctrl + wheel, pinch | zoom to cursor |
| Two-finger scroll | pan where the platform tells it from a wheel (D-18) |
| Space hold + drag | temporary pan |
| Space tap | play/pause |
| ⌘→ | step |
| F / H | focus picked entity / home |
| 5 | perspective ↔ ortho |
| 7 / 1 / 3 (Shift = reverse) | top / front / right views |
| G | grid |
| W A S D / arrows, Q / E | move the camera (minimal only: in `full` these keys are teleop's while it is armed) |

---

## 3. Spec items that need a decision or a change

1. **Native backend, Q7: deferred.** The `SimBackend` trait comes with the websocket
   implementation behind it. A Rust gz-transport client is a separate later track.
2. **Target release: Jetty** (spec open Q2). See §3a.
3. **Console panel.** Gazebo doesn't publish its log on a topic, and the websocket server doesn't
   forward it. **Proposed:** the console shows client-side messages (connection, asset failures,
   service errors, our `warn!`s) and `/rosout`-style topics when present. Log this in
   DECISIONS.md.
4. **Editing features** (tools, inspector edits, Assets insert, undo, Save, model editing) go
   through Gazebo services over the websocket `req` op. Each needs its service verified on the
   Harmonic websocket server (`/world/<w>/set_pose`, `/create`, `/remove`, `/generate_world_sdf`,
   `/world/<w>/control`, light/visual config). Model editing (§6.6) is the largest of these; it
   comes last.
5. **Multi-view cost.** Each view is a full `Camera3d` with our quality stack. Quad view at
   Ultra means 4× TAA/SSAO/shadow cascades. **Measured in M6b** (DECISIONS.md D-95): an extra
   view costs about 0.5 ms at Low and 1.0 at High (RTX 5090, 1440 × 900); the largest part of
   High's is the environment light refiltered every frame per camera; SSAO, bloom and TAA
   measure as nothing. Every view keeps the full stack; measure a GPU-bound target before
   cutting.
6. **Spec location.** `design/ui/` (DESIGN.md, DECISIONS.md, tokens.json, icons.json,
   shortcuts.json, theme.rs, brand assets, boards). Agents and CI read this one copy. Decisions
   we make go into its `DECISIONS.md`, as the spec asks.
7. **Fonts.** The Geist and Geist Mono TTFs (OFL-1.1) aren't in the zip. They get vendored from the
   upstream release, with the licence file, and embedded with `include_bytes!` for wasm.
8. **Icons.** `icons.json` holds 89 SVG path icons. A `build.rs` in `gz-ui` rasterizes them with
   `resvg` into an atlas (1× and 2×, at 15/17/18/20 px and the other drawn sizes, D-44) and generates the `IconId` enum. Icons
   are tinted at runtime by `ColorRole`. Build-time rasterizing keeps `resvg` out of the wasm
   payload.

---

## 3a. Jetty migration track

Jetty (gz-sim 10) deprecates gz-launch. The websocket server is now the gz-sim system
`gz-sim-websocket-server-system`, added in the world SDF or at runtime through
`/world/<w>/system/add`. Our Docker images, corpus and protos are all Harmonic today. This track
runs alongside the UI milestones and must land before M4, which builds on server behaviour:

- **J1, image:** `Dockerfile.gazebo` moves to Jetty on Ubuntu 24.04. It starts gz-sim with the
  websocket system, through a `-s` flag or the `system/add` service, so arbitrary worlds still
  work without editing them.
- **J2, protos:** update `gz-proto` to Jetty's gz-msgs and check for wire changes the client cares
  about (Scene, Material/PBR, Light, Heightmap, WorldStatistics).
- **J3, protocol:** diff the gz-sim websocket system against gz-launch's (ops, `asset` handling,
  auth, throttling, message framing, the out-of-bounds bugs noted in DESIGN.md). Fix the
  client and the replay server where they differ.
- **J4, corpus:** re-record `tests/corpus/*` against Jetty, and update the gzweb browser parity
  job if gzweb needs changes to run against it. Keep the Harmonic corpus only if we decide to
  support both.

## 4. Milestones

These follow the spec's §12 order, adapted to this repo. Each milestone is one PR, ends green on
`scripts/check.sh` plus the parity suite, and includes light and dark screenshots at 1440×900.

### M0 — Split the camera out and add UI modes — **done 2026-10-19**

Visible changes: the new keys (5, 7/1/3, F, H, G, Space, ⌘→, fly keys), the wheel zooming
about the view centre (Ctrl+wheel and pinch to the cursor), N no longer stepping, the HUD hint,
and orthographic views. The browser input items of M10 moved here (see M10).
- `gz-bevy::view`: move `camera.rs` orbit/pan/zoom into `ViewCamera` + `ViewCommand`. Add
  projection toggle, snap views with a 250 ms animation, focus/home, Space tap/hold, Ctrl+wheel
  zoom to cursor, WASD/QE fly.
- `UiMode { None, Minimal, Full }` replaces `GzConfig.interactive`. Wire `--ui` and `?ui=`.
  `Full` is temporarily the same as `Minimal`.
- Load `shortcuts.json` into an `InputMap` resource. Add the CI conflict test.
- Tests: unit tests for every `ViewCommand`; a headless `minimal` app driven by synthetic input.

### M1 — Theme, icons, fonts, widgets (`gz-ui` crate) — **done 2026-10-19**
- Generate `theme.rs` from `tokens.json` in `build.rs`, or keep the spec's
  hand-written mirror plus a test that it matches. Add the WCAG contrast test over the §13 pairs.
- Icon atlas build step, fonts, `ColorRole` + `apply_theme_system` + `button_visuals_system`.
- Widgets §5.1–5.16 on `bevy_ui_widgets`: IconButton, TextButton, Button, SegmentedGroup,
  SectionHeader, TreeRow, NumberField (text_input + expression parser + scrub), Toggle, Select,
  Slider, Chip, Badge, Menu/MenuItem, Tooltip, Popover, Splitter.
- A `--ui gallery` debug screen. Accept: both themes render, and runtime theme switch re-colours
  everything.
- Notes: `build.rs` generates the token tables and the icon atlases (DECISIONS.md D-21, D-24);
  fonts are Geist 1.8.1, embedded (+0.9 MB gzipped to the wasm with the atlases and logos).
  Text input is Bevy's (selection, clipboard, IME) behind the NumberField wrapper (D-27); no
  undo or ellipsis yet. Text fields stand the viewer's keys down and, in the browser, stop
  the canvas preventing defaults. The gallery shows one theme at a time (D-28). Bevy's text
  looks lighter than the boards' (linear-space glyph blending). See WORKLOG.

### M2 — Shell layout — **done 2026-10-19**
- Top bar, activity bar, sidebar with two slots, viewport area, inspector, status bar, splitters,
  collapse rules, minimum-size and floor rules (§2.1). Panels are empty shells with their
  empty/loading/error states.
- The single 3D view renders into the viewport-area node (a texture of the node's physical
  size that the node shows, resized with it in the same frame, scale-factor aware;
  DECISIONS.md D-31). The no-world card and the disconnected banner (§7.10) are driven by
  `GzStatus`.
- Notes: `gz_ui::shell` (`ShellPlugin`, `ShellLayout`, `UiCommand`). The view renders into a
  texture the size of the view node instead of a `Camera::viewport` (DECISIONS.md D-31: Bevy
  0.20's TAA ignores viewports), placed after UI layout in the same frame. Pointer routing
  uses bevy_picking's hover map over pass-through containers (D-32). One connect card on both
  targets (D-33); `GzReconnect` added to `gz-bevy::connection`. The transport (reset,
  play/pause, step, steps per click), clock, RTF and status readouts are wired to the existing
  stats, so part of M4's transport and status work is done; the World menu, Snap popover,
  search and workspace button open nothing yet. Settings switches the theme until ⌘K (D-35).
  `--ui full --screenshot`, `--window WxH`, `--capture-at-ms`. PR #29 review fixes: focus
  (D-52), start-up rules on the first real size (D-53), steps per click everywhere with a
  Custom field (D-54), server address rules (D-55), readouts on change (D-56). See WORKLOG.
- Restructured after M2 (DECISIONS.md D-51): feature folders, panels and inspectors as
  plugins in registries, shared layout primitives, a small public API and tests next to
  their modules. Later milestones follow §7.
- Panel contract after the PR #30 second review (D-58): panels write their views into
  `PanelStatus`, define their own actions and header actions, build their content in
  their own observer; slots keep bodies across a second slot and a swap; Tab walks the
  regions.

### M3 — Viewport controls — **done 2026-10-20**
- Rail (Orbit/Pan/Zoom radio, projection, camera mode, focus, overlays, layout), compact rail,
  view cube (render-to-texture camera + picking on the image), scale label in ortho, target chip.
  The rail's controls are tab stops inside the viewport area's tab group (D-58); the rail adds
  no `TabGroup` of its own (§7, Focus: a nested group would jump the Tab order).
- Overlays menu wired to existing toggles: grid, collisions, wireframe (line-mesh fallback on
  wasm), joints, frames, inertia/CoM.
- Camera menu: Free / Follow / Track (the gzweb camera modes from PORT_PLAN Phase 5).
- Notes: `gz_ui::shell::viewport::{rail, view_cube, hud}` on the view's node (D-59); they send
  `ViewCommand`s and `OverlayCommand`s and show `gz-bevy`'s state. Only the grid existed, so
  `gz_bevy::overlays` is new: collisions, inertials and joints come from the world's
  `generate_world_sdf` (`gz_core::sdf`), drawn as Gazebo does; wireframe is `WireframePlugin`
  natively and line meshes in the browser (D-62). Camera modes in `gz_bevy::view::modes`:
  Follow moves with the target, Track turns to it; ⇧F/⇧T take the selection, else the model
  under the pointer, so they work before M5; the menu's Follow/Track wait for the selection
  (D-60). The cube picks faces (snap, ortho), edges and corners (look from, keeping the
  projection), lights its hover, orbits on a drag (D-61). The scale label has a bar (D-63).
  `--view` / `--overlays` start a capture in an axis view with overlays (D-64). Left for
  M5/M6/M7: "Look through" (sensor cameras of the selection), the split layouts, Maximize,
  Close view, Link cameras, Shared selection and Apply to all views (disabled, "Not available
  yet"), lidar/sensor data, the inactive view's face marker, per-view overlays. Found: a
  short window with the inspector open grows the shell past the window (DECISIONS.md, Open).

### M4 — Backend, scene sync, transport and status — **done 2026-10-20**
- `SimBackend` trait with the websocket implementation (see §3 item 1).
- Top-bar transport: play/pause/step/steps-per-click/reset, sim clock, RTF from `/stats`.
  Status bar: connection, engine/step, fps, iterations, messages chip, tool hint, selection,
  cursor position.
- Scene panel: entity tree from `GzIndex`, virtualized, with filter, chips and visibility toggles.
  Spawn and delete update it live; this needs `scene/deletion` handling, which PORT_PLAN Phase 1
  left out.
- Scene panel on the contract (D-58): `.content::<SceneTree>()` and an observer on
  `Add<SceneTree>` that builds rows from panel-owned resources (expansion, filter text, chip,
  scroll offset), since a body is respawned on every view change; Expand all / Collapse all
  (header actions, defined and disabled now) enabled and handled in its
  `PanelActionActivated` observer. Systems that only feed the shown tree run under
  `panel_shown(SCENE)`.
- `tree_row` recyclable (PR #30 second review, item 7): a `TreeRowSpec` change must update
  the name text, letter, indent, caret and AccessKit label too (today only the badge, weight,
  eye and opacity), so a virtualized row can be reused. The eye becomes controlled: it emits
  an event and the panel (and M5's undo) owns visibility, instead of `on_eye_activate`
  toggling `spec.hidden` itself (the feathers controlled pattern).
- Notes: `gz_bevy::backend::SimBackend` with `WebsocketBackend` behind the message API
  (D-65); `scene/deletion` in the interactive modes, `apply_deletions` before
  `apply_scene`; sensors on links (`GzSensors`); `GzIndex` changes only with the
  structure; `SdfWanted`. Header actions' enabled state at run time through
  `PanelStatus` (`set_action_enabled`, its own commit for PR #33). The Scene panel
  (`panels/scene/{model,view,systems}`): the tree of world, models, links, lights,
  sensors and joints (from the SDFormat), virtualized over a `ScrollArea` with a
  recycled pool, filter field and controlled type chips, Expand/Collapse all,
  visibility on the scene entity, one tab stop with arrow keys (D-66, D-67). The
  status bar's message chip from the client's warnings and errors
  (`gz_bevy::messages`, D-68); the transport and the other readouts were done in M2
  (checked against §3.1/§3.6; engine reads "—", the server doesn't say; "sel: —" until
  M5). The shell's root has min sizes of 0, which fixes M3's short-window open item
  (D-69). `gzweb --scene-filter`, `--scene-expand` (D-70). Verified live against
  Jetty (`/create`, `/remove`). Left: selection, Hide others/Isolate, row context
  menu and drag to re-parent (M5); the message tray (M8); arrow keys in the other
  groups (M10).

### M5 — Selection, inspector, transform tools, undo
In four PRs: **M5a** selection, **M5b** inspectors, **M5c** transform tools (with the patched
websocket server, DECISIONS.md D-76), **M5d** undo.

- **M5a notes:** views select top-level models by click (3 px slop), Shift adds, ⌘/Ctrl toggles,
  and the Box select tool or ⌘/Ctrl+drag draws a marquee (D-71, D-72); the tree selects rows,
  Shift ranges, ⌘/Ctrl toggles, the world row clears, and selected rows show it (D-73); "sel:"
  in the status bar; Hide others and Isolate (D-74); the outline: a mask camera and a UI edge
  shader, 2 px for the selection, 1 px at 50% for the tree's hover (D-75); `gzweb --select`
  (D-64b). Follow/Track, focus and snaps use the selection as they were built to. Left for
  M5b: joint and sensor rows selecting, with their inspectors.
- **M5b notes:** the inspector frame shows a subject (D-77); Model, Link, Light, Joint, Sensor
  and multi-selection inspectors with live poses and joint states and definitions from the
  SDFormat, read-only until M5c (D-78, D-80, D-81); joints and sensors select as parts (D-79).
  Left: editable fields and the joint command slider (M5c), sub-views and All components
  (D-82). Workspace 631 → 646 passing; coverage 98.4% lines. check.sh passes.
- **M5c notes:** the image ships the patched websocket server and clients probe for it (D-83);
  Translate and Rotate gizmos with axis constraints, snapping and Esc (D-84), sending `set_pose`
  and holding the sim's poses meanwhile (D-85); the tools are off against a stock server (D-86);
  the inspector's pose edits movable models (D-87). Left: the joint command slider
  (D-88), Duplicate and Delete (M8), undo (M5d).
- **M5d notes:** the undo stack (D-89): gizmo drags, pose commits (a scrub is one edit), eyes,
  Hide, Hide others and Isolate, by scene path; ⌘Z and ⇧⌘Z; moves go back through `set_pose`;
  the world button's unsaved dot follows the dirty state; a new world clears the stack. Reset
  isn't undoable (the server can't put a world back) and forgets the moves. Left: the World
  menu's Undo and Redo items and Save (M8), Delete's chip (M8).
- One shared `Selection`: click/Shift/⌘ in views and tree, box select (B), hover highlight, and a
  selection outline (second render layer).
- Inspectors: World, Model, Link, Joint, Light, Sensor, multi-select. Live-value streaming and
  the freeze-on-focus rule. All components view.
- Translate/Rotate gizmos with snap and X/Y/Z constraints. Edits go through `set_pose` and the
  other services.
- `UndoStack` and dirty-state rules (§7.1). Delete's undo chip (M8); Reset isn't undoable (D-89).
- Inspector registry for real (PR #30 second review, item 8): `InspectorDef` says which
  selection kind it handles (`for_selection()` returns World today, and `BodyFn` sees no
  entity); the frame rebuilds its body when `Selection` changes; bodies are built like panel
  content (D-58: a marker per inspector and its own observer on `Add<Marker>`, state in
  resources).

### M6 — Split viewports
In three PRs: **M6a** several views, **M6b** inactive views, per-view overlays and the
quality policy, **M6c** one splitter.

- **M6a notes:** each view a camera with a `ViewId`, its own placement, outline mask and render
  layer, new ones copying the first and dressed for the environment (D-90); the pointer drives
  the hovered view and keys the hovered, else the active one, the gizmo in the active view, the
  accent frame on it (D-91); the five presets as a flex grid with share dividers (drag, reset,
  accent), the Layout popover, Maximize ⇧Space, Close view ⌘W, splits ⌥⌘→/↓, the floor rule
  for split layouts, `gzweb --layout` (D-92); a rail, cube, pills and outline per view. Left
  for M6b: the inactive face marker and hover reveal, per-view overlays and Apply to all views,
  Link cameras, Shared selection, and the quality policy with its measurements.
- **M6b notes:** per-view overlays on a render layer each, the rail's menu and badge per view,
  Apply to all views, wireframes as line meshes everywhere (D-93); Link cameras, Shared
  selection on (D-94); the inactive view's face marker and 100 ms hover reveal, every view at
  full quality after measuring, the cascades spanning every showing view, `gzweb --no-vsync`
  (D-95).
- **M6c notes:** one splitter, the §5.18 widget, controlled: its owner keeps the size and range
  in it and applies `SplitterMoved` and `SplitterReset`; the side and slot splitters, the view
  dividers and the gallery own one each, so all are tab stops with arrows, Home and End; the
  slot splitter's double-click resets the split (D-96).
- Layout presets, active/hovered view rules, inactive-view face marker, per-view `ViewState`,
  floor and fallback rules, the multi-view quality policy (§3 item 5).
- One splitter (PR #30 second review, item 9): done in M6c (D-96).

### M7 — Panels
- Assets (shapes and lights via `/create`; Fuel search and insert), Topics (list, echo, publish),
  Plot (charts per §3.3 rules), Sensors (camera image via the websocket `image` op; lidar
  top-down), Teleop (keyboard, sliders, gamepad via gilrs; joints; X e-stop), Console (§3 item 3).
- Pop-out windows (§7.11). Generalize the sidebar's two hard-wired slots (PR #30 second
  review, item 13: `rebuild_slots` walks the two `SlotView`s, `slot_counts` finds a count
  through its `PanelHeader`, `ShellLayout::is_active` knows only the two slots) to a
  `PanelHost { panel }` component on a slot or a window, rebuilt per host; bodies already
  move between hosts (D-58).
- The panels' §3.3.1 states from their data (D-58: each writes its `PanelView`): Topics
  Ready once the client keeps the server's topic list (today `gz-bevy` only logs it), and
  "Topic ended" for a dropped echo; Sensors "Select a model with sensors" vs "No sensors in
  this world" (needs the world's sensors in the scene model and M5's selection); Assets
  "Fuel is unreachable" + Retry (a `PanelAction` it handles); Plot's Add field… enabled.
  Topics' Publish and Services header actions enabled.
- Footers (PR #33 review, N10): §3.3's optional footer has no place in the contract yet, so
  Plot's (Add field…, Export CSV, Pop out) could only live in its ready content and its empty
  state would lose it. Decide before Plot: likely `PanelDef::footer(..)` with `PanelAction`s
  drawn by the shell under every state (equal-width buttons, §3.3), their enabled state in
  `PanelState` like header actions; Console's footer is a command input, so also a footer
  content marker, built by the panel's observer like `.content::<Marker>()`.
- Gating (PR #30 second review, item 6): a topic echo's and a camera's `image`
  subscriptions run only while their panel is shown (`panel_shown(id)` on the systems that
  subscribe, plus unsubscribing when it hides); Plot's series and the Console's log keep
  recording while closed, so their data lives in always-on resources and only the view is
  gated.

### M8 — Menus, command palette, workspaces, persistence
- World menu (Save via `generate_world_sdf`, Save as, Open/Import where the server allows,
  Export screenshot), entity context menu with Align, ⌘K over every action and panel, Snap
  popover, message tray, workspace switcher.
- Persistence: desktop config dir `gz/ui/workspaces.json`; web `localStorage`.
- Command registry (PR #30 second review, item 11): `shortcut_keys` matches binding strings
  inline (`commands.rs`) and `UiCommand` is a closed enum without labels. ⌘K "over every
  action and panel" needs a registry of commands (id, label, binding, dispatch) that panels
  and their actions (`PanelAction`, `HeaderAction`, D-58) register into; panels are already
  enumerable through `PanelRegistry::iter()`.
- Ids and persistence (PR #30 second review, item 12): `PanelId(&'static str)` can't be
  built from a loaded string. Add `PanelRegistry::by_name` (and the same for inspectors) and
  restore a saved `ShellLayout` through it (Reflect-deserializing `&'static str` ids won't
  work). Unknown ids already fall back to the first registered panel in
  `apply_ui_commands` (`PanelRegistry::resolve`); restoring uses the same.

### M9 — Model editing (§6.6)
- Sim pause, dimming, EDITING pill, Parts tab, link/joint/visual/collision/sensor insertion,
  validation, write-back.

### M10 — Hardening
- Filter a static environment map once (DECISIONS.md D-95): Bevy's
  `GeneratedEnvironmentMapLight` refilters the studio or cubemap sky every frame for every view
  camera; prefilter it once into an `EnvironmentMapLight` (the atmosphere's stays real-time).
- AccessKit names and focus order (§10), UI scale 80–200%, reduce-motion. The regions are
  tab groups in §6.5's order already (D-58); M10 adds arrow keys within groups and the
  viewport's own tab stop (the D-52 open item, a Tab-focused button also taking the global
  Space chord, is resolved: D-58a). A collapsed panel's widgets (its splitter among them) are
  `Display::None` but still in the tab order: hide them too (D-96).
- Moved to M0 (done there, `gz-bevy::view::web`): context-menu suppression on the canvas,
  `web_reserved`/`web_alt`, `preventDefault` only for the listed chords while the canvas has
  focus, cancelling Ctrl+wheel/pinch page zoom (pinch zooms the view), and `?hud=0` removing
  the page's status lines. Text-field focus inside the canvas was done in M1 (bevy_ui text
  inputs keep the browser's chords). M10 still owns layout-aware letters.
- Performance: 5,000-entity tree at 60 fps, wasm payload budget.
- The §13 checklist is the PR's acceptance list.

---

## 5. Testing approach

- **Logic:** unit tests for state machines (camera, tools, selection, undo, layout floor rules,
  Esc precedence, Space tap vs hold, live-field freeze).
- **Layout:** headless bevy_ui layout tests that assert node rects for the shell at 1440×900,
  1280, 1100 and 1024 widths (collapse rules), and splitter drags.
- **Spec tests from the spec's own data:** WCAG contrast from `tokens.json`; the `shortcuts.json`
  conflict and `web_alt` test; a "no colour literals in `gz-ui/src` outside `theme.rs`" grep.
- **Integration:** existing headless + parity tests stay green in `none` mode. New
  synthetic-world tests drive `full`-mode actions (play/pause round trip, select → inspector,
  inspector pose edit → `set_pose` request seen by the replay server).
- **Visual:** screenshot capture per milestone in both themes (`--screenshot` gains `--ui full`).
  Compare against the exported boards in `design/ui/assets/boards/`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| `bevy_ui_widgets` 0.20 is new; gaps in text input (IME, selection) or scroll | Wrap every widget in our own builder (spec §9.5) so a crate or an upstream fix can replace internals |
| Viewport/UI coordinate drift on resize, scale factor and splitter drag | One system sizes the view's texture and camera from `ComputedNode` after layout (D-31). Test it at scale factors 1, 1.5 and 2 |
| Input conflicts between UI, camera and teleop | One input map with scopes, focus tracking, and the CI conflict test |
| Editing services missing from the websocket server | Verify each service against Harmonic in M4/M5 before building UI on it. Fall back to read-only with a disabled control and tooltip |
| Large scope | Milestones are independent PRs. `minimal` mode is usable and shippable after M0 |

---

## 7. Code layout and conventions (`gz-ui`)

Agreed after M2 (DECISIONS.md D-51; the panel contract D-57, D-58), after Rerun (`re_ui`
plus a crate per view, registered as a view class), Zed (`ui` plus a crate per panel behind
a `Panel` trait) and Bevy's `bevy_feathers`. Later milestones follow it; change it here,
not by example.

```
crates/gz-ui/src/
  lib.rs            GzUiPlugin, GzUiSystems, prelude; re-exports Registry, Registered
  spec_checks.rs    the spec's own checks (tokens, contrast, no colour literals, fonts, icons,
                    binding ids)
  test_support/     the headless harness (ui_app), the full UI on it (shell_app, helpers) and
                    a probe panel (probe.rs) for the shell's tests
  registry.rs       Registry<D> (panels' and inspectors'), the ui_id! macro for open ids
  format/           how sim and connection state reads (clock, RTF, step, addresses)
  theme/            Theme and the OS preference; tokens.rs (generated tables), roles.rs,
                    visuals/ (state looks)
  assets/           fonts, icons/ (atlas), brand (logo)
  widgets/          the §5 components, flat under widgets::; layout.rs (row, column,
                    flex_spacer, overlay, clip_box, separator, group/group_header/
                    property_row, set_shown), text.rs (set_text), shortcut.rs, focus.rs
                    (text-field focus, released and hidden focus, keys a focused widget
                    takes, D-52, D-58, D-58a); a folder per
                    widget with more than one file or its own tests (menu/, number_field/,
                    slider/, splitter/, toggle/, tooltip/, tree_row/)
  shell/            mod.rs: ShellPlugin, ShellPlugins, ShellSystems and panel_shown only;
                    commands, layout (ShellLayout), regions (and their tab groups), routing,
                    splitter, collapsed_bar, connection, panel_status (PanelStatus,
                    SetPanelStatus), inspector (the frame); top_bar/ activity_bar/ sidebar/
                    (slots, body, slot_splitter) status_bar/ viewport/ (view, banner,
                    connect_card; rail/ with its menus, view_cube/, hud/: M3)
  panels/           the panel contract (PanelId, PanelDef, PanelView, PanelAction,
                    HeaderAction, PanelActionActivated, PanelBody, PanelRegistry,
                    RegisterPanel, PanelPlugins); a folder per panel with more than one file
                    (scene/ topics/ plot/ console/), a file per one-file panel (assets.rs,
                    sensors.rs, teleop.rs)
  inspectors/       InspectorId, InspectorDef, InspectorRegistry, InspectorPlugins; one
                    folder per kind: world/ (M5 adds model, link, joint, light, sensor,
                    multi)
  gallery/          the --ui gallery page, one file per section
```

Rules:

- **Feature folders.** A shell region, panel, inspector or widget with more than one file or
  tests of its own is a folder: `mod.rs` (the plugin or builder and the module's docs), a
  file per job and `tests.rs`. The jobs: `view.rs` builds entities (bundles, a panel's
  content observer), `systems.rs` keeps state in step (readouts, what a panel writes into
  `PanelStatus`, its action observers). One-file panels and widgets stay files.
- **Panels and inspectors are plugins in a registry** (DECISIONS.md D-57, D-58). A panel
  declares its id as a constant (`pub const SCENE: PanelId = PanelId::new("scene")`; open,
  so a panel in another crate brings its own) and its plugin calls
  `app.register_panel(PanelDef::new(id, title, icon).place(group, order).binding(..)
  .default_open().view(..).header_action(..).content::<Marker>())`. The contract:
  - **What changes at run time** its own systems write into the shell's `PanelStatus`
    through `SetPanelStatus` (`status.set_view(id, ..)`, `set_badge`, `set_count`,
    `set_action_enabled`, which compare and mark a change only), in `ShellSystems::Panels`:
    its body's `PanelView` (messages are `Cow<'static, str>`, so they can say what the data
    says), its activity badge, its header count and which header actions are enabled (an
    action it never sets keeps its registered `enabled`; the shell updates the button in
    place, D-58a). `PanelDef::view` is the view until then; a panel whose
    view never changes needs no system.
  - **Actions are the panel's.** An empty or error state's buttons are `PanelAction { id,
    label, enabled }`, its header's icon buttons `HeaderAction { id, icon, tooltip,
    binding, enabled }`; both trigger `PanelActionActivated { entity, panel, action }`,
    which the panel observes (matching its id). A header action's `binding` does the same
    while its panel is shown. Reconnect (`PanelAction::reconnect()`) is the shell's own. A
    disabled action shows dimmed with "Not available yet" and is no Tab stop.
  - **Content** is built by the panel's own observer on `Add<Marker>` (the marker named in
    `.content::<Marker>()`, inserted on a `Ready` body), with whatever system parameters it
    needs. A ready body is full-bleed: the shell pads only the empty, loading and error
    states.
  - **Bodies are disposable views.** The shell rebuilds a body when its view changes and
    (M7) when it pops out, and keeps it, re-parented, across a second slot opening or
    closing and a swap; a panel never relies on either. Everything a user would miss after
    a rebuild (tree expansion, filter text, scroll offset, plot series, the selected
    sensor) lives in the panel's own resources, which its content is built from; that is
    also what M8 persists. Nothing panel-specific is stored on body entities.
  - **Gating.** Systems that only feed a shown panel (a subscription for an echo or an
    image) run under `panel_shown(id)`; data the panel keeps while closed and the systems
    writing its `PanelStatus` run unconditionally.

  The activity bar (by group, then order, a separator between groups), the sidebar's slots
  and the panel shortcuts only host what `PanelRegistry` holds and draw `PanelStatus`;
  nothing outside `panels/<name>` matches on one panel or one of its actions. `ShellLayout`
  stores ids, and a command naming an id nothing registered stands for the first panel in
  the bar (`PanelRegistry::resolve`); without a `default_open` panel the first one opens
  at start. Panels register while the app is built: the shell closes the registry when it
  spawns, and a later `register_panel` panics. An id registered twice panics (the earlier
  plugin's systems would keep writing its state); a binding registered twice warns, and the
  first panel in the activity bar keeps it. The built-in panels are `PanelPlugins` inside
  `ShellPlugins`: to replace one, disable its plugin there
  (`ShellPlugins.build().disable::<ScenePanelPlugin>()`) and register your own under its id.
  Inspectors work the same way on the same `Registry` (`InspectorId`,
  `register_inspector`, `InspectorPlugins`); `shell::inspector` is the frame.
- **Crates later.** A panel moves to its own crate when it grows heavy or compile times ask
  for it; it depends only on `gz-ui`'s public API (prelude, widgets, theme, registries), so
  the move is mechanical.
- **Widgets.** Spawn functions return a bundle; names are the spec's §5 names, and
  `bevy_feathers`' where the spec has none (`flex_spacer`, `group`, `group_header`). A widget
  with optional parts takes a `XxxProps` struct with `new()` and builder methods
  (`TextButtonProps`, `SliderProps`, `MenuItemProps`); slots for children take a closure
  (`menu`) or are the caller's children (`section_header`). Behaviour markers are components
  named after the widget. Don't add a props struct where two plain parameters say it.
- **Build from primitives.** The shell, panels and gallery use `widgets::layout`
  (`Node { padding, ..row(SPACE[4]) }`) rather than spelling out a flexbox, `set_text` /
  `set_shown` for readouts (they write, and mark changed, only on a change), and
  `Tooltip::new(..).binding(map, id)` for shortcuts (every binding id is checked against
  `shortcuts.json` by `spec_checks`). Sizes from `Theme::s`, spacing from
  `SPACE`, colours only through roles: no colour literals (`spec_checks` greps; only
  `theme/tokens.rs` is exempt).
- **Systems.** A system updates one thing. One that needs more than seven parameters reads
  through a `#[derive(SystemParam)]` struct whose methods do the work (`SlotSources::view`,
  `StatusSources::text`, `PointerOver::over_ui`, `LookTargets::paint`, `RowParts::paint`)
  instead of a `clippy::too_many_arguments` allow; a struct that is only destructured on the
  system's first line is ceremony, not grouping. That is for clarity, not speed: readouts
  that all write `Text` still run one after another. A readout returns early unless what it
  shows changed. Each region's `build()` (each panel's plugin) registers its own systems:
  the layout chain in `ShellSystems::Layout` (the sidebar's after the regions'
  `apply_layout`), the panels' own in `ShellSystems::Panels`, the sidebar's slots (rebuild,
  counts, header actions) in `ShellSystems::Slots`, everything else that shows state in
  `ShellSystems::Readouts`.
- **Focus.** Each region is a `TabGroup` in §6.5's order (`Region::tab_order`), so Tab walks
  top bar → activity bar → sidebar → viewport → inspector → status bar. A node that isn't
  displayed is made invisible (`focus::hide_undisplayed`), since Bevy's tab navigation skips
  only invisible entities; focus on a hidden node is released (D-52); a disabled control is
  no Tab stop. **No nested `TabGroup`s:** Bevy sorts every non-modal group globally by its
  `order` and doesn't descend into a group inside another, so a group inside a region (the
  M3 rail, a panel's list) would jump the Tab order; controls inside a region are plain tab
  stops (`TabIndex`), and only a modal (a dialog, an open menu) gets a group of its own.
  **A focused widget consumes the keys it acts on** (D-58a): a widget handling a key in its
  `FocusedInput` observer calls `propagate(false)`, and only keys that reach the window are
  the shortcuts' (`focus::hide_taken_keys`), so a focused button takes Space from the
  play/pause chord. A new widget with keys does the same; a shortcut reads
  `ButtonInput<KeyCode>` (or the shell's `Shortcuts`), never `KeyboardInput` directly.
- **Visibility.** `pub` is the crate's API only: the plugins and sets, `UiCommand`,
  `ShellLayout`/`Slot`/`Side`/`Tool`/`Phase`, `PanelStatus`/`SetPanelStatus`/`panel_shown`,
  the ids, registries and the panel contract's types, the theme and its looks, the fonts' and icons' builders, and the widgets (constructors, props,
  markers, events). Everything else is `pub(crate)` or private, and module trees are
  `pub(crate)` with re-exports (`shell::MIN_WINDOW`, `theme::Active`, `assets::icon`). The
  leaf modules whose items read better qualified stay public: `format` (`format::clock`;
  domain formatting, so not under `widgets`) and `widgets::{layout, text, shortcut}`
  (`layout::row`). The crate warns on `unreachable_pub`, so check.sh rejects a stray `pub`.
- **Tests** sit next to their module (`#[cfg(test)] mod tests;`, a sibling `tests.rs`);
  shared harness code goes in `test_support`. A region's and widget's tests import it through
  `super::`. The shell's tests use the probe panel (`test_support::probe`), not a built-in
  one; a panel's tests drive it through the shell (open it, change the connection or its
  data, check what its body, header and badge do) rather than restate its strings.
- **Docs.** Every module opens with what it is and its spec §; decisions cite DECISIONS.md.
