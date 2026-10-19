//! The workbench shell (spec §2, §3; milestone M2): top bar, activity bar,
//! sidebar with two slots, viewport area, inspector and status bar, the
//! splitters and collapse rules, the 3D view inside the viewport area, and
//! the connect card and disconnected banner.
//!
//! [`ShellPlugin`] is added on top of [`GzUiPlugin`](crate::GzUiPlugin) in
//! the full UI mode. The layout state is [`ShellLayout`]; everything the
//! user asks for goes through [`UiCommand`] messages, which
//! [`apply_ui_commands`] applies, so buttons, keys and tests share one
//! path. Sim requests are `gz-bevy` messages (`GzCommand`, `GzReconnect`).

pub mod activity_bar;
pub mod connection;
pub mod format;
pub mod inspector;
pub mod layout;
pub mod sidebar;
pub mod status_bar;
pub mod top_bar;
pub mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::spawn::SpawnWith;
use bevy::picking::events::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::window::{PrimaryWindow, SystemCursorIcon};
use gz_bevy::connection::{GzCommand, GzReconnect};
use gz_bevy::view::controls::{ActiveScopes, ViewPlacementSystems, ViewRouting};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::ViewPlacement;

pub use layout::{Panel, ShellLayout, Side, Slot};

use crate::theme::{border, ColorRole, Fill, Stroke, Theme, ThemeMode};
use crate::GzUiSystems;

/// The top-bar tools (spec §6.1). Only the selection shows until M5.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum Tool {
    #[default]
    Navigate,
    BoxSelect,
    Translate,
    Rotate,
}

impl Tool {
    pub const ALL: [Tool; 4] = [
        Tool::Navigate,
        Tool::BoxSelect,
        Tool::Translate,
        Tool::Rotate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Navigate => "Navigate",
            Tool::BoxSelect => "Box select",
            Tool::Translate => "Translate",
            Tool::Rotate => "Rotate",
        }
    }

    /// Its `shortcuts.json` binding.
    pub fn binding(self) -> &'static str {
        match self {
            Tool::Navigate => "tool.navigate",
            Tool::BoxSelect => "tool.box_select",
            Tool::Translate => "tool.translate",
            Tool::Rotate => "tool.rotate",
        }
    }

    /// The status bar's hint for it (§3.6 item 7).
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Navigate => "Click select · Drag orbit · Right-drag pan · Wheel zoom",
            Tool::BoxSelect => "Drag a box to select · Shift adds · Esc to finish",
            Tool::Translate => "Drag an axis to move · Shift to snap · Esc to finish",
            Tool::Rotate => "Drag a ring to rotate · Shift to snap · Esc to finish",
        }
    }
}

/// Iterations per Step click (§3.1 "1 step ▾").
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepsPerClick(pub u32);

impl Default for StepsPerClick {
    fn default() -> Self {
        StepsPerClick(1)
    }
}

/// What the shell's controls and keys ask for.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// ⌘B, the sidebar header's chevron.
    ToggleSidebar,
    /// ⌘⌥I, the inspector header's chevron.
    ToggleInspector,
    /// An activity button: plain click or Shift+click (§3.2).
    Activity {
        panel: Panel,
        shift: bool,
    },
    OpenPanel {
        panel: Panel,
        slot: Slot,
    },
    SwapSlots,
    CloseSecondary,
    SetTool(Tool),
    /// The theme command: Settings for now (⌘K, which will list "Theme:
    /// light / dark / follow system", is M8; DECISIONS.md).
    SetTheme(ThemeMode),
    ToggleTheme,
    /// A splitter double-click.
    ResetWidth(Side),
}

/// Marks the shell's regions (spec §2.1).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Root,
    TopBar,
    Body,
    ActivityBar,
    Sidebar,
    ViewportArea,
    Inspector,
    StatusBar,
}

/// A splitter between a side panel and the viewport area (§2.1): a 6 px
/// hit area centred on the panel's border.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellSplitter(pub Side);

/// A splitter drag in progress: the width it started from.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SplitDrag(pub f32);

