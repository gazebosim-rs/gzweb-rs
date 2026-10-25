# Gazebo UI — Design and Layout Spec

Version 0.4.4 · October 2026 · Target: Rust + Bevy (`bevy_ui`), desktop and WASM

Changes in 0.4.4 (M4, DECISIONS.md D-65 to D-69): §3.3: the Scene tree lists the world, models, links, lights, sensors and joints (no visuals); the filter button shows and hides the type chips and looks active while a chip filters; a filter shows the matches and the paths to them; §5.7: the tree is one tab stop whose arrow keys move a cursor (rows aren't tab stops); its caret and eye ask the tree's owner (controlled); §5.12: filter chips are padded 8 each side; §3.6: the message chip shows the client's own warnings and errors; §9.4: the backend trait as built.

Changes in 0.4.3 (M3, DECISIONS.md D-59 to D-63): §3.4: the scale label holds a bar as long as the length it shows; the rail's 1px border is inside its 4px padding (40 wide outside); the view cube's corners turn the view to look from them, its edge and corner clicks keep the projection, hovering lights what a click picks in `accent_ring`, and its labels are compass directions; §7.2: "Look through" shows once the selection has sensor cameras; §9.1 lists the viewport's controls.

Changes in 0.4.2 (documentation only; DECISIONS.md D-58, D-58a): §9.1 describes the panel contract as built: panels write their own views, define their own actions and header actions, and build their content in their own observer; bodies are disposable views; the regions are tab groups in §6.5's order; a focused control consumes the keys it acts on and a disabled one is no Tab stop (§6.3, §6.5).

Changes in 0.4.1 (documentation only; DECISIONS.md D-51): §9.1 describes the code layout as built: feature folders, and panels and inspectors as plugins registered in a registry the shell hosts.

Changes in 0.4 (owner decisions during the build; DECISIONS.md D-48 to D-50): the top bar's tool and transport groups have no container, their buttons look like activity buttons and Play/Pause is a plain icon button (§3.1, §5.4); tooltips open right of the activity bar and never leave the window (§5.15); a collapsed sidebar or inspector leaves a 24px bar with a button that expands it, and the activity button of the collapsed panel stays selected (§2.1, §3.2, §3.3).

Changes in 0.3: decisions Q1–Q8 recorded in DECISIONS.md and folded in (second sidebar slot, Workspaces out of the activity bar, Delete behind more-actions, Space tap = play/pause, X = e-stop, in-place model editing, gz-transport native + websocket web).

Changes in 0.2 (after design review): platform-aware shortcut table moved to `shortcuts.json`; Space/Z/Ctrl+1–4 conflicts resolved; Esc no longer resets the camera drag action; active-view rule fixed; states (first run, disconnected, empty/loading/error) specified; live-value editing rule; undo scope and dirty-state rules; icon collisions fixed; stray colors tokenized; contrast failures fixed; sizes and labels unified.

This is the implementation spec for the new Gazebo user interface. It is written for an AI or human implementor who has not seen the design sessions. Follow it literally. Where it is silent, follow the closest existing pattern in this document and log the decision in `DECISIONS.md` rather than inventing a new pattern.

Files in this folder:

| File | What it is | Authority |
|---|---|---|
| `DESIGN.md` | This spec: layout, components, behavior, build order | Source of truth for structure and behavior |
| `tokens.json` | Colors (light and dark), type, spacing, radius, sizes, motion | Source of truth for every visual value |
| `theme.rs` | Rust mirror of `tokens.json`: `Theme` resource, `Palette`, `Sizes`, `ColorRole` | Copy into `src/ui/theme.rs` |
| `icons.json` | Every icon's vector path data | Source of truth for icons |
| `shortcuts.json` | Every keyboard binding, per platform, with scope | Source of truth for shortcuts; tooltips, menus, ⌘K and a CI conflict test read it |
| `assets/ui/brand/` | Gazebo icon, light and dark variants | — |

Visual references live on the design canvas "Gazebo UI — Rust + Bevy" (boards named `W2 — …`). Section headings below cite the board to compare against. If the canvas and this file disagree, this file wins. Known board drift: the viewport section of `W2 — Top bar, status bar, viewport controls` shows a superseded design (text dropdowns, bottom-left camera strip) and is obsolete; `W2 — Viewport rail` is current. Boards also predate the 0.2 token changes (hint text, accent text, axis text colors, icon fixes); tokens.json and icons.json win. The 0.4 changes supersede the boards too: they show the top bar's tool and transport groups in `bg` containers with a filled Play/Pause, activity tooltips below their buttons, and collapsed panels with nothing left on screen.

Words used precisely: **MUST** is required for acceptance. **SHOULD** is expected unless there is a logged reason. **MAY** is optional.

---

## 1. Principles

1. **The scene is the product.** The 3D viewport gets every pixel that isn't needed. Chrome is thin, flat and quiet.
2. **Key interfaces are always visible; advanced ones are one click away.** Run controls, transform tools, scene tree and inspector are on screen. Topics, plots, sensors, teleop and console are one click away in the activity bar. Rare settings sit behind menus or the command bar (⌘K).
3. **One place per job.** World and file actions live in the world-name menu. Tools that change the world live in the top bar. Controls that change how you look at the world live on the viewport. Status and messages live in the status bar. Nothing is duplicated across regions.
4. **Gazebo users keep their habits.** Gazebo keyboard shortcuts (Esc, T, R) and mouse navigation carry over.
5. **Every color is a token.** No hex literal appears in widget code. Light and dark themes share one token set.
6. **Icons over words, with tooltips.** Toolbars use icons. Every icon-only control MUST have a tooltip with its name and shortcut.

---

## 2. Screen layout

Board: `W2 — Main screen, split viewport` (light) and `W2 — Main screen, dark`.

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ TOP BAR  h=44                                                                              │
│ [logo][world ▾ •] │ [nav][box][T][R][snap ▾]      [⟲][❚❚][▷|][×1▾]│00:02:14.380 ● RTF 98%      [ search ⌘K ] │
├────┬──────────────┬───────────────────────────────────────────────┬──────────────────────┤
│ A  │ SIDEBAR      │ VIEWPORT AREA (flex 1)                         │ INSPECTOR            │
│ C  │ w=300        │ ┌──────────────────────────┬──────────────────┐│ w=300                │
│ T  │ (240–480)    │ │ Viewport 1 (active)  cube││ Viewport 2  cube ││ (260–480)            │
│ I  │              │ │                     rail ││                  ││                      │
│ V  │ one activity │ │                          ││                  ││ selection details     │
│ I  │ panel at a   │ │                          ││                  ││                      │
│ T  │ time         │ └──────────────────────────┴──────────────────┘│                      │
│ Y  │              │                                                 │                      │
│w=56│              │                                                 │                      │
├────┴──────────────┴───────────────────────────────────────────────┴──────────────────────┤
│ STATUS BAR  h=28   ● localhost · dart 1ms · 60fps · iter · [⚠ message]   hint · sel · cursor · [workspace ▴] │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Region contract

| Region | Size | Background token | Borders | Collapsible |
|---|---|---|---|---|
| Top bar | full width × 44 | `surface` | bottom 1px `line` | No |
| Activity bar | 56 × fill | `bg` | right 1px `line` | No |
| Sidebar | 300 default, 240–480, drag to resize; holds one or two panel **slots** stacked vertically (§3.3.2) | `surface` | right 1px `line` | Yes: click active activity button, or chevron in section header, or ⌘B |
| Sidebar bar (while the sidebar is collapsed) | 24 × fill | `surface` | right 1px `line` | — (it is the collapsed sidebar) |
| Viewport area | fill | `bg` (shows as 2px gap between views) | none | No |
| Inspector bar (while the inspector is collapsed) | 24 × fill | `surface` | left 1px `line` | — |
| Inspector | 300 default, 260–480, drag to resize | `surface` | left 1px `line` | Yes: chevron in its header, or ⌘⌥I (Ctrl+Alt on Linux/Windows) |
| Status bar | full width × 28 | `surface` | top 1px `line` | No |

Rules:

- Root is a column flex: top bar, body (row flex), status bar. Body: activity bar, sidebar, viewport area, inspector.
- Splitters between sidebar|viewport and viewport|inspector are 6px invisible hit areas centered on the 1px border. Cursor changes to col-resize on hover; the border turns `accent` while dragging. Double-click resets to default width.
- Minimum window: 1024 × 640. Below 1280 wide the inspector starts collapsed. Below 1100 wide the sidebar also starts collapsed. Collapsed panels never overlay the viewport; they reopen pushing it narrower.
- A collapsed sidebar or inspector leaves a 24px bar (`surface`, 1px `line` on the side facing the view) between the activity bar (or the window's right edge) and the view, holding at its top a 20px icon button with a chevron pointing at the view (`chevron_right` for the sidebar, `chevron_left` for the inspector; tooltip "Show sidebar · ⌘B" / "Show inspector · ⌘⌥I"). It expands the panel at its previous width. The bars count toward the viewport floor and the view's placement; splitters are hidden while their panel is collapsed.
- Viewport area floor: the area MUST be able to hold every view at `viewport_min` (240). When a resize or a panel opening would break that, collapse the inspector first, then the sidebar, and if still too small, fall back to the Single layout (the multi-view layout is remembered and restored when space returns). Never clamp a view below 240.
- Panel widths, collapsed state and the viewport layout are part of the workspace (§8) and persist.

---

## 3. Regions in detail

### 3.1 Top bar

Board: `W2 — Top bar, status bar, viewport controls`.

Row flex, `align-items: center`, gap 8, padding left 8, right 10. Contents left to right:

| # | Item | Spec |
|---|---|---|
| 1 | Logo | 28×28 image `ui/brand/gazebo-icon.png` (light) or `gazebo-icon-dark.png` (dark). Not interactive. MAY open "About" on click later. |
| 2 | World menu button | Text button: world file name in `text` weight 500, extension `.sdf` in `text_3` weight 400, unsaved dot (7px circle `warn`, only when dirty), chevron_down 15px. Opens the World menu (§7.1). Tooltip: full path; when dirty append "· unsaved changes · ⌘S". |
| 3 | Vertical separator | 1×18 `line`, margin 0 4. |
| 4 | Tool group | No container (no fill, border or padding; gap 2): the buttons sit on the bar's `surface`, white in light. Buttons (icon_btn 32, icon 17, radius 7) look like activity buttons (§3.2 states: transparent with `text_3` icon at rest, `hover`/`text` on hover, the selected tool `accent_soft`/`accent_text` with the inset 1px `accent_ring`): **Navigate** `navigate` (Esc), **Box select** `box_select` (B), **Translate** `translate` (T), **Rotate** `rotate` (R). Exactly one is active (radio). Then a text button "Snap 0.25 ▾" in mono 12 that opens the Snap popover (§7.6). |
| 5 | Flexible spacer | flex 1 |
| 6 | Transport group | No container, as the tool group; its icon buttons have the same activity-button look. **Reset** icon button `reset` (return-to-start glyph, the mirror of Step; not undoable, DECISIONS.md D-89). **Play/Pause** a plain icon button like Reset and Step (no `primary` fill); shows `pause` when running, `play` when paused. **Step** icon button `step`. **Steps per click** text button mono 12 labelled "1 step ▾" (options 1, 10, 100, custom; tooltip "Steps per click"). Separator. **Sim clock** mono 13 weight 500 `text`, format `HH:MM:SS.mmm`. **RTF** status dot (7px; `ok` ≥ 90%, `warn` 50–90%, `danger` < 50%, `disabled` when paused) + "RTF 98%" mono 12 `text_2`; reads "paused" when paused. Hover RTF: tooltip with real time, sim time, iterations, step size. |
| 7 | Flexible spacer | flex 1 (keeps transport visually centered) |
| 8 | Search / command bar trigger | 220 wide text button, border 1px `line`, fill `field`, icon `search`, placeholder "Search or run a command" `text_3`, key cap "⌘K" (mono 11, border `line`, radius 4). Opens the command palette (§7.7). |

Not in the top bar (by decision): Open, Import, Export, Save (in the World menu); workspace switcher (in the status bar); camera controls (on the viewport).

### 3.2 Activity bar

Column flex, centered, gap 2, padding 8 0. Buttons are 44×44, radius 10, icon 20px.

Order (top group, separator, middle group, spacer, bottom group):

1. **Scene** `activity_scene` (hierarchy glyph, never three bars, which reads as a hamburger menu) — ⌘⇧E
2. **Assets** `activity_assets` — ⌘⇧A
3. separator 24×1 `line`, margin 6 0
4. **Topics** `activity_topics` — ⌘⇧T
5. **Plot** `activity_plot` — ⌘⇧P (badge: number of active charts, neutral: fill `text_2`, label `surface`)
6. **Sensors** `activity_sensors`
7. **Teleop** `activity_teleop` (d-pad glyph; shows a 7px `ok` dot while armed and publishing)
8. **Console** `activity_console` — ⌘⇧C (badge: unread warnings+errors; fill `warn_badge` or `danger`)
9. flex spacer
10. **Settings** `settings` (opens Settings dialog, not a panel)

Workspaces has no activity button (decision Q2); it is reached from the status-bar switcher (§7.8) or ⌘K.

Badges: counts use `text_2`; only attention states (warnings, errors) use `warn_badge`/`danger`. Accent is reserved for on/selected states.

States:

| State | Fill | Icon | Ring |
|---|---|---|---|
| Default | transparent | `text_3` | none |
| Hover | `hover` | `text` | none |
| Active (its panel is open, or in the collapsed sidebar) | `accent_soft` | `accent_text` | inset 1px `accent_ring` around the whole button |
| Focus (keyboard) | as state | as state | 2px `accent` outside, offset 2 |

Behavior: clicking an inactive button opens its panel in the sidebar's primary slot (replacing what was there), expanding a collapsed sidebar. Clicking the active button collapses the sidebar; it stays selected (active look, pressed to accessibility) while collapsed, and clicking it again expands the sidebar. Shift+click opens the panel in the secondary slot. A button whose panel is in either slot shows the active state, the sidebar open or collapsed; a panel that is popped out shows a hollow 7px ring instead. Badge: 14px tall pill, top-right offset 6/6, mono 9 weight 600, text color = `surface`.

### 3.3 Sidebar panels

Board: `W2 — Activity panels`. Every panel shares this shell:

- **Section header** 36 tall: title in `section` type (12/600 uppercase, `text_2`), optional count in mono 11 `text_3`, flex spacer, up to three 28px icon buttons, bottom border `line_soft`.
- **Body** scrolls vertically. Inner padding 8 for lists, 14 for forms.
- **Footer** (optional): top border `line`, padding 10 14, row of equal-width buttons (button_h 30). The shell draws it under the body in every state (DECISIONS.md D-98).
- **Collapsed**: the sidebar leaves its 24px bar with a "Show sidebar" button (§2.1); the active activity button stays selected.

| Panel | Content (top to bottom) | Gazebo source |
|---|---|---|
| **Scene** | Header actions: expand all, collapse all. Filter input + type-filter icon button (shows or hides the chip row; active look while a chip other than All filters). Filter chips (All, Models, Lights, Sensors, Joints; one at a time). Tree (§5.7): the world, then models, links, lights, the links' sensors and the models' joints (joints come from the world's SDFormat), no visuals; a filter shows the matches (name contains the text, kind matches the chip) and the paths to them, opened. Footer: New group, Hide others, Isolate. | EntityTree |
| **Assets** | Tabs: Shapes, Lights, Fuel, Local. Shapes tab: 3-column grid of 64px tiles (box, sphere, cylinder, capsule, cone, plane). Hint line "Click to place at cursor · Shift to place several". Fuel section: search input, result rows (44px thumb, name, owner · size, Insert button). As built (D-102): a tile or Insert arms it and a click in the view places it; Local takes a URI the server resolves (the websocket server lists no local models); inserting needs a server that takes edits (D-76). | Shapes, Lights, ResourceSpawner |
| **Topics** | Header actions: publish, services. Filter input. Chips (All, Sensors, Models, Quiet). Topic list rows (mono 12, name ellipsized left-aligned, rate right in `text_2`). Echo section: header with topic, message type + rate + pub/sub counts, live message as monospace text. Footer: Plot field, Copy, Publish… As built (D-99): names clip at the edge; a rate shows for the topics the client receives ("—" otherwise) and the counts don't show, since the websocket server gives neither; Publish and Services are unavailable on that server. | TopicViewer, TopicEcho, Publisher |
| **Plot** | Time window chips (10 s, 30 s, 2 min, All). One or more charts stacked: title, unit, legend rows (12px swatch + field path + live value mono). Footer: Add field…, Export CSV, Pop out. As built (D-100): header + (Add field) and Pause (a toggle, D-101); the field picker opens inline at the top of the body; a chart holds up to four series; Pop out waits for M7e. | Plot |
| **Sensors** | Sensor list for the selected model (type badge, name, rate). Selected sensor preview: camera image, or lidar top-down scan. Footer: Show in viewport, Record. As built (D-103): other sensors' messages show as text; Show in viewport and Record wait. | ImageDisplay, PointCloud |
| **Teleop** | Target select (model · topic), mode segmented control (Keyboard, Sliders, Gamepad), WASD pad (40px keys, pressed = accent_soft), linear/angular sliders with values, max speed fields. Joints section: one slider per joint. Footer: **Stop · Space** (danger text), Apply force… As built (D-104): driving needs the patched websocket server's `pub_in`; a pad key, a slider or the stick arms; a disarm (X, Stop, a new target or mode, hiding) sends the robot a zero and holds until the input lets go, and the server zeroes a client's drives when it disconnects; the subscriber banner, Apply force… and the activity dot wait. | Teleop, KeyPublisher, JointPositionController, ApplyForceTorque |
| **Console** | Filter input, level chips (All, Error, Warn, Info, Debug), log rows (8px level dot, message, `source · time` mono 11). Footer: command input (mono 12). As built (D-105): the client's own log (Gazebo forwards none); header Clear and Pause; the command line runs `gz topic` (-l, -i, -e, -p) and `gz service -s`. | gz log output |
| **Workspaces** | Workspace cards (layout thumbnail 72×44, name, description, ⌘1–4), current layout summary, Save / Save as new / Reset. Footer note on sync location. As built (D-111): Alt+1–4 (§11); the user's own workspaces join with Delete; Save as new… names one in a row under the buttons; Import waits for a file picker, Export saves `gz-workspaces.json`. | GUI config save/load |

Charts MUST follow: 2px lines, `line_soft` gridlines, `line` baseline, axis labels mono 10–11 `text_2`, legend always present for 2+ series, series colors in fixed order `chart_1`, `chart_2`, `chart_3`, `chart_4` (blue, purple, teal, dark orange; orange is last so a two-series plot never looks like a warning), y-axis linear with labeled ticks, live crosshair tooltip on hover. Text never takes the series color. A series keeps its color when others are removed.

### 3.3.2 Two slots (decision Q1)

The sidebar hosts a **primary** slot and an optional **secondary** slot stacked below it, separated by a 6px row splitter (§5.18) with a 32×2 grip in `cube_edge`. Rules:

- Secondary opens when a flow needs a second panel (Topics "Plot field", Sensors "Open in Teleop", Shift+click on an activity button, or the panel header's "Move to second slot" button). Default split 55/45 (a double-click on the splitter restores it); min slot height 180; the ratio persists per workspace.
- Every panel header gains, at its right: move-to-other-slot, swap (when two slots), pop out, and close-slot (secondary only). Closing the secondary returns the sidebar to one slot.
- A panel can be in one slot only. Opening a panel already in the other slot swaps it there.
- Pop-out (§7.11) remains the escape hatch for a third panel or for placing a panel over the scene.
- Workspace defaults: Build = Scene alone; Debug = Topics over Plot; Review = Plot alone.

### 3.3.1 Panel states

Every panel MUST implement these states. Empty and error states are centered in the body: icon 24px `text_3`, one line of `body` text in `text_2`, one optional action Button.

| Panel | Empty | Loading | Error |
|---|---|---|---|
| Scene | "No world loaded" + **Open world…** / **New world** | Skeleton rows (3, `hover` fill) while the first entity snapshot arrives | "Lost connection to server" + **Reconnect** |
| Assets › Fuel | "Search Fuel for models and worlds" | Search: spinner in the input; download: progress bar (2px `accent`) under the result row with size and cancel | "Fuel is unreachable" + **Retry**; a failed insert shows a `danger_soft` row |
| Topics | "No topics yet" (connected) / "Not connected" | Topic list: skeleton; echo: "Waiting for first message…" | Echo on a dropped topic: "Topic ended" in `text_3`, keeps the last message |
| Plot | "Add a field from Topics or the inspector" + **Add field…** | Series before first sample: legend row with "—" | Field no longer exists: legend row in `danger` with ⚠ |
| Sensors | "Select a model with sensors" (selection has none) / "No sensors in this world" | Image: last frame dimmed 50% until the next arrives | Stale > 2 s: "No data for 3.2 s" overlay in `warn_text` |
| Teleop | "Select a robot to drive" | — | Target topic has no subscriber: banner "Nobody is listening on /cmd_vel" `warn_soft` |
| Console | "No messages" | — | — |
| Workspaces | never empty (defaults exist) | — | Load failed: "Couldn't read workspaces.json, using defaults" |
| Inspector | World inspector when nothing is selected | Fields show "—" until first value | Entity removed while shown: header turns `text_3` with "(removed)", quick actions disabled, Delete becomes **Dismiss** |

### 3.4 Viewport area

Boards: `W2 — Main screen, split viewport`, `W2 — Viewport rail (camera controls v2)`.

**Layouts** (presets in the Layout popover): Single; Two columns 3:2; Two rows 1:1; One-plus-two (left full height, right split); Quad. Views are separated by a 2px gap showing `bg`; the divider's hit area is `viewport_divider_hit` (6px) centered on the gap, cursor col-/row-resize, `accent` while dragging, double-click resets the ratio. Min view size 240 (see §2.1 floor rule). Each view has an independent camera, projection, camera drag action, camera mode and overlay set (`ViewState`).

**Active view**: the view that received the last pointer-down, including on its rail or cube. Shown with a 2px `accent` inset outline. Top-bar tools and gizmo drags apply to the active view. Single-key view shortcuts (F, H, 5, 7/1/3, G, C, I, J) apply to the **hovered** view if the pointer is over one, else to the active view; this lets a user hit G over the second view without clicking into it.

**Compact rail**: when a view is shorter than `rail_compact_below_view_h` (500px), Orbit/Pan/Zoom collapse into one button showing the current drag action that opens a 3-item flyout. This keeps cube + rail under 300px in quad layouts.

**Per-view overlays** (all positioned with inset 10 from the view's top-right unless noted):

| Element | Spec |
|---|---|
| View cube | 56×56, standalone, no card. Faces `cube_top` / `cube_right` / `cube_left`, edges `cube_edge`, labels mono 7 `cube_label` (TOP, N, E, …); the face the view is snapped to fills `cube_snapped`. Drop shadow per `elevation.cube`. Drag on cube = orbit. Click face = snap to that axis view and switch to orthographic. Click edge = 45° view; click corner = the view from that corner; both keep the projection. Hovering lights the face, edge or corner a click would pick in `accent_ring`. Labels are compass directions (Gazebo is east-north-up): TOP, BTM, N, S, E, W. Orbiting away from a snapped view returns to perspective. The cube never opens a menu. Position: top 10, right 2 (visually centered over the rail). |
| Rail | Starts 8 below the cube. Width 40, padding 4 (the 1px border included), radius 11, fill `hud`, border 1px `line`, column gap 2. Buttons rail_btn 32, icon 18. Contents in order: **Orbit**, **Pan**, **Zoom** (radio, sets the plain left-drag action for this view, Orbit default; persists per view and across Esc) · separator · **Projection** toggle (icon shows current: `proj_perspective` / `proj_ortho`; key 5) · separator · **Camera mode** (`camera_mode`; opens Camera menu §7.2; when not Free: `accent_soft` fill + 7px `accent` dot top-right; sensor camera uses `entity_sensor` on `sensor_soft`) · **Focus** (`focus`; F frames selection, double-click or H = home) · **Overlays** (`overlays`; opens Overlays menu §7.3; badge = count of overlays on, neutral `text_2`) · separator · **Layout** (`layout_split`; opens Layout popover §7.4). Separators are 22×1 `line`, margin 3 0. |
| Target chip | Shown only when camera mode is Follow, Track or a sensor camera. 28 tall, `hud` fill, border `line`, radius 8: `follow` icon 13px in `accent_text`, target name 12/500, close button 20×20. Placed to the left of the Camera mode button, vertically centered on it, gap 8. × or ⇧F releases. |
| Scale label | Bottom-right, inset 10: mono 11 `text_2` on `hud` pill (e.g. "1 m") after a 1px `text_2` bar with end ticks that long: a 1-2-5 length between 32 and 80 px at the view's scale. Ortho views only. |
| Inactive view | Rail and chip hidden. Cube replaced by a 34px flat face marker (e.g. a square reading TOP for an ortho top view) at 80% opacity. Hovering the view shows the full cube and rail (fade 100ms). |

Floating viewport controls change the view only. They MUST NOT include tools that edit the world.

### 3.5 Inspector

Board: `W2 — Inspector by entity type`. Shows the current selection. With no selection it shows the **World** inspector. With multiple selection it shows a common-fields view: count, shared pose (shows "—" for mixed values), Delete, Group.

Shell (all entity types):

1. Section header "Inspector": pin button (keeps showing this entity when selection changes), collapse chevron. Collapsed, the inspector leaves its 24px bar at the window's right edge with a "Show inspector" button (§2.1).
2. Title row (padding 14 16 10): type tag (mono 11 500, radius 4, padding 2 6; colors per type below), name `entity_name` type (15/600, ellipsized, double-click or F2 to rename), more-actions icon button.
3. Breadcrumb: mono 11 `text_3`, parent chain and source (e.g. `warehouse_demo › amr_01 · fuel://…`).
4. Quick actions row: three equal buttons (button_h 30), never destructive (decision Q3). Delete, Rename, Hide/Show, Copy and Reset-pose live in the more-actions menu (⋯) at the right of the title row; Delete is the last item, in `danger`, with the ⌫ shortcut shown. Delete remains undoable with the status-bar chip (§7.1).
5. Groups (padding 14 16, gap 10, bottom border `line`): group heading `heading` type (13/600) with optional unit hint at right (12 `text_3`).
6. Link list (padding 8 16): rows 36–38 tall, label left, value + "›" right in `text_3`. Opens a sub-view inside the inspector with a back arrow. Last row is always **All components N ›** (the raw ECS component list, editable, Gazebo's ComponentInspectorEditor).

Type tags:

| Type | Tag text | Tag fg | Tag bg |
|---|---|---|---|
| World | WORLD | `text_2` | `entity_badge_bg` |
| Model | MODEL | `accent_text` | `accent_soft` |
| Link | LINK | `text_2` | `entity_badge_bg` |
| Joint | JOINT | `entity_joint` | joint at 12% alpha |
| Light | LIGHT | `entity_light` | light at 12% alpha |
| Sensor | SENSOR | `entity_sensor` | sensor at 12% alpha |

Per-type groups:

| Type | Quick actions | Groups | Links |
|---|---|---|---|
| World | Save, Reset, Statistics | Physics (engine select, step s, RTF target, gravity xyz); Scene (ambient, background, shadows, grid); Plugins (name → detail) | Spherical coordinates, Wind & atmosphere, All components |
| Model | Focus, Duplicate, Save as… | Pose (world; "Position" X Y Z in m, "Rotation" R P Yw in deg; yaw is tagged **Yw** so it never duplicates the Y position tag); Physics (Static, Gravity, Self collide toggles); Plugins (+ add) | Links, Joints, Sensors, **Edit model** (enters in-place editing, §6.6), All components |
| Link | Select model, Show inertia, Show collision | Pose (relative to model); Inertial (mass, ixx iyy izz, link to off-diagonals/CoM); Flags (Gravity, Kinematic, Wind); Geometry (visuals and collisions list) | Sensors, All components |
| Joint | Select parent, Select child, Show axis | Live (position, velocity, effort, command slider); Definition (type, axis xyz, lower/upper, effort/velocity limits, damping/friction) | Plot position/velocity, All components |
| Light | Focus, Duplicate, Hide | Pose (xyz, direction xyz); Light (type, diffuse, specular, intensity slider, range slider, inner/outer angle, cast shadows) | Attenuation, All components |
| Sensor | View in Sensors, Echo topic, Plot | Output (topic, msg type, rate Hz, always on, visualize); type-specific groups (e.g. lidar: horizontal scan, range, noise) | Vertical scan (lidar), All components |

Editing rules:

- Field edits apply on Enter or blur, not per keystroke. Esc while editing reverts the field.
- Number fields accept expressions (`1.5*2`, `90deg`) and units; they display in the unit shown in the group hint.
- Drag on the axis label (X/Y/Z/R/P/Yw) scrubs the value; Shift = coarse (×10), Alt = fine (÷10). Same vocabulary as sliders (§5.11).
- **Live values vs. editing.** Fields that stream from the sim (pose, joint state, world stats) update continuously while unfocused and show a 6px `ok` dot at the group header right ("live"). Focusing a field freezes it; the rest of the group keeps streaming. On commit, the typed value is sent; if the sim is running and the entity is dynamic, the field flashes `accent_soft` once and the sim's next value replaces it (physics may move it again). Blur without change restores streaming.
- Every edit is undoable (⌘Z); see §7.1 for the undo scope.

### 3.6 Status bar

Row flex, height 28, gap 16, padding 0 12, mono 12 `text_2`. Left to right:

1. Connection: 7px dot (`ok` connected, `warn` reconnecting, `danger` lost) + server address.
2. Physics engine and step: "dart · 1 ms".
3. Render rate: "60 fps".
4. Iteration count: "iter 134,380".
5. Latest system message chip (only when one exists; the client's own warnings and errors, DECISIONS.md D-68): 22 tall, radius 5, fill `warn_soft`/`danger_soft`/transparent for info, text `warn_text`/`danger`, Geist 12/500, icon `warning` 13px, message ellipsized at `message_chip_max_w` (420px), count "2 warnings". Click opens the Message tray (§7.5). Undo chips (§7.1) use the same slot with `accent_soft`/`accent_text` and an **Undo** button.
6. flex spacer
7. Tool hint: Geist 12 `text_3`, changes with the active tool (e.g. Navigate: "Click select · Drag orbit · Right-drag pan · Wheel zoom"; Translate: "Drag an axis to move · Shift to snap · Esc to finish").
8. Selection: "sel: amr_01" (or "3 selected").
9. Cursor world position on the ground plane: "cursor 2.41, −0.73".
10. Workspace button: `activity_workspaces` icon 12px + name + chevron_up 10px, 22 tall. Tooltip: "Workspace: Build · click to switch · Alt+1–4". Opens the Workspace switcher (§7.8).

Transient confirmations ("Saved warehouse_demo.sdf") replace item 5 for 3 s using `accent_soft` / `accent_text`. Below 1280px wide, items 7 and 9 (tool hint, cursor) are dropped first, then 3 and 4.

---

## 4. Theming

Board: `W2 — Theme tokens and components, light vs dark`.

### 4.1 Rules

- Theme modes: System (default), Light, Dark. Setting in Settings › Appearance and in ⌘K ("Toggle theme").
- All colors come from `Theme.c` (see `theme.rs`). All fixed sizes from `Theme.s`. Spacing from `SPACE`, radii from `radius`.
- Widgets store a `ColorRole` component (or several, for fill/border/text) and never a resolved `Color`.
- Accent appears only for: selection, active/on states, focus, links, primary-series data. Never decorate with it.
- Play/Pause is the only `primary`-filled control.

### 4.2 Dark-specific choices

- Accent is lighter (`#4C8DFF`), and accent text/icons use `accent_text` (`#7AABFF`) for contrast on `accent_soft`.
- `primary` inverts (light button, dark icon) so Play/Pause stays the highest-contrast control.
- Tooltips are a raised `tooltip` card with a 1px `line` border (not black). Menus use a stronger shadow.
- Viewport defaults (`viewport_sky`, `viewport_ground`, `viewport_grid`) darken. A world's own `<scene><background>` overrides `viewport_sky`; the grid color follows the theme unless the world sets it.
- Logo switches to `gazebo-icon-dark.png` (light strokes, orange face unchanged).

### 4.3 Theme switching (Bevy)

- `Theme` is a `Resource`. On change (`resource_changed::<Theme>`), run `apply_theme_system`, which re-resolves `BackgroundColor`, `BorderColor`, `TextColor` and image tints for every entity carrying a role component.
- Hover/press/active visuals are computed from roles each frame in one `button_visuals_system` from `Interaction` plus a widget state component (`Active`, `Disabled`). No widget sets colors ad hoc.
- System mode: read the OS preference at startup (desktop: platform query or `winit` theme; web: `prefers-color-scheme` media query) and on change events.

### 4.4 Contrast

Text MUST meet 4.5:1 against its background (3:1 for 24px+ and for icons). Every pair listed in §13 was computed from `tokens.json` and passes in both themes (lowest: light `text_3` on `hover` 4.64; dark `text_3` on `hover` 4.53). Keep a CI test that recomputes WCAG ratios from `tokens.json` so a token edit can't silently break this.

Note: the canvas boards were drawn before this check and use light `text_3` `#6B7380` and `accent_text` `#1C64F2`. The token values here (`#646C79`, `#1858D6`) supersede them.

Two more pairs failed in review and were fixed in 0.2: badge label on the orange badge (now `warn_badge` `#B45309` in light, 5.0:1 with `surface` text) and 12px axis tags on `field` (now `axis_x_text`/`axis_y_text`/`axis_z_text`, all ≥ 5.0:1 in light). The brighter `axis_x/y/z` remain for gizmo geometry only, where the 3:1 graphics rule applies.

### 4.5 Viewport colors

A world's `<scene><background>` overrides `viewport_sky`; `<scene><grid>` color, if set, overrides `viewport_grid`; otherwise both follow the theme. All views in a split use the same sky and grid tokens.

---

## 5. Components

Every component below is a Bevy UI bundle-builder function in `src/ui/widgets/`, returning an entity with `Node`, the role components, and its behavior marker. States are driven by `Interaction` and marker components.

Heights are fixed across the app: section headers 36, menu items 30, inspector link rows 36, tree rows 28, bordered buttons 30, inputs 32 (30 in panel filter bars). Boards that show 40px headers or 32px menu items are wrong.

Common state table for clickable components:

| State | Fill | Content | Border/ring |
|---|---|---|---|
| Default | per component | `icon` (icons) / `text` (labels) | per component |
| Hover | `hover` | `text` | — |
| Pressed | `pressed` | `text` | — |
| Active / on | `accent_soft` | `accent_text` | per component |
| Focus-visible | unchanged | unchanged | 2px `accent` ring, offset 2 |
| Disabled | unchanged | 40% opacity | no hover |

### 5.1 IconButton
Sizes: 32 (icon 17), 28 (icon 15), rail 32 (icon 18). Radius 7 (8 on rail). Transparent default. MUST have a tooltip (name + shortcut) and an accessible label.

### 5.2 TextButton
Height 32, padding 0 9, radius 7, gap 6, Geist 13/500, optional leading icon 15 and trailing chevron 15. Used for World menu, Snap, Multiplier, Search trigger.

### 5.3 Button (bordered)
Height 30, padding 0 10–12, radius 6, 1px `line` border, fill `surface_2` (white in light, a step lighter than the panel in dark), Geist 12/500 `text`. Destructive variant: text `danger`. In rows, equal width (`flex: 1`), gap 6.

### 5.4 SegmentedGroup (tool group, transport group)
Container with `bg` fill, radius 8, padding 2, gap 2, holds IconButtons. Radio semantics for the tool group. In the top bar (owner decision, 0.4) the tool and transport groups keep the group semantics (radio for the tools) but drop the container: no fill, border or padding, gap 2, with activity-button-styled icon buttons on the bar's `surface` (§3.1).

### 5.5 ActivityButton
See §3.2. Includes optional Badge (§5.13).

### 5.6 SectionHeader
See §3.3.

### 5.7 TreeRow
Height 28, radius 6, padding 0 8, gap 6, Geist 13. Children: indent spacer (depth × 14), caret 14 wide (▾ expanded, ▸ collapsed, empty for leaves; mono 10 `text_3`), entity badge 16×16 radius 4 (mono 10/500, fg = entity color, bg `entity_badge_bg`; letters: **W** world `entity_world`, **M** model `entity_model`, **L** light `entity_light`, **K** link `entity_link`, **J** joint `entity_joint`, **S** sensor `entity_sensor`), name (ellipsized), visibility toggle 16×16 at the right: hidden entities show `eye_off` in `text_3` and the row at 50% opacity; visible rows show `eye` only on hover.
Selected: fill `accent_soft`, weight 500, badge inverts (fg `surface`, bg `accent`). Hover: `hover`. Multi-select with Shift+click (range) and ⌘/Ctrl+click (toggle; same meaning in the viewport and box select). Right-click opens the entity context menu (§7.9). Drag rows to re-parent where Gazebo allows.
Virtualize the list: only build nodes for visible rows (worlds can have thousands of entities).
Keyboard (§6.5): the tree is one tab stop; rows aren't. Arrow keys move a cursor (up/down; right opens or goes to the first child; left closes or goes to the parent; Home, End, Page up/down), drawn as the 2px `accent` focus ring inside the row while the tree has focus. The caret and the eye ask the tree's owner, which answers by changing the row (DECISIONS.md D-67).

### 5.8 NumberField (axis-tagged)
Height 32, radius 6, 1px `line`, fill `field`, padding 0 8, gap 6, mono 12/500. Leading tag (X/Y/Z/R/P/Yw or a short label like "mass") weight 600: axis tags colored `axis_x_text/axis_y_text/axis_z_text` (R→x, P→y, Yw→z), other tags `text_2`. Optional trailing unit in `text_3`. Focus: border `accent` + 3px `accent_soft` halo. Laid out in 3-column (pose) or 2-column grids, gap 6.

### 5.9 Toggle
34×20 track radius 10, knob 16 at inset 2. Off: track `toggle_track_off`, knob `toggle_knob_off`. On: track `accent`, knob `toggle_knob_on` at left 16. Always paired with a left label in a 30–32px row; the whole row is the hit target.

### 5.10 Select
Height 32, radius 6, `field` fill, `line` border, Geist 13; opens a Menu.

### 5.11 Slider
Track 4px `line`, filled part `accent`, thumb 16 circle with 2px `accent` border and `surface` fill. Label left (12 `text_2`, fixed width), value right (mono 12 `text`). Arrow keys step; Shift = coarse (×10), Alt = fine (÷10).

### 5.12 Chip
- Filter chip: 24 tall, radius 12, padding 0 8, 1px `line`, Geist 12/500 `text_2`; selected = fill `text`, label `surface`.
- Status chip: see §3.6 item 5.
- Target chip: see §3.4.

### 5.13 Badge
Pill 14 tall, min width 14, padding 0 3, mono 9/600 (`micro`); label color `surface`. Fill by meaning: neutral count `text_2`; warnings `warn_badge`; errors `danger`. Never `accent` (accent means selected/on).

### 5.14 Menu and MenuItem
Menu: `surface`, 1px `line`, radius 10, padding 6, shadow per `elevation.menu`, min width `menu_min_w` (200); the World menu is fixed at `world_menu_w` (280). Opens below its trigger (or left of a rail button), flips to stay on screen. Closes on Esc, click outside, or selection.
MenuItem: height 30, radius 6, padding 0 10, gap 10, Geist 13 `text`, leading icon 15 `text_2` (optional), trailing shortcut mono 11 `text_3`. Hover `hover`. Current/checked choice: `accent_soft` + `accent_text`. Checkbox item: 14px box radius 4 (off: 1.5px `text_3` border; on: `accent` fill). Group label: `caps` type, padding 6 10 4. Separator: 1px `line_soft`, margin 4 6. Arrow keys move, Enter activates, type-to-search.

### 5.15 Tooltip
Appears after 500ms hover, 0ms when moving between tooltipped controls within 300ms. Radius 7, padding 8 10, Geist 12; title weight 600 `tooltip_text`, second line `tooltip_text_2`. Light: `tooltip` fill. Dark: `tooltip` fill + 1px `line`. Placed below the trigger (above in the status bar, left of the rail, right of the activity bar), 8px gap, 8px arrow pointing at the trigger. A tooltip never leaves the window: it flips to the opposite side when its own doesn't fit, and shifts along to stay 8px inside the window's edges (the arrow still points at the trigger). One line per row: titles and second lines don't wrap.

### 5.16 Popover
A Menu-styled container with arbitrary content (Snap, Layout presets). Same open/close rules.

### 5.17 ViewCube
See §3.4. Implement as a render-to-texture of a small cube mesh using a dedicated camera whose rotation mirrors the view's camera; display in a UI `ImageNode`; use picking on the texture to find face/edge/corner hits.

### 5.18 Splitter
6px hit area, col-resize / row-resize cursor, `accent` line while dragging. A double-click restores the default size. It is a tab stop (a press focuses it too): the arrow keys along its axis move it 8px, Home and End to its limits; chords (Ctrl/⌘/Alt+arrow) stay with the shortcuts.

---

## 6. Interaction model

### 6.1 Tools (top bar, one active)

| Tool | Key | Left click | Left drag | Notes |
|---|---|---|---|---|
| Navigate (default) | Esc | Select entity under cursor (empty space clears) | The view's camera drag action (Orbit default) | Shift+click adds, ⌘/Ctrl+click toggles; ⌘/Ctrl+drag draws a marquee without leaving the tool |
| Box select | B | Select under cursor | Draw selection rectangle | Shift adds, ⌘/Ctrl toggles |
| Translate | T | Select | Drag gizmo axis or plane; drag on empty space = camera drag action | Hold Shift to snap; X/Y/Z constrain the axis while dragging |
| Rotate | R | Select | Drag gizmo ring | Hold Shift to snap angle |

In every tool: right-drag pans, wheel and middle-drag zoom. Gizmo colors: `axis_x`, `axis_y`, `axis_z`; hovered axis brightens with a `text` outline; snapping shows a tick readout next to the cursor. Teleop armed (publishing) disables gizmo drags so the X stop key cannot collide with the X constraint.

### 6.2 Camera input map (Navigate tool)

| Action | Mouse | Trackpad |
|---|---|---|
| Select | Left click | Click |
| Add to selection | Shift + click | Shift + click |
| Toggle in selection | ⌘/Ctrl + click | ⌘/Ctrl + click |
| Orbit | Left drag | Click-drag |
| Pan | Right drag | Two-finger drag |
| Zoom | Wheel or middle drag | Pinch |
| Zoom to cursor | Ctrl + wheel | Ctrl + pinch |
| Temporary Pan | Hold Space + left drag | Hold Space + click-drag |

The rail's Orbit/Pan/Zoom sets the view's plain left-drag action; it persists per view and is not changed by Esc. Orbit pivots around the point under the cursor at drag start (fallback: selection center, then world origin). There is no hold-to-zoom key; wheel, pinch and middle-drag cover zoom in every mode.

### 6.3 Keyboard map

`shortcuts.json` is the single source of truth and is loaded at startup; tooltips, menus, the command palette and the CI conflict test read it. Summary of the rules it encodes:

- **Modifier vocabulary.** Shift = coarse step and extend selection. Alt = fine step and temporary alternate. ⌘ (Ctrl on Linux/Windows) = command chords and toggle-in-selection. Space (hold) = temporary Pan. Z is reserved for axis constraint and has no other binding.
- **Esc order.** Close open menu/popover → cancel current drag → return to the Navigate tool. Esc never changes the camera drag action and never releases follow (⇧F does).
- **Scopes.** `global` chords fire anywhere except inside text fields that consume them, and a focused control consumes the keys it acts on: Space and Enter press a focused button and don't also play/pause (DECISIONS.md D-58a). `viewport` single letters fire when the pointer is over a view or a view has focus. `teleop` keys (WASD/arrows, X = emergency stop) fire only while the Teleop panel is armed, with focus in the viewport or the Teleop panel; the panel shows a "keyboard captured" indicator while armed.
- **Platform.** ⌘ renders as Ctrl on Linux/Windows. Workspaces are Alt+1–4 on every platform (Ctrl+1–9 are browser tab shortcuts and collide with the ⌃ view presets). Reverse views are Shift+7/1/3.
- **Web build.** Bindings in `web_reserved` (⌘W, ⌘N, ⌘T, ⌘⇧T, ⌘⇧N, ⌘1–9, ⌘D) cannot be intercepted by a page; each has a `web_alt` and tooltips show the alternate in the browser. `prevent_default_when_canvas_focused` lists chords the page must swallow (⌘K, ⌘S, ⌘O, Space, Tab) only while the viewport has focus, so the browser's own shortcuts keep working in text fields.
- **Play/Pause** is a Space **tap** (decision Q4): key down and up within 200 ms with no pointer drag. Space held longer, or dragged while held, is temporary Pan and never toggles. Space does nothing in text fields. The transport Play button tooltip reads "Play · Space".
- **Reset world** is ⌘⇧R; it isn't undoable (DECISIONS.md D-89).

Key table (abbreviated; see the file for scope and web alternates): Esc Navigate · B box select · T translate · R rotate · X/Y/Z constrain · F focus · H home · 5 projection · 7/1/3 top/front/right (Shift+ for bottom/back/left) · Space tap play-pause · Space-hold pan · ⇧F follow · ⇧T track · ⇧Space maximize · ⌥⌘→/↓ split · ⌘W close view · G/C/I/J overlays · ⌘→ step · ⌘⇧R reset · X stop (teleop armed) · ⌘Z/⇧⌘Z undo/redo · ⌘C/V/D copy/paste/duplicate · Delete · F2 rename · ⌘N/O/S/⇧S/I file · ⌘K palette · ⌘B sidebar · ⌘⌥I inspector · Alt+1–4 workspaces · ⌘⇧E/A/T/P/C panels.

### 6.6 Model editing (decision Q6: in place)

Entering: Model inspector › Edit model, context menu › Edit model, or double-click a model with Translate/Rotate active. Nothing changes frame. What changes:

- The sim pauses (resumes on Done if it was running). The status bar reads "sim paused while editing".
- All other entities render at 25% opacity and are not selectable; the ground grid stays.
- An EDITING pill (`hud`, 36 tall, radius 9) floats top-center of the active view: `EDITING` tag, model name, "· N links · M joints · sim paused", and a **Done** button (`primary`). Esc with nothing selected inside the model also exits.
- The Scene tree auto-expands the model and dims other roots; selection is limited to the model's children.
- Assets gains a **Parts** tab (Link, Joint, Visual, Collision, Sensor, Plugin). Dropping a Link into the view adds it at the drop point; dropping Visual/Collision/Sensor onto a link adds it to that link; Joint opens a two-click flow (parent link, child link) with a purple rubber-band line.
- Joints draw as `entity_joint` dashed rings/arrows on their axis; link frames show.
- The Link inspector gains quick actions Add visual / Add collision / Add sensor; the Joint inspector gains Reparent.
- Done: validates (every link has inertia or is static; joints have both ends), writes the model, resumes the sim. Every edit is undoable individually; Undo after Done reverts the whole editing session as one step.

As built (D-112, M9a): entering, the dimming (other models unpickable, their meshes at 25%), the selection limited to the model's parts, the Scene tree's open model and dimmed rows, the EDITING pill with Done, the status bar's "sim paused while editing", the joints and frames turned on, and Esc. Parts, validation and write-back follow (M9b, M9c).

As built (D-113, M9b): the Parts tab, its six tiles and the shape, joint and sensor selects; a click adds a link where it lands, or a visual, collision or sensor to the link clicked; a joint's two clicks with its rubber band; Plugin's fields; a Shapes tile adding a link while editing; each part undoable. Clicks stand in for drops (D-102's placing). Until M9c the parts are a draft: Done drops them.

### 6.4 Selection model
One shared selection across views, tree and inspector (option in Layout popover to unsync later). Selecting in the tree frames nothing automatically; F frames. Hover in the tree highlights the entity in all views with a 1px `selection_outline` at 50% alpha.

### 6.5 Focus and keyboard navigation
Tab order: top bar → activity bar → sidebar → viewport (receives keys) → inspector → status bar; disabled controls are skipped. Arrow keys move within the tool group, activity bar, tree and menus. Focus-visible ring per §5. A focused control takes Space and Enter (DECISIONS.md D-58a).

---

## 7. Menus and popovers

### 7.1 World menu (from the world name)
Header: file name (13/600) and state line mono 11 (`warn_text` "unsaved changes · last saved 12:01", or `text_3` "saved"). Then: New world ⌘N · Open world… ⌘O · Open recent › · separator · Save ⌘S · Save as… ⇧⌘S · Revert to saved (`revert` icon) · separator · Undo ⌘Z · Redo ⇧⌘Z (labels name the action: "Undo move amr_01") · separator · Import SDF / URDF / mesh… ⌘I · Export world as SDF… · Export screenshot / video… · separator · Connect to server… (current address as hint). As built (D-108): the websocket server can't load a world (New, Open, Open recent, Revert unavailable); Save downloads or writes `<world>.sdf` from `generate_world_sdf` with the authored poses; Import and video wait.

**Dirty state.** The world is dirty when an authored property changes: an entity is spawned, deleted, renamed, re-parented, moved by a tool or the inspector, a toggle/field/plugin/light/sensor parameter changes, or an entity is hidden. Physics moving a dynamic body does not dirty the world. Camera, overlays, panel layout and workspaces never dirty it. Save writes the authored world: for dynamic entities that is the pose they had when last placed by the user (or loaded), not the live physics pose. Save as… offers "Use current poses" as a checkbox for users who want to bake a simulated state. The unsaved dot and the menu header follow this flag.

**Undo scope.** Undoable: every operation that dirties the world, plus Delete (not Reset world: DECISIONS.md D-89). Not undoable: play/pause/step, camera moves, layout changes, Teleop commands. Delete also shows a 5-second chip in the status bar ("Deleted amr_01 · Undo") so the undo is one click away. Undo of a move on a running sim restores the authored pose and lets physics continue from there. The stack is per world file and clears on Open or New.

### 7.2 Camera menu (rail)
Free · Follow ‹selection› ⇧F · Track ‹selection› ⇧T · group "Look through" listing sensor cameras of the selection (shown when there are some) · separator · Follow offset… › · FOV & speed… ›. Current mode shown as checked.

### 7.3 Overlays menu (rail)
Checkbox items: Grid G · Collisions C · Inertia I · Center of mass · Joints J · Frames · Lidar scans / sensor data · Wireframe · separator · Apply to all views.

### 7.4 Layout popover (rail)
Row of five layout preset thumbnails (52×36, radius 6, active = `accent_soft` with `accent_ring`), separator, Maximize ⇧Space, Close view ⌘W, separator, checkbox Link cameras, checkbox Shared selection (on).

### 7.5 Message tray (status bar chip)
560 wide, opens upward from the chip. Header: "System messages", counts, Open console ›, Clear. Rows: 8px level dot, message, time mono 11. As built (D-110): warnings and errors only, counted as listed; Clear drops them from the tray and the chip, not the Console.

### 7.6 Snap popover
Translate step (m), rotate step (deg), scale step (reserved), space: World / Local. As built (D-108): World only until the gizmo has local axes.

### 7.7 Command palette (⌘K)
Centered, 560 wide, top offset 15% of window. Search input 44 tall. Results grouped: Commands, Entities, Topics, Panels, Settings. Each result: icon, name, path/hint in `text_3`, shortcut at right. Every action in this spec MUST be reachable here, including every Gazebo GUI plugin by name. As built (D-107): a command runs as its key would; an unavailable one is dimmed with its reason; the shown panels' actions are commands; the World menu's and the workspaces' commands join with M8b and M8c.

### 7.8 Workspace switcher (status bar)
Opens upward. Items: Build Alt+1 · Run Alt+2 · Debug Alt+3 · Review Alt+4 (current checked) · separator · Save current layout · Manage… (opens the Workspaces panel). As built (D-110, D-111): the user's own workspaces list after the four; Manage… opens the Workspaces panel.

### 7.9 Entity context menu (viewport and tree)
Focus F · Follow ⇧F · Track ⇧T · Move camera to · separator · Rename F2 · Hide / Show · Copy ⌘C · Paste ⌘V · Duplicate ⌘D · Delete · separator · Align › (2+ selected: X/Y/Z min/center/max, like Gazebo's AlignTool) · Show › (collisions, inertia, CoM, joints, frames for this entity) · Apply force… · Edit model (models only) · separator · Inspect. As built (D-109): Copy, Paste, Duplicate and Delete act on top-level models through `generate_world_sdf`, `create` and `remove`, undoable; Rename (no server support), Apply force… and Edit model are unavailable; Show › toggles the view's overlays for every entity.

### 7.11 Pop-out window
A panel popped out floats over the viewport area: `surface`, 1px `line`, radius 12, shadow `elevation.menu`, min 320×240, draggable by its 36px header, resizable at edges, clamped to the viewport area. Header: drag grip, title, meta text, **Dock** (returns to the slot it came from) and **Close**. Pop-outs render inside the Bevy window on every platform (no OS windows), stack by last-focused, and persist in the workspace. As built (D-106): every slot header has Pop out; a window is one more panel host; opening a popped-out panel brings its window to the top; persisting waits for the workspaces (M8).

### 7.10 First run and connection
The viewport area shows a centered card (`surface`, radius 12, 420 wide) whenever no world is loaded:
- **Desktop:** logo, "Open a world to start", buttons **Open world…**, **New world**, and a Recent list (up to 5, path in `text_3`). A **Connect to a running server…** link at the bottom.
- **Web:** logo, "Connect to a Gazebo server", address field (prefilled from the URL query or last used), **Connect** button, and status text. On success the card fades out (160ms) and the Scene panel fills.
- **Disconnected** (after a connection existed): a 36px banner across the top of the viewport area, `warn_soft`/`warn_text`: "Lost connection to localhost:11345 · Reconnecting… (3)" with a **Retry now** button; the last frame stays visible, dimmed to 70%, and all world-editing controls disable. After 60 s the banner turns `danger_soft` and offers **Connect to another server…**.

---

## 8. Workspaces and persistence

A workspace stores: active activity panel, sidebar open + width, inspector open + width, viewport layout (preset, split ratios), per-view camera pose, projection, camera mode, overlays, and theme override (optional). Defaults:

| Workspace | Sidebar | Inspector | Views |
|---|---|---|---|
| Build Alt+1 | Scene | shown | 2 columns: Perspective + Top ortho |
| Run Alt+2 | collapsed | collapsed | Single perspective |
| Debug Alt+3 | Topics | shown | Single perspective; Plot in the second slot or popped out (Q1) |
| Review Alt+4 | Plot | collapsed | Single, playback timeline visible |

Storage: desktop `~/.gz/ui/workspaces.json` (use the platform config dir); web: browser storage (IndexedDB or `localStorage`). Unsaved changes to a workspace show "modified" in the Workspaces panel; switching keeps the modifications in memory until app exit unless saved. As built (D-110): `gz/ui/workspaces.json` in the config dir, `localStorage` on the web; the app starts in the workspace it was left in (Build unsaved as before; the start flags go over it); the current workspace and the theme persist on change; cameras save free; a theme asked for isn't saved; a file it can't parse is kept apart, a newer version's never written over; a workspace's theme waits. "modified" (D-111) compares all but the cameras' poses; Reset puts back the layout as saved, then §8's default.

---

## 9. Architecture for Bevy

### 9.1 Code layout

As built in gzweb-rs (`.agent/UI_PLAN.md` §7 has the full conventions; DECISIONS.md D-51). The UI is one crate, `gz-ui`, beside the backend (`gz-core`: protocol and scene model; `gz-bevy`: connection, scene sync, rendering, the camera and the input map) and the app (`gzweb`: native and wasm entry, mode selection).

```
gz-ui/src/
  lib.rs                 GzUiPlugin, GzUiSystems, prelude
  registry.rs            Registry (panels' and inspectors'), open ids
  format/                how sim and connection state reads (clock, RTF, step)
  theme/                 Theme; tokens.rs (generated from tokens.json), roles.rs, visuals/
  assets/                fonts, icons/ (atlas generated from icons.json), brand
  widgets/               §5 components; layout.rs (row, column, flex_spacer, separator,
                         group/property_row), text.rs, shortcut.rs, focus.rs; a folder per
                         larger widget
  shell/                 ShellPlugin, ShellPlugins, ShellSystems; commands, layout
                         (ShellLayout), regions, routing, splitter, collapsed_bar,
                         connection, panel_status, inspector (frame)
    top_bar/             tools, transport, world_button, search
    activity_bar/ sidebar/ status_bar/
    viewport/            view, banner, connect_card; rail/ (and its menus), view_cube/,
                         hud/ (scale label, target chip)
  panels/                the panel contract and PanelRegistry; one plugin per panel:
    scene/ topics/ plot/ console/ assets sensors teleop
  inspectors/            InspectorRegistry; one folder and plugin per selection kind:
    world/               (M5: model/, link/, joint/, light/, sensor/, multi/)
  gallery/               the widget gallery (§12 item 1)
```

Each panel and inspector is a Bevy plugin that registers a definition (an open id, title, icon, shortcut binding, activity bar group and order, whether it opens at start, its header's icon actions, the body it starts with) in a registry resource while the app is built. Its own systems write what changes at run time into the shell's panel status: its body's state (§3.3.1: content, empty, loading or error, with messages and action buttons the panel defines), its activity badge, its header count and which of its header actions are enabled. Its content is built by its own observer when the shell spawns a ready body, and is full-bleed; the empty, loading and error states are padded. Bodies are disposable views: the shell respawns one when its state changes (and keeps it across the second slot opening, closing or swapping), so a panel keeps its expansion, filter, scroll and series in its own resources. Action buttons, in the body or the header, send the panel an event, and so does a header action's shortcut while the panel is shown; Reconnect is the shell's own. The activity bar, the sidebar's slots and the inspector frame show whatever is registered and never name a panel. The built-in panels and inspectors are plugin groups inside `ShellPlugins`, so an app can leave one out or replace it under the same id (DECISIONS.md D-57, D-58). Each region is a tab group in §6.5's order. A panel can move to its own crate when it grows heavy; the plugin is its whole interface. Shell regions and panels are built from `widgets/` and its layout primitives, with colours only through roles (§4.1).

### 9.2 State resources
`Selection { entities: Vec<GzEntityId> }`, `ActiveTool { Navigate | BoxSelect | Translate | Rotate }`, `ViewLayout { preset, ratios, views: Vec<ViewState> }`, `ViewState { camera_pose, projection, camera_mode, drag_action, overlays }`, `ActiveView(usize)`, `Workspace`, `SimStatus { running, sim_time, real_time, rtf, iterations, step_size, connection }`, `MessageLog`, `UndoStack`.

UI systems read state and write `Event`s / `Message`s (`SetTool`, `Select`, `SetPose`, `PlayPause`, `Step(n)`, `ResetWorld`, `OpenPanel`, …). Only `sim/` systems talk to the backend.

### 9.3 Viewports
Each view is a UI `Node` placeholder in the viewport area plus a `Camera3d` that renders into a texture of the node's physical size (scale factor applied), which the node shows; the texture and the camera's target size and projection follow the node's computed position and size in the same frame, after UI layout (DECISIONS.md D-31: a `Camera::viewport` of the window breaks TAA and SSAO in Bevy 0.20). Overlays (cube, rail, chip, labels) are UI children of the placeholder node. One render layer for the scene; gizmos and selection outlines on a second layer drawn by all scene cameras (DECISIONS.md D-90: with split views each view has a layer of its own, the gizmo draws in the active view, and each view renders its own outline mask).

### 9.4 Backend
`trait SimBackend { connect, subscribe(topic), publish(topic, msg), request(service, req), world_control(cmd) }`. As built (DECISIONS.md D-65): `gz_bevy::backend::SimBackend`, which also has `poll` and `flush` for the frame; the app talks to it only through messages, and the websocket server's is the one implementation on both targets for now.
- Native (decision Q7): a gz-transport client in Rust, speaking its wire protocol directly (ZeroMQ for topics and services, UDP multicast discovery, gz-msgs protobuf payloads decoded with `prost`). No bridge process. Verify the protocol details against the gz-transport version shipped with the targeted Gazebo release and keep the client in its own crate (`gz_transport_rs`) so it can be reused.
- Web (decision Q7): a websocket client to a Gazebo websocket server (the one gzweb uses, or a thin server shipped with this project if that one is missing features). Subscriptions, publishing, service calls and world control all go over the socket; payloads stay protobuf, framed with topic name and type.
Both implement `SimBackend` and deliver the same internal message types, so every panel is backend-agnostic.

### 9.5 Text and widgets
Check what the pinned Bevy version offers for text input, scrolling and focus (recent Bevy releases have been adding headless widgets). If text input is missing, use a maintained crate and wrap it in our NumberField / TextField so it can be swapped.

### 9.6 WASM
- No blocking I/O on the main thread; file open/save use browser file pickers and downloads.
- Fonts and icon atlas embedded or loaded from the asset server; keep the first-load payload small.
- Pointer: right-drag needs the browser context menu suppressed over the viewport only.
- Shortcuts: follow `shortcuts.json` `web_reserved` and `web_alt`; tooltips show the alternate in the browser. Call `preventDefault` only for the listed chords and only while the viewport has focus.
- Theme in ⌘K is three commands (Theme: light / dark / follow system), not a toggle.

---

## 10. Accessibility

- Every control has an accessible name (tooltip text). Wire Bevy's accessibility (AccessKit) nodes for buttons, toggles, inputs, tree items and menus.
- Keyboard reaches everything (§6.5). No action is mouse-only except drawing in the viewport; equivalents exist via fields in the inspector.
- Color is never the only signal: active states also change shape (ring), status dots have text, warnings have icons.
- UI scale setting 80–200% (Settings › Appearance) multiplies `Theme.scale`.
- Honor reduce-motion (all durations 0).

---

## 11. Gazebo parity map

Every built-in Gazebo GUI capability and its home in this UI. "Surface" = always visible; "1 click" = menu, panel or context; "Deep" = ⌘K or settings.

| Gazebo plugin | Capability | New home | Tier |
|---|---|---|---|
| WorldControl | Play, pause, step, reset, step size | Top bar transport | Surface |
| WorldStats | Sim time, RTF, iterations | Top bar clock + RTF; hover tooltip; status bar | Surface |
| InteractiveViewControl | Orbit, pan, zoom, select | Viewport input + rail | Surface |
| ViewAngle | View presets | View cube | Surface |
| CameraTracking | Follow, track, move to | Rail camera menu, context menu, ⇧F | 1 click |
| VisualizationCapabilities | Collisions, inertia, CoM, joints, frames, wireframe, transparent | Rail overlays menu | 1 click |
| GridConfig | Grid settings | Overlays + World inspector › Scene | 1 click |
| Screenshot, VideoRecorder | Capture | World menu › Export screenshot/video | 1 click |
| PlaybackScrubber | Log playback | Review workspace timeline | Surface (in playback) |
| TransformControl | Select, translate, rotate, snap | Top bar tool group | Surface |
| Shapes, Lights | Insert primitives and lights | Assets panel | 1 click |
| ResourceSpawner | Fuel and local models | Assets panel › Fuel / Local | 1 click |
| EntityTree | Hierarchy | Scene panel | Surface |
| ComponentInspector(Editor) | Properties, raw components | Inspector; All components | Surface / 1 click |
| CopyPaste | Copy, paste | Keys, context menu | 1 click |
| AlignTool | Align entities | Context menu (multi-select) › Align | 1 click |
| ModelEditor | Add links, joints | Model inspector › Edit model | 1 click |
| TopicViewer, TopicEcho | Topics, echo | Topics panel | 1 click |
| Publisher | Publish | Topics panel › Publish | 1 click |
| Plot | Plot fields | Plot panel | 1 click |
| ImageDisplay | Camera images | Sensors panel | 1 click |
| PointCloud | Point clouds | Overlays › sensor data | 1 click |
| MarkerManager | Markers | Always rendered | Surface |
| Teleop, KeyPublisher | Drive robots | Teleop panel | 1 click |
| JointPositionController | Joint control | Teleop › Joints; Joint inspector | 1 click |
| ApplyForceTorque, MouseDrag | Apply wrenches, drag bodies | Context menu; Alt-drag in viewport | 1 click |
| NavSatMap | Map view | Panel (later) | Deep |
| SphericalCoordinates, EnvironmentLoader | World geo / environment data | World inspector links | Deep |
| GlobalIlluminationVct/Civct | GI | World inspector › Scene › Rendering | Deep |
| TapeMeasure, BananaForScale | Measure, scale reference | ⌘K / Tools | Deep |
| GUI config save/load | Layouts | Workspaces | 1 click |

---

## 12. Build order and acceptance

Build in this order. Each milestone ends with screenshots compared against the named boards, at 1440×900, in both themes.

1. **Theme and widgets.** `theme.rs`, icon atlas from `icons.json`, fonts, widgets §5.1–5.16 in a gallery screen. Accept: gallery matches `W2 — Theme tokens and components` in light and dark; theme switch at runtime re-colors everything with no restart.
2. **Shell layout.** Top bar, activity bar, sidebar, viewport placeholder, inspector, status bar, splitters, collapse, min-size rules. Accept: matches `W2 — Main screen` layout to ±2px; resizing and collapsing behave per §2.1.
3. **Single viewport.** Bevy scene, Navigate tool, input map §6.2, view cube, rail, projection, focus/home. Accept: all of §6.2 works with mouse and trackpad; cube snaps animate in 250ms.
4. **Backend and scene sync.** Connect native and web; populate tree; world control; clock/RTF; status bar. Accept: play/pause/step/reset round-trip with a running Gazebo; tree updates on spawn/delete.
5. **Selection, inspector, transform tools.** Selection sync, Model/Link/Joint/Light/Sensor/World inspectors, Translate/Rotate with snap, undo. Accept: editing pose in the inspector and with the gizmo both update the sim; ⌘Z reverts.
6. **Split viewports.** Layout presets, active view, inactive view state, per-view settings. Accept: matches `W2 — Viewport rail` split example.
7. **Panels.** Assets, Topics, Plot, Sensors, Teleop, Console. Accept: matches `W2 — Activity panels`.
8. **Menus, command palette, workspaces, persistence.**
9. **Accessibility pass, web build hardening, performance** (tree virtualization with 5,000 entities at 60 fps).

---

## 13. Verification checklist

- [ ] No hex color literals outside `theme.rs` (CI grep for `srgb`/`#` in `src/ui/` except theme.rs).
- [ ] Every icon-only button has a tooltip and accessible label.
- [ ] Contrast test passes for pairs: text/surface, text_2/surface, text_3/surface, text_3/bg, text_3/field, text_3/hover, accent_text/accent_soft, warn_text/warn_soft, danger/surface, on_primary/primary, tooltip_text/tooltip, tooltip_text_2/tooltip, surface/warn_badge, axis_x_text/field, axis_y_text/field, axis_z_text/field — both themes, ≥ 4.5:1.
- [ ] Single-letter shortcuts do nothing while a text field has focus.
- [ ] `shortcuts.json` conflict test passes: no two bindings share keys within overlapping scopes on any platform; every `web: null` binding has a `web_alt`.
- [ ] Esc precedence: closes menu → cancels drag → returns to Navigate; never changes the camera drag action or releases follow.
- [ ] Every panel renders its empty, loading and error states (§3.3.1); the no-world card and disconnected banner exist (§7.10).
- [ ] Focusing a live inspector field freezes it; blur without change resumes streaming.
- [ ] Delete shows an Undo chip and ⌘Z reverses it (Reset isn't undoable, DECISIONS.md D-89).
- [ ] No icon glyph is reused for two different actions (Reset ≠ Rotate ≠ Revert; Scene ≠ hamburger; Workspaces ≠ Layout; Focus ≠ Track ≠ Teleop).
- [ ] Viewport camera regions match their UI nodes after window resize, splitter drag and UI scale change.
- [ ] Theme switch, workspace switch and layout changes persist across restart (desktop) and reload (web).
- [ ] Web build: right-drag works in the viewport; browser-reserved shortcuts have alternates.

---

## 14. Open questions (log answers in DECISIONS.md)

Decided (see DECISIONS.md): Q1 second slot · Q2 Workspaces out of the activity bar · Q3 Delete in more-actions · Q4 Space tap = play/pause · Q5 X = e-stop · Q6 in-place model editing · Q7 gz-transport native, websocket web.

Still open:
1. Playback/Review workspace: exact timeline design (W3 board has a starting point).
2. Which Gazebo release is the first target? Decides the gz-transport protocol version and websocket server to verify against.
3. Settings dialog contents and layout (Appearance, Input, Connection, Paths).
