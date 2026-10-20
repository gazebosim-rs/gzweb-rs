//! A panel's body in its empty, loading or error state (spec §3.3.1):
//! what each panel shows in each connection phase, skeleton rows, and the
//! centred icon, message and action buttons.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzReconnect;
use gz_bevy::spawn::GzIndex;

use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::shell::connection::Phase;
use crate::shell::Panel;
use crate::theme::{radius, ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::{button, rounded, ButtonVariant, Tooltip};

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

/// Marks a panel body.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelBody(pub Panel);

/// A skeleton row (§3.3.1 loading: `hover` fill).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SkeletonRow;

pub(super) fn panel_body(theme: &Theme, panel: Panel, view: PanelView) -> impl Bundle {
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

/// An empty or error state's Reconnect retries the server now.
pub fn on_panel_action(
    ev: On<Activate>,
    actions: Query<(&PanelAction, Has<InteractionDisabled>)>,
    mut reconnect: MessageWriter<GzReconnect>,
) {
    if let Ok((PanelAction::Reconnect, false)) = actions.get(ev.entity) {
        crate::shell::connection::retry(&mut reconnect);
    }
}