/// Adds the shell. `camera` spawns the UI camera (tests bring their own).
pub struct ShellPlugin {
    pub camera: bool,
}

impl Default for ShellPlugin {
    fn default() -> Self {
        Self { camera: true }
    }
}

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UiCommand>()
            .add_message::<GzCommand>()
            .add_message::<GzReconnect>()
            .init_resource::<Tool>()
            .init_resource::<StepsPerClick>()
            .init_resource::<connection::ConnectionView>()
            .init_resource::<connection::CurrentPhase>()
            .init_resource::<ViewRouting>()
            .init_resource::<viewport::ViewFocus>()
            .init_resource::<status_bar::CursorReadout>()
            .add_systems(PreStartup, init_layout)
            .add_systems(Startup, spawn_shell)
            .add_systems(
                PreUpdate,
                viewport::route_pointer.after(bevy::picking::PickingSystems::Hover),
            )
            .add_systems(
                Update,
                (
                    connection::track_connection,
                    shortcut_keys,
                    apply_ui_commands,
                    fit_to_window,
                    apply_layout,
                    sidebar::rebuild_slots,
                )
                    .chain()
                    .in_set(GzUiSystems::Behaviour),
            )
            .add_systems(
                Update,
                (
                    activity_bar::activity_states,
                    top_bar::tool_states,
                    top_bar::transport_states,
                    top_bar::top_bar_texts,
                    inspector::inspector_texts,
                    status_bar::status_texts,
                    status_bar::narrow_rules,
                    status_bar::cursor_readout,
                    viewport::overlays,
                    splitter_strokes,
                )
                    .in_set(GzUiSystems::Behaviour)
                    .after(sidebar::rebuild_slots),
            )
            .add_systems(Update, viewport::fade_card.after(GzUiSystems::Visuals))
            .init_resource::<ViewPlacement>()
            .add_systems(
                PostUpdate,
                viewport::place_view
                    .after(UiSystems::Layout)
                    .before(ViewPlacementSystems)
                    .before(VisibilitySystems::UpdateFrusta),
            )
            .add_observer(on_splitter_drag_start)
            .add_observer(on_splitter_drag)
            .add_observer(on_splitter_drag_end)
            .add_observer(on_splitter_click);
        activity_bar::build(app);
        top_bar::build(app);
        sidebar::build(app);
        inspector::build(app);
        viewport::build(app);
        if self.camera {
            app.add_systems(Startup, spawn_ui_camera);
        }
    }
}

/// The UI's own camera on the window. It runs after the 3D view's camera
/// (order 1), which renders into the texture the view node shows.
pub fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
        Msaa::Off,
        IsDefaultUiCamera,
    ));
}

/// The window's width in UI logical pixels (`UiScale` applied).
pub fn ui_width(window: &Window, ui_scale: f32) -> f32 {
    window.width() / ui_scale.max(f32::EPSILON)
}

/// The start-up layout from the window's width (§2.1), at the theme's UI
/// scale (which becomes `UiScale` later in the first frame).
pub fn init_layout(
    mut commands: Commands,
    windows: Query<&Window, With<PrimaryWindow>>,
    theme: Option<Res<Theme>>,
    existing: Option<Res<ShellLayout>>,
) {
    if existing.is_some() {
        return;
    }
    let scale = theme.map_or(1.0, |t| t.scale);
    let width = windows
        .single()
        .map_or(layout::MIN_WINDOW.0, |w| ui_width(w, scale));
    commands.insert_resource(ShellLayout::for_width(width));
}

/// Refits the layout when the window's width changes (the floor rule).
pub fn fit_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut layout: ResMut<ShellLayout>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let w = ui_width(window, ui_scale.0);
    if (w - layout.window_w).abs() > 0.01 {
        layout.fit(w);
    }
}

