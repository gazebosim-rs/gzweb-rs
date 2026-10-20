//! The sidebar (spec §3.3): a primary and an optional secondary slot with a
//! row splitter between them (§3.3.2), each holding one panel's shell: a
//! section header with its title, count and slot buttons, and a body in
//! its empty, loading or error state (§3.3.1). The panels' contents come
//! in M4 and M7.

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, InteractionDisabled};
use bevy::ui_widgets::Activate;
use bevy::window::SystemCursorIcon;
use gz_bevy::connection::GzReconnect;
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::input::InputMap;

use super::connection::{CurrentPhase, Phase};
use super::layout::DEFAULT_SPLIT;
use super::{region, side_splitter, Panel, Region, ShellLayout, Side, Slot, UiCommand};
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::{border, radius, ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::menu::shortcut_label;
use crate::widgets::{
    button, icon_button, rounded, section_header, ButtonVariant, IconButtonSize, Tooltip,
};

/// A sidebar slot's node.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotView(pub Slot);

/// The row splitter between the slots.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SlotSplitter;

/// A row-splitter drag: the split and the sidebar's height at its start.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SlotDrag {
    pub split: f32,
    pub height: f32,
}

/// A panel header's slot buttons (§3.3.2).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAction {
    /// With two slots: the panels swap.
    Swap,
    /// The primary header's chevron: collapse the sidebar.
    Collapse,
    /// The secondary header's close: back to one slot.
    Close,
}

/// What a panel's body shows (§3.3.1).
#[derive(Clone, Debug, PartialEq)]
pub enum PanelView {
    /// Content (M4/M7); the shell's body is empty.
    Ready,
    Empty {
        message: &'static str,
        actions: Vec<PanelAction>,
    },
    /// Skeleton rows.
    Loading,
    Error {
        message: &'static str,
        actions: Vec<PanelAction>,
    },
}

/// A button in an empty or error state.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    /// Retry the server now.
    Reconnect,
    /// Not available until the World menu (M8): shown disabled.
    OpenWorld,
    NewWorld,
    AddField,
}

impl PanelAction {
    pub fn label(self) -> &'static str {
        match self {
            PanelAction::Reconnect => "Reconnect",
            PanelAction::OpenWorld => "Open world…",
            PanelAction::NewWorld => "New world",
            PanelAction::AddField => "Add field…",
        }
    }

    /// Whether it does something yet.
    pub fn available(self) -> bool {
        self == PanelAction::Reconnect
    }
}

/// A panel's state in a connection phase (§3.3.1's table, as far as M2
/// knows the world: the connection and whether the scene arrived).
pub fn panel_view(panel: Panel, phase: Phase) -> PanelView {
    let empty = |message, actions| PanelView::Empty { message, actions };
    match panel {
        Panel::Scene => match phase {
            Phase::Live => PanelView::Ready,
            // Skeleton rows while the first entity snapshot arrives.
            Phase::Loading => PanelView::Loading,
            Phase::Lost { .. } => PanelView::Error {
                message: "Lost connection to server",
                actions: vec![PanelAction::Reconnect],
            },
            Phase::Idle | Phase::Connecting => empty(
                "No world loaded",
                vec![PanelAction::OpenWorld, PanelAction::NewWorld],
            ),
        },
        Panel::Topics => match phase {
            Phase::Live => empty("No topics yet", vec![]),
            Phase::Loading => PanelView::Loading,
            _ => empty("Not connected", vec![]),
        },
        Panel::Assets => empty("Search Fuel for models and worlds", vec![]),
        Panel::Plot => empty(
            "Add a field from Topics or the inspector",
            vec![PanelAction::AddField],
        ),
        Panel::Sensors => empty("Select a model with sensors", vec![]),
        Panel::Teleop => empty("Select a robot to drive", vec![]),
        Panel::Console => empty("No messages", vec![]),
    }
}

/// A panel header's count: Scene's entities once the world is loaded.
pub fn panel_count(panel: Panel, phase: Phase, index: Option<&GzIndex>) -> Option<String> {
    match (panel, phase) {
        (Panel::Scene, Phase::Live) => index.map(|i| {
            let n = i.order.len();
            format!("{n} {}", if n == 1 { "entity" } else { "entities" })
        }),
        _ => None,
    }
}

