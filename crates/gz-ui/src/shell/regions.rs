//! The shell's regions (spec §2.1): spawning them, and writing
//! [`ShellLayout`] into their nodes as the window and the layout change.

use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gz_bevy::view::input::InputMap;

use super::collapsed_bar::collapsed_bar;
use super::layout::{self, ShellLayout, Side};
use super::{activity_bar, inspector, sidebar, status_bar, top_bar, viewport};
use crate::inspectors::InspectorRegistry;
use crate::panels::PanelRegistry;
use crate::theme::{border, ColorRole, Fill, Stroke, Theme};
use crate::widgets::layout::{column, display, fill_parent};

/// Marks the shell's regions (spec §2.1).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Region {
    Root,
    TopBar,
    Body,
    ActivityBar,
    Sidebar,
    /// The bar a collapsed sidebar leaves (D-50).
    SidebarBar,
    ViewportArea,
    /// The bar a collapsed inspector leaves.
    InspectorBar,
    Inspector,
    StatusBar,
}

impl Region {
    /// Its place in the Tab order (§6.5: top bar → activity bar → sidebar
    /// → viewport → inspector → status bar; a collapsed panel's bar where
    /// the panel would be). The containers have none.
    pub(crate) fn tab_order(self) -> Option<i32> {
        Some(match self {
            Region::Root | Region::Body => return None,
            Region::TopBar => 0,
            Region::ActivityBar => 1,
            Region::Sidebar => 2,
            Region::SidebarBar => 3,
            Region::ViewportArea => 4,
            Region::InspectorBar => 5,
            Region::Inspector => 6,
            Region::StatusBar => 7,
        })
    }
}

/// Every region but the containers is a tab group, in the spec's order
/// (DECISIONS.md D-58).
pub(crate) fn add_tab_groups(ev: On<Add<Region>>, regions: Query<&Region>, mut commands: Commands) {
    if let Some(order) = regions.get(ev.entity).ok().and_then(|r| r.tab_order()) {
        commands.entity(ev.entity).insert(TabGroup::new(order));
    }
}

/// A plain region node with its background and one border.
pub(crate) fn region(region: Region, node: Node, fill: ColorRole) -> impl Bundle {
    (region, node, Fill(fill), Stroke(ColorRole::Line))
}

/// A side panel's region (§2.1): its width and open state from the
/// layout, a column on `surface` with a 1px `line` border facing the view.
pub(crate) fn side_panel(side: Side, layout: &ShellLayout) -> impl Bundle {
    let (kind, border) = match side {
        Side::Sidebar => (Region::Sidebar, UiRect::right(px(border::HAIRLINE))),
        Side::Inspector => (Region::Inspector, UiRect::left(px(border::HAIRLINE))),
    };
    region(
        kind,
        Node {
            width: px(layout.width(side)),
            flex_shrink: 0.0,
            border,
            display: display(layout.is_open(side)),
            ..column(0.0)
        },
        ColorRole::Surface,
    )
}

/// A container the pointer passes through: not a hit for picking, so
/// "over the UI" means over a real control or panel.
pub(crate) fn pass_through() -> Pickable {
    Pickable::IGNORE
}

/// The UI's own camera on the window. It runs after the 3D view's camera
/// (order 1), which renders into the texture the view node shows.
pub(crate) fn spawn_ui_camera(mut commands: Commands) {
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
pub(crate) fn ui_width(window: &Window, ui_scale: f32) -> f32 {
    window.width() / ui_scale.max(f32::EPSILON)
}

/// The start-up layout from the window's width (§2.1), at the theme's UI
/// scale (which becomes `UiScale` later in the first frame).
pub(crate) fn init_layout(
    mut commands: Commands,
    windows: Query<&Window, With<PrimaryWindow>>,
    theme: Option<Res<Theme>>,
    panels: Res<PanelRegistry>,
    existing: Option<Res<ShellLayout>>,
) {
    if existing.is_some() {
        return;
    }
    let scale = theme.map_or(1.0, |t| t.scale);
    let width = windows
        .single()
        .map_or(layout::MIN_WINDOW.0, |w| ui_width(w, scale));
    commands.insert_resource(ShellLayout::for_width(width, panels.default_open()));
}

/// Refits the layout when the window's width changes: the start-up rules
/// while start-up lasts ([`ShellLayout::startup`]: the window's first real
/// size may come a few frames late), then the floor rule.
pub(crate) fn fit_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    time: Res<Time<Real>>,
    mut frames: Local<u32>,
    mut layout: ResMut<ShellLayout>,
) {
    *frames = frames.saturating_add(1);
    if layout.startup
        && *frames > layout::STARTUP_FRAMES
        && time.elapsed_secs_f64() > layout::STARTUP_SECS
    {
        layout.startup = false;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let w = ui_width(window, ui_scale.0);
    if (w - layout.window_w).abs() > 0.01 {
        layout.fit(w);
    }
}

/// Writes [`ShellLayout`] into the region nodes: widths and collapse
/// (`Display::None`, so a collapsed panel takes no room and never overlays
/// the viewport).
pub(crate) fn apply_layout(layout: Res<ShellLayout>, mut regions: Query<(&Region, &mut Node)>) {
    if !layout.is_changed() {
        return;
    }
    for (region, mut node) in &mut regions {
        match region {
            Region::Sidebar => {
                node.width = px(layout.sidebar_w);
                node.display = display(layout.sidebar_open);
            }
            Region::Inspector => {
                node.width = px(layout.inspector_w);
                node.display = display(layout.inspector_open);
            }
            Region::SidebarBar => node.display = display(!layout.sidebar_open),
            Region::InspectorBar => node.display = display(!layout.inspector_open),
            _ => {}
        }
    }
}

/// Spawns the shell (spec §2.1): a column of top bar, body and status bar;
/// the body a row of activity bar, sidebar, viewport area and inspector.
/// It closes the panel and inspector registries (DECISIONS.md D-57).
pub(crate) fn spawn_shell(
    mut commands: Commands,
    theme: Res<Theme>,
    layout: Res<ShellLayout>,
    map: Option<Res<InputMap>>,
    mut panels: ResMut<PanelRegistry>,
    mut inspectors: ResMut<InspectorRegistry>,
) {
    // The activity bar and the inspector are built from the registries once.
    panels.close();
    inspectors.close();
    let map = map.map_or_else(InputMap::builtin, |m| m.clone());
    let t = theme.clone();
    let l = layout.clone();
    let (panels, inspectors) = (panels.clone(), inspectors.clone());
    commands.spawn((
        Region::Root,
        Node {
            flex_direction: FlexDirection::Column,
            ..fill_parent()
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
                body.spawn(activity_bar::activity_bar(&t, &map, &panels));
                body.spawn(sidebar::sidebar(&t, &l));
                body.spawn(collapsed_bar(&t, Side::Sidebar, &map));
                body.spawn(viewport::viewport_area(&t, &map));
                body.spawn(collapsed_bar(&t, Side::Inspector, &map));
                body.spawn(inspector::inspector(&t, &l, &map, &inspectors));
            });
            root.spawn(status_bar::status_bar(&t, &map));
        })),
    ));
}