/// ⌘B, ⌘⌥I and the tool keys, from the input map, in the scopes live now
/// (none while typing; viewport keys only when the view has them).
pub fn shortcut_keys(
    keys: Res<ButtonInput<KeyCode>>,
    map: Option<Res<InputMap>>,
    scopes: Option<Res<ActiveScopes>>,
    routing: Res<ViewRouting>,
    mut out: MessageWriter<UiCommand>,
) {
    let (Some(map), Some(scopes)) = (map, scopes) else {
        return;
    };
    let scopes = routing.key_scopes(&scopes.0);
    for b in map.just_pressed(&keys, &scopes) {
        let cmd = match b.id.as_str() {
            "ui.sidebar" => UiCommand::ToggleSidebar,
            "ui.inspector" => UiCommand::ToggleInspector,
            id => match Tool::ALL.into_iter().find(|t| t.binding() == id) {
                Some(tool) => UiCommand::SetTool(tool),
                None => match Panel::ALL.into_iter().find(|p| p.binding() == Some(id)) {
                    Some(panel) => UiCommand::OpenPanel {
                        panel,
                        slot: Slot::Primary,
                    },
                    None => continue,
                },
            },
        };
        out.write(cmd);
    }
}

/// Applies [`UiCommand`]s.
pub fn apply_ui_commands(
    mut commands: MessageReader<UiCommand>,
    mut layout: ResMut<ShellLayout>,
    mut tool: ResMut<Tool>,
    mut theme: ResMut<Theme>,
) {
    for cmd in commands.read() {
        match *cmd {
            UiCommand::ToggleSidebar => layout.toggle(Side::Sidebar),
            UiCommand::ToggleInspector => layout.toggle(Side::Inspector),
            UiCommand::Activity { panel, shift } => layout.activity_click(panel, shift),
            UiCommand::OpenPanel { panel, slot } => layout.open_in(panel, slot),
            UiCommand::SwapSlots => layout.swap_slots(),
            UiCommand::CloseSecondary => layout.close_secondary(),
            UiCommand::SetTool(t) => {
                tool.set_if_neq(t);
            }
            UiCommand::SetTheme(mode) => theme.set_mode(mode),
            UiCommand::ToggleTheme => theme.toggle(),
            UiCommand::ResetWidth(side) => layout.reset_width(side),
        }
    }
}

/// Writes [`ShellLayout`] into the region nodes: widths, collapse
/// (`Display::None`, so a collapsed panel takes no room and never overlays
/// the viewport) and the slot split.
#[allow(clippy::type_complexity)]
pub fn apply_layout(
    layout: Res<ShellLayout>,
    mut regions: Query<(&Region, &mut Node)>,
    mut slots: Query<(&sidebar::SlotView, &mut Node), Without<Region>>,
    mut row_splitters: Query<
        &mut Node,
        (
            With<sidebar::SlotSplitter>,
            Without<Region>,
            Without<sidebar::SlotView>,
        ),
    >,
) {
    if !layout.is_changed() {
        return;
    }
    let shown = |open: bool| if open { Display::Flex } else { Display::None };
    for (region, mut node) in &mut regions {
        match region {
            Region::Sidebar => {
                node.width = px(layout.sidebar_w);
                node.display = shown(layout.sidebar_open);
            }
            Region::Inspector => {
                node.width = px(layout.inspector_w);
                node.display = shown(layout.inspector_open);
            }
            _ => {}
        }
    }
    let two = layout.secondary.is_some();
    for (slot, mut node) in &mut slots {
        let (grow, display) = match (slot.0, two) {
            (Slot::Primary, false) => (1.0, Display::Flex),
            (Slot::Primary, true) => (layout.split, Display::Flex),
            (Slot::Secondary, true) => (1.0 - layout.split, Display::Flex),
            (Slot::Secondary, false) => (0.0, Display::None),
        };
        node.flex_grow = grow;
        node.display = display;
    }
    for mut node in &mut row_splitters {
        node.display = shown(two);
    }
}