/// The sidebar region.
pub fn sidebar(theme: &Theme, layout: &ShellLayout) -> impl Bundle {
    let t = theme.clone();
    (
        region(
            Region::Sidebar,
            Node {
                width: px(layout.sidebar_w),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::right(px(border::HAIRLINE)),
                display: if layout.sidebar_open {
                    Display::Flex
                } else {
                    Display::None
                },
                ..default()
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |s: &mut ChildSpawner| {
            s.spawn(slot_node(Slot::Primary));
            s.spawn(slot_splitter(&t));
            s.spawn(slot_node(Slot::Secondary));
            s.spawn(side_splitter(&t, Side::Sidebar));
        })),
    )
}

fn slot_node(slot: Slot) -> impl Bundle {
    (
        SlotView(slot),
        Node {
            flex_direction: FlexDirection::Column,
            flex_basis: px(0),
            flex_grow: match slot {
                Slot::Primary => DEFAULT_SPLIT,
                Slot::Secondary => 1.0 - DEFAULT_SPLIT,
            },
            min_height: px(0),
            overflow: Overflow::clip(),
            ..default()
        },
    )
}

/// 6 px row splitter with a 32×2 grip in `cube_edge` (§3.3.2).
fn slot_splitter(theme: &Theme) -> impl Bundle {
    (
        SlotSplitter,
        Node {
            height: px(theme.s.viewport_divider_hit),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::top(px(border::HAIRLINE)),
            display: Display::None,
            ..default()
        },
        crate::theme::Stroke(ColorRole::Line),
        Hovered::default(),
        bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::RowResize),
        crate::widgets::a11y(accesskit::Role::Splitter, "Resize slots"),
        children![(
            Node {
                width: px(32),
                height: px(2),
                border_radius: rounded(1.0),
                ..default()
            },
            Fill(ColorRole::CubeEdge),
        )],
    )
}

/// What a slot shows; rebuilt when it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotContent {
    pub panel: Option<Panel>,
    pub view: Option<PanelView>,
    pub count: Option<String>,
    pub two: bool,
}

/// Rebuilds a slot's header and body when its panel, state or count
/// changes.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_slots(
    layout: Res<ShellLayout>,
    phase: Res<CurrentPhase>,
    index: Option<Res<GzIndex>>,
    theme: Res<Theme>,
    map: Option<Res<InputMap>>,
    slots: Query<(Entity, &SlotView)>,
    mut shown: Local<[Option<SlotContent>; 2]>,
    mut commands: Commands,
) {
    let two = layout.secondary.is_some();
    for (e, slot) in &slots {
        let (panel, i) = match slot.0 {
            Slot::Primary => (layout.primary, 0),
            Slot::Secondary => (layout.secondary, 1),
        };
        let want = SlotContent {
            panel,
            view: panel.map(|p| panel_view(p, phase.0)),
            count: panel.and_then(|p| panel_count(p, phase.0, index.as_deref())),
            two,
        };
        if shown[i].as_ref() == Some(&want) {
            continue;
        }
        commands.entity(e).despawn_children();
        if let (Some(panel), Some(view)) = (want.panel, want.view.clone()) {
            let t = theme.clone();
            let map = map.as_deref().cloned().unwrap_or_else(InputMap::builtin);
            let count = want.count.clone();
            let which = slot.0;
            commands.entity(e).with_children(|p| {
                p.spawn(section_header(&t, panel.title(), count))
                    .with_children(|h| header_buttons(h, &t, &map, which, two));
                p.spawn(panel_body(&t, panel, view));
            });
        }
        shown[i] = Some(want);
    }
}

fn header_buttons(
    h: &mut ChildSpawnerCommands,
    theme: &Theme,
    map: &InputMap,
    slot: Slot,
    two: bool,
) {
    let small = IconButtonSize::Small;
    if two {
        h.spawn((
            icon_button(
                theme,
                IconId::MoveTo,
                small,
                Tooltip::new("Swap slots").detail("Move this panel to the other slot"),
            ),
            SlotAction::Swap,
        ));
    }
    match slot {
        Slot::Primary => {
            h.spawn((
                icon_button(
                    theme,
                    IconId::ChevronLeft,
                    small,
                    Tooltip::new("Collapse sidebar")
                        .shortcut_opt(shortcut_label(map, "ui.sidebar")),
                ),
                SlotAction::Collapse,
            ));
        }
        Slot::Secondary => {
            h.spawn((
                icon_button(
                    theme,
                    IconId::Close,
                    small,
                    Tooltip::new("Close second slot"),
                ),
                SlotAction::Close,
            ));
        }
    }
}

/// Marks a panel body.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelBody(pub Panel);

/// A skeleton row (§3.3.1 loading: `hover` fill).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SkeletonRow;

