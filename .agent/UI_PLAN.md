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
gzweb     (exists)  binary/wasm entry: picks the mode and adds the plugins
```

Rules that keep `minimal` real rather than a stripped-down `full`:
- **The camera lives in `gz-bevy::view`, not `gz-ui`.** It is a `ViewCamera` component with a
  state machine (drag action, projection, mode, pivot, animated snaps) driven by **messages**
  (`ViewCommand::{Orbit, Pan, Zoom, Snap(face), Focus, Home, SetProjection, …}`).
  `minimal` produces those messages from raw mouse and keys. `full` produces the same messages
  from the rail, the cube and the same key map, scoped to the hovered/active view.
- **One input map**, loaded from `shortcuts.json` (embedded at build time), shared by both modes.
  `minimal` enables only the `viewport` scope plus `sim.*`. `full` enables everything. The
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
| Left drag / right drag / wheel, middle drag | orbit / pan / zoom (pivot = point under cursor) |
| Ctrl + wheel | zoom to cursor |
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
   Ultra means 4× TAA/SSAO/shadow cascades. **Proposed:** inactive views drop to the `High`
   preset, and the shadow map is shared. Measure in M6.
6. **Spec location.** `design/ui/` (DESIGN.md, DECISIONS.md, tokens.json, icons.json,
   shortcuts.json, theme.rs, brand assets, boards). Agents and CI read this one copy. Decisions
   we make go into its `DECISIONS.md`, as the spec asks.
7. **Fonts.** The Geist and Geist Mono TTFs (OFL-1.1) aren't in the zip. They get vendored from the
   upstream release, with the licence file, and embedded with `include_bytes!` for wasm.
8. **Icons.** `icons.json` holds 89 SVG path icons. A `build.rs` in `gz-ui` rasterizes them with
   `resvg` into an atlas (1× and 2×, at 15/17/18/20 px) and generates the `IconId` enum. Icons
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

### M0 — Split the camera out and add UI modes (no visible change)
- `gz-bevy::view`: move `camera.rs` orbit/pan/zoom into `ViewCamera` + `ViewCommand`. Add
  projection toggle, snap views with a 250 ms animation, focus/home, Space tap/hold, Ctrl+wheel
  zoom to cursor, WASD/QE fly.
- `UiMode { None, Minimal, Full }` replaces `GzConfig.interactive`. Wire `--ui` and `?ui=`.
  `Full` is temporarily the same as `Minimal`.
- Load `shortcuts.json` into an `InputMap` resource. Add the CI conflict test.
- Tests: unit tests for every `ViewCommand`; a headless `minimal` app driven by synthetic input.

### M1 — Theme, icons, fonts, widgets (`gz-ui` crate)
- Generate `theme.rs` from `tokens.json` in `build.rs`, or keep the spec's
  hand-written mirror plus a test that it matches. Add the WCAG contrast test over the §13 pairs.
- Icon atlas build step, fonts, `ColorRole` + `apply_theme_system` + `button_visuals_system`.
- Widgets §5.1–5.16 on `bevy_ui_widgets`: IconButton, TextButton, Button, SegmentedGroup,
  SectionHeader, TreeRow, NumberField (text_input + expression parser + scrub), Toggle, Select,
  Slider, Chip, Badge, Menu/MenuItem, Tooltip, Popover, Splitter.
- A `--ui gallery` debug screen. Accept: both themes render, and runtime theme switch re-colours
  everything.

### M2 — Shell layout
- Top bar, activity bar, sidebar with two slots, viewport area, inspector, status bar, splitters,
  collapse rules, minimum-size and floor rules (§2.1). Panels are empty shells with their
  empty/loading/error states.
- The single 3D view renders into the viewport-area node (`Camera::viewport` synced each frame to
  the node rect, scale-factor aware). The no-world card and the disconnected banner (§7.10) are
  driven by `GzStatus`.

### M3 — Viewport controls
- Rail (Orbit/Pan/Zoom radio, projection, camera mode, focus, overlays, layout), compact rail,
  view cube (render-to-texture camera + picking on the image), scale label in ortho, target chip.
- Overlays menu wired to existing toggles: grid, collisions, wireframe (line-mesh fallback on
  wasm), joints, frames, inertia/CoM.
- Camera menu: Free / Follow / Track (the gzweb camera modes from PORT_PLAN Phase 5).

### M4 — Backend, scene sync, transport and status
- `SimBackend` trait with the websocket implementation (see §3 item 1).
- Top-bar transport: play/pause/step/steps-per-click/reset, sim clock, RTF from `/stats`.
  Status bar: connection, engine/step, fps, iterations, messages chip, tool hint, selection,
  cursor position.
- Scene panel: entity tree from `GzIndex`, virtualized, with filter, chips and visibility toggles.
  Spawn and delete update it live; this needs `scene/deletion` handling, which PORT_PLAN Phase 1
  left out.

### M5 — Selection, inspector, transform tools, undo
- One shared `Selection`: click/Shift/⌘ in views and tree, box select (B), hover highlight, and a
  selection outline (second render layer).
- Inspectors: World, Model, Link, Joint, Light, Sensor, multi-select. Live-value streaming and
  the freeze-on-focus rule. All components view.
- Translate/Rotate gizmos with snap and X/Y/Z constraints. Edits go through `set_pose` and the
  other services.
- `UndoStack` and dirty-state rules (§7.1). Undo chips for Delete and Reset.

### M6 — Split viewports
- Layout presets, active/hovered view rules, inactive-view face marker, per-view `ViewState`,
  floor and fallback rules, the multi-view quality policy (§3 item 5).

### M7 — Panels
- Assets (shapes and lights via `/create`; Fuel search and insert), Topics (list, echo, publish),
  Plot (charts per §3.3 rules), Sensors (camera image via the websocket `image` op; lidar
  top-down), Teleop (keyboard, sliders, gamepad via gilrs; joints; X e-stop), Console (§3 item 3).
- Pop-out windows (§7.11).

### M8 — Menus, command palette, workspaces, persistence
- World menu (Save via `generate_world_sdf`, Save as, Open/Import where the server allows,
  Export screenshot), entity context menu with Align, ⌘K over every action and panel, Snap
  popover, message tray, workspace switcher.
- Persistence: desktop config dir `gz/ui/workspaces.json`; web `localStorage`.

### M9 — Model editing (§6.6)
- Sim pause, dimming, EDITING pill, Parts tab, link/joint/visual/collision/sensor insertion,
  validation, write-back.

### M10 — Hardening
- AccessKit names and focus order (§10), UI scale 80–200%, reduce-motion. Web: context-menu
  suppression, `web_reserved`/`web_alt`, `preventDefault` only for listed chords.
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
| Viewport/UI coordinate drift on resize, scale factor and splitter drag | One system syncs `Camera::viewport` from `ComputedNode` after layout. Test it at scale factors 1, 1.5 and 2 |
| Input conflicts between UI, camera and teleop | One input map with scopes, focus tracking, and the CI conflict test |
| Editing services missing from the websocket server | Verify each service against Harmonic in M4/M5 before building UI on it. Fall back to read-only with a disabled control and tooltip |
| Large scope | Milestones are independent PRs. `minimal` mode is usable and shippable after M0 |
