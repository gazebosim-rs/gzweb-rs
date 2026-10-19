# Decisions

Log of product and design decisions for the Gazebo Bevy UI. Newest at the bottom. Each entry: what was decided, the alternatives, why, and where the spec reflects it. Implementors append here when they make a call the spec left open.

## 2026-10-18 · Frame and direction (summary of the design sessions)

- **D-01 Layout: activity-sidebar workbench.** Alternatives: fixed workbench with tool rail (W1), ribbon + timeline (W3), viewport-first floating UI (Direction B). Chosen for density, keyboard reach and a stable frame. Spec §2.
- **D-02 Theme: light first, dark second, both tokenized.** Spec §4, tokens.json.
- **D-03 Shortcuts follow Gazebo (Esc, T, R).** Unity-style Q/W/E rejected because the users come from Gazebo. shortcuts.json.
- **D-04 Default tool is Navigate:** click selects, left-drag orbits, right-drag pans, wheel/middle zooms. Box select is its own tool (B). Spec §6.1.
- **D-05 Camera controls live on the viewport only**, as one cube + one rail; nothing that edits the world is on the viewport. Spec §3.4.
- **D-06 File actions live in the world-name menu**, not the top bar. Spec §7.1.
- **D-07 Workspaces switch from the status bar.** Spec §7.8.

## 2026-10-18 · Review follow-ups (Q1–Q7)

- **Q1 Second panel → second stacked slot in the sidebar.** Alternative: pop-out only. Chosen because Topics→Plot, Sensors↔Teleop and the Debug workspace need two docked panels, and pop-outs cover the scene. Pop-out stays as the escape hatch. Spec §3.3.2, §7.11.
- **Q2 Workspaces button removed from the activity bar.** Alternative: keep both entry points. One place per job; the status-bar switcher gains "Manage workspaces…". Spec §3.2, §7.8.
- **Q3 Delete moves behind the inspector's more-actions menu.** Alternative: keep as a quick action with an Undo chip. Chosen for safety; Delete key, context menu and ⌘Z still make it fast. Quick actions become Focus / Duplicate / Save as… (Model) and Focus / Duplicate / Hide (Light). Spec §3.5.
- **Q4 Space tap = play/pause.** Alternatives: ⌘⏎ chord, no key. Chosen because it is the convention in sim and video tools. Tap is < 200 ms with no drag; hold+drag stays temporary Pan. Space never fires in text fields. Risk accepted: a stray Space starts the sim; mitigated by the Undo-free nature of pause and the visible Play button state. shortcuts.json `sim.play_pause`.
- **Q5 X = Teleop emergency stop**, only while Teleop is armed; gizmo drags are disabled while armed so X never means "constrain X" at the same time. shortcuts.json `teleop.stop`.
- **Q6 Model editing happens in place.** Alternative: separate full-screen editor. Chosen to keep world context and avoid a second frame. Sim pauses, other entities dim, EDITING pill with Done, Assets gets a Parts tab. Spec §6.6.
- **Q7 Backends: native speaks gz-transport directly from Rust; web speaks websockets.** Alternative: a bridge process for both. Chosen so the desktop build has no sidecar and the web build uses the standard Gazebo websocket server. Spec §9.4.

## 2026-10-19 · gzweb-rs implementation decisions

- **D-08 Desktop connection deferred (amends Q7).** Both desktop and web use the Gazebo websocket server for now, behind a `SimBackend` trait. A Rust gz-transport client is a later, separate track. Why: gzweb-rs already speaks the websocket protocol on both targets, and every panel can be built on it. See `.agent/UI_PLAN.md` §3.
- **D-09 First target release: Gazebo Jetty (gz-sim 10)** (answers open Q2). In Jetty the websocket server is the gz-sim system `gz-sim-websocket-server-system` (gz-launch is deprecated). See `.agent/UI_PLAN.md` §3a.
- **D-10 UI modes.** gzweb-rs runs in three modes: `none` (snapshot/screenshot), `minimal` (3D view, orbit camera, viewport keyboard bindings, no panels) and `full` (this spec). Defaults: `full` on desktop, `minimal` on the web. Alternative considered: one mode with collapsible chrome. Rejected because embeds and kiosks need a bare view with no UI cost. See `.agent/UI_PLAN.md` §2.

## Open

- Review workspace timeline design.
- Settings dialog layout.
