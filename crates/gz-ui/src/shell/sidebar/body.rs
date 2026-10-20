//! A panel's body in its empty, loading or error state (spec §3.3.1):
//! skeleton rows, or the centred icon, message and action buttons, for
//! whatever [`PanelView`] its panel returns; its content once it's ready.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzReconnect;

use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::panels::{Panel, PanelAction, PanelDef, PanelView};
use crate::theme::{radius, ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, row};
use crate::widgets::{button, rounded, ButtonVariant, Tooltip};

/// Marks a panel body.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanelBody(pub Panel);

/// A skeleton row (§3.3.1 loading: `hover` fill).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SkeletonRow;

pub(super) fn panel_body(theme: &Theme, panel: &PanelDef, view: PanelView) -> impl Bundle {
    let t = theme.clone();
    let (id, icon_id, content) = (panel.id, panel.icon, panel.content);
    let centred = matches!(view, PanelView::Empty { .. } | PanelView::Error { .. });
    (
        PanelBody(id),
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            padding: UiRect::all(px(SPACE[4])),
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
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| match view {
            PanelView::Ready => {
                if let Some(content) = content {
                    content(b, &t);
                }
            }
            PanelView::Loading => {
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
            }
            PanelView::Empty { message, actions } | PanelView::Error { message, actions } => {
                let id = if actions.contains(&PanelAction::Reconnect) {
                    IconId::Warning
                } else {
                    icon_id
                };
                b.spawn(icon(id, 24.0, ColorRole::Text3));
                b.spawn((
                    text(TypeStyle::BODY, ColorRole::Text2, message),
                    TextLayout::justify(Justify::Center),
                ));
                if !actions.is_empty() {
                    b.spawn(row(SPACE[4])).with_children(|row| {
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
pub(crate) fn on_panel_action(
    ev: On<Activate>,
    actions: Query<(&PanelAction, Has<InteractionDisabled>)>,
    mut reconnect: MessageWriter<GzReconnect>,
) {
    if let Ok((PanelAction::Reconnect, false)) = actions.get(ev.entity) {
        crate::shell::connection::retry(&mut reconnect);
    }
}