/// A side splitter: absolute, 6 px wide, centred on the panel's 1 px
/// border (`right` for the sidebar, `left` for the inspector).
pub fn side_splitter(theme: &Theme, side: Side) -> impl Bundle {
    let hit = theme.s.viewport_divider_hit;
    // Absolute offsets are from the padding box, inside the border: centre
    // the hit area on the border pixel.
    let offset = px(-(hit + border::HAIRLINE) / 2.0);
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: px(0),
        bottom: px(0),
        width: px(hit),
        ..default()
    };
    match side {
        Side::Sidebar => node.right = offset,
        Side::Inspector => node.left = offset,
    }
    (
        node,
        ShellSplitter(side),
        Hovered::default(),
        GlobalZIndex(10),
        bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::ColResize),
        crate::widgets::a11y(
            accesskit::Role::Splitter,
            match side {
                Side::Sidebar => "Resize sidebar",
                Side::Inspector => "Resize inspector",
            },
        ),
    )
}

pub fn on_splitter_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&ShellSplitter>,
    layout: Res<ShellLayout>,
    mut commands: Commands,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        commands
            .entity(ev.entity)
            .insert(SplitDrag(layout.width(s.0)));
    }
}

/// The pointer moves in window pixels and widths are UI pixels: the drag
/// divides by `UiScale` (DECISIONS.md D-41).
pub fn on_splitter_drag(
    ev: On<PointerDrag>,
    scale: Res<UiScale>,
    splitters: Query<(&ShellSplitter, &SplitDrag)>,
    mut layout: ResMut<ShellLayout>,
) {
    if let Ok((s, drag)) = splitters.get(ev.entity) {
        layout.drag(s.0, drag.0, ev.distance.x / scale.0);
    }
}

pub fn on_splitter_drag_end(
    ev: On<PointerDragEnd>,
    splitters: Query<(), With<ShellSplitter>>,
    mut commands: Commands,
) {
    if splitters.contains(ev.entity) {
        commands.entity(ev.entity).remove::<SplitDrag>();
    }
}

/// A double-click resets the width (§2.1).
pub fn on_splitter_click(
    ev: On<PointerClick>,
    splitters: Query<&ShellSplitter>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(s) = splitters.get(ev.entity) {
        if ev.count >= 2 {
            out.write(UiCommand::ResetWidth(s.0));
        }
    }
}

/// The panel's border turns `accent` while its splitter drags (§2.1).
pub fn splitter_strokes(
    splitters: Query<(&ShellSplitter, Has<SplitDrag>)>,
    mut regions: Query<(&Region, &mut Stroke)>,
) {
    for (s, dragging) in &splitters {
        let want = Stroke(if dragging {
            ColorRole::Accent
        } else {
            ColorRole::Line
        });
        let region = match s.0 {
            Side::Sidebar => Region::Sidebar,
            Side::Inspector => Region::Inspector,
        };
        for (r, mut stroke) in &mut regions {
            if *r == region {
                stroke.set_if_neq(want);
            }
        }
    }
}

/// A container the pointer passes through: not a hit for picking, so
/// "over the UI" means over a real control or panel.
pub fn pass_through() -> Pickable {
    Pickable::IGNORE
}

/// Spawns the shell (spec §2.1): a column of top bar, body and status bar;
/// the body a row of activity bar, sidebar, viewport area and inspector.
pub fn spawn_shell(
    mut commands: Commands,
    theme: Res<Theme>,
    layout: Res<ShellLayout>,
    map: Option<Res<InputMap>>,
) {
    let map = map.map_or_else(InputMap::builtin, |m| m.clone());
    let t = theme.clone();
    let l = layout.clone();
    commands.spawn((
        Region::Root,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        pass_through(),
        Children::spawn(SpawnWith(move |root: &mut ChildSpawner| {
            root.spawn(top_bar::top_bar(&t, &map));
            root.spawn((
                Region::Body,
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                },
                pass_through(),
            ))
            .with_children(|body| {
                body.spawn(activity_bar::activity_bar(&t, &map));
                body.spawn(sidebar::sidebar(&t, &l));
                body.spawn(viewport::viewport_area(&t));
                body.spawn(inspector::inspector(&t, &l, &map));
            });
            root.spawn(status_bar::status_bar(&t));
        })),
    ));
}

/// A plain region node with its background and one border.
pub(crate) fn region(region: Region, node: Node, fill: ColorRole) -> impl Bundle {
    (region, node, Fill(fill), Stroke(ColorRole::Line))
}