fn panel_body(theme: &Theme, panel: Panel, view: PanelView) -> impl Bundle {
    let t = theme.clone();
    let centred = matches!(view, PanelView::Empty { .. } | PanelView::Error { .. });
    (
        PanelBody(panel),
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(SPACE[4])),
            row_gap: px(if centred { SPACE[5] } else { SPACE[1] }),
            justify_content: if centred {
                JustifyContent::Center
            } else {
                JustifyContent::FlexStart
            },
            align_items: if centred {
                AlignItems::Center
            } else {
                AlignItems::Stretch
            },
            overflow: Overflow::clip(),
            ..default()
        },
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| match view {
            PanelView::Ready => {}
            PanelView::Loading => {
                for width in [72.0, 56.0, 64.0] {
                    b.spawn(Node {
                        height: px(t.s.tree_row_h),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(SPACE[3])),
                        ..default()
                    })
                    .with_child((
                        SkeletonRow,
                        Node {
                            width: Val::Percent(width),
                            height: px(12),
                            border_radius: rounded(radius::XS),
                            ..default()
                        },
                        Fill(ColorRole::Hover),
                    ));
                }
            }
            PanelView::Empty { message, actions } | PanelView::Error { message, actions } => {
                let id = if actions.contains(&PanelAction::Reconnect) {
                    IconId::Warning
                } else {
                    panel.icon()
                };
                b.spawn(icon(id, 24.0, ColorRole::Text3));
                b.spawn((
                    text(TypeStyle::BODY, ColorRole::Text2, message),
                    TextLayout::justify(Justify::Center),
                ));
                if !actions.is_empty() {
                    b.spawn(Node {
                        column_gap: px(SPACE[4]),
                        ..default()
                    })
                    .with_children(|row| {
                        for a in actions {
                            let mut e =
                                row.spawn((button(&t, a.label(), ButtonVariant::Normal), a));
                            if !a.available() {
                                e.insert((
                                    InteractionDisabled,
                                    Tooltip::new(a.label()).detail("Not available yet"),
                                ));
                            }
                        }
                    });
                }
            }
        })),
    )
}

/// Header and state buttons.
pub fn on_slot_button(
    ev: On<Activate>,
    actions: Query<&SlotAction>,
    panel_actions: Query<(&PanelAction, Has<InteractionDisabled>)>,
    mut out: MessageWriter<UiCommand>,
    mut reconnect: MessageWriter<GzReconnect>,
) {
    if let Ok(action) = actions.get(ev.entity) {
        let cmd = match action {
            SlotAction::Swap => UiCommand::SwapSlots,
            SlotAction::Collapse => UiCommand::ToggleSidebar,
            SlotAction::Close => UiCommand::CloseSecondary,
        };
        out.write(cmd);
    } else if let Ok((PanelAction::Reconnect, false)) = panel_actions.get(ev.entity) {
        super::connection::retry(&mut reconnect);
    }
}

pub fn on_slot_drag_start(
    ev: On<PointerDragStart>,
    splitters: Query<&ChildOf, With<SlotSplitter>>,
    nodes: Query<&ComputedNode>,
    layout: Res<ShellLayout>,
    mut commands: Commands,
) {
    if let Ok(parent) = splitters.get(ev.entity) {
        let height = nodes
            .get(parent.parent())
            .map_or(0.0, |n| n.size().y * n.inverse_scale_factor());
        commands.entity(ev.entity).insert(SlotDrag {
            split: layout.split,
            height,
        });
    }
}

pub fn on_slot_drag(
    ev: On<PointerDrag>,
    drags: Query<&SlotDrag>,
    theme: Res<Theme>,
    scale: Res<UiScale>,
    mut layout: ResMut<ShellLayout>,
) {
    if let Ok(d) = drags.get(ev.entity) {
        // The slots share the height less the splitter; the pointer moves
        // in window pixels, the height is in UI pixels.
        let h = d.height - theme.s.viewport_divider_hit;
        let split = ShellLayout::split_after(d.split, ev.distance.y / scale.0, h);
        if layout.split != split {
            layout.split = split;
        }
    }
}

pub fn on_slot_drag_end(
    ev: On<PointerDragEnd>,
    drags: Query<(), With<SlotDrag>>,
    mut commands: Commands,
) {
    if drags.contains(ev.entity) {
        commands.entity(ev.entity).remove::<SlotDrag>();
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_slot_button)
        .add_observer(on_slot_drag_start)
        .add_observer(on_slot_drag)
        .add_observer(on_slot_drag_end);
}
