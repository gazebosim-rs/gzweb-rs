//! A panel's body (spec §3.3.1): its content once ready, which the
//! panel's own observer builds under its marker, full-bleed; else skeleton
//! rows, or the centred icon, message and action buttons, padded. And the
//! action buttons, in the body and in the header, which trigger
//! [`PanelActionActivated`] (DECISIONS.md D-58).

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzReconnect;

use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::panels::{
    ActionId, PanelAction, PanelActionActivated, PanelBody, PanelDef, PanelId, PanelView, RECONNECT,
};
use crate::theme::{radius, ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, row};
use crate::widgets::{button, rounded, ButtonVariant, Tooltip};

/// The view a body was built for: it is rebuilt when its panel's view
/// differs.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct BodyView(pub PanelView);

/// A skeleton row (§3.3.1 loading: `hover` fill).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SkeletonRow;

/// A panel action's button, in a body or a header.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActionButton {
    pub panel: PanelId,
    pub action: ActionId,
}

/// What a disabled action's tooltip says.
pub(crate) const NOT_YET: &str = "Not available yet";

/// Spawns `def`'s body in `view` under `slot`: empty and full-bleed when
/// ready (with the panel's content marker, for its observer to fill),
/// padded otherwise.
pub(super) fn spawn_body(
    commands: &mut Commands,
    slot: Entity,
    theme: &Theme,
    def: &PanelDef,
    view: PanelView,
) -> Entity {
    let centred = matches!(view, PanelView::Empty { .. } | PanelView::Error { .. });
    let mut body = commands.spawn((
        PanelBody(def.id),
        BodyView(view.clone()),
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            padding: match view {
                PanelView::Ready => UiRect::ZERO,
                _ => UiRect::all(px(SPACE[4])),
            },
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
            ..column(if centred { SPACE[5] } else { SPACE[1] })
        },
        ChildOf(slot),
    ));
    let t = theme.clone();
    let panel = def.id;
    let glyph = match view {
        PanelView::Error { .. } => IconId::Warning,
        _ => def.icon,
    };
    match view {
        PanelView::Ready => {
            if let Some(mark) = def.content {
                mark(&mut body);
            }
        }
        PanelView::Loading => {
            body.insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
                for width in [72.0, 56.0, 64.0] {
                    b.spawn(Node {
                        height: px(t.s.tree_row_h),
                        padding: UiRect::horizontal(px(SPACE[3])),
                        ..row(0.0)
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
            })));
        }
        PanelView::Empty { message, actions } | PanelView::Error { message, actions } => {
            body.insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
                b.spawn(icon(glyph, 24.0, ColorRole::Text3));
                b.spawn((
                    text(TypeStyle::BODY, ColorRole::Text2, message),
                    TextLayout::justify(Justify::Center),
                ));
                if !actions.is_empty() {
                    b.spawn(row(SPACE[4])).with_children(|row| {
                        for a in actions {
                            spawn_action(row, &t, panel, a);
                        }
                    });
                }
            })));
        }
    }
    body.id()
}

/// An empty or error state's button: dimmed with "Not available yet" when
/// disabled.
fn spawn_action(row: &mut ChildSpawner, theme: &Theme, panel: PanelId, a: PanelAction) {
    let mut e = row.spawn((
        button(theme, a.label.clone(), ButtonVariant::Normal),
        ActionButton {
            panel,
            action: a.id,
        },
    ));
    if !a.enabled {
        e.insert((InteractionDisabled, Tooltip::new(a.label).detail(NOT_YET)));
    }
}

/// An enabled action button triggers [`PanelActionActivated`] for its
/// panel to observe.
pub(crate) fn on_action_button(
    ev: On<Activate>,
    buttons: Query<(&ActionButton, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if let Ok((b, false)) = buttons.get(ev.entity) {
        commands.trigger(PanelActionActivated {
            entity: ev.entity,
            panel: b.panel,
            action: b.action,
        });
    }
}

/// The shell's own action: Reconnect retries the server now, from any
/// panel.
pub(crate) fn on_reconnect(
    ev: On<PanelActionActivated>,
    mut reconnect: MessageWriter<GzReconnect>,
) {
    if ev.action == RECONNECT {
        crate::shell::connection::retry(&mut reconnect);
    }
}
