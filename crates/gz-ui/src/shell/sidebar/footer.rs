//! A panel's footer (spec §3.3; DECISIONS.md D-98): under the body in
//! every state, a top border in `line`, padding 10 14 and a row of
//! equal-width buttons. The shell builds the panel's
//! [`PanelAction`](crate::panels::PanelAction)s into it, then marks it
//! with the panel's footer content marker, if any, for the panel's own
//! observer to fill.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::body::{ActionButton, NOT_YET};
use crate::panels::{PanelDef, PanelId};
use crate::shell::PanelStatus;
use crate::theme::{border, radius, ColorRole, Stroke, Theme, SPACE};
use crate::widgets::layout::row;
use crate::widgets::{button, rounded, ButtonVariant, Tooltip};

/// A host's footer, for `panel`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanelFooter(pub PanelId);

/// A footer action's button (with its [`ActionButton`]): its enabled
/// state follows [`PanelStatus`] in place, like a header action's.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct FooterButton;

/// The footer row (§3.3).
pub(crate) fn footer_bar() -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(px(SPACE[7]), px(SPACE[5])),
            border: UiRect::top(px(border::HAIRLINE)),
            flex_shrink: 0.0,
            ..row(SPACE[3])
        },
        Stroke(ColorRole::Line),
    )
}

/// Spawns a footer button in `row`: `label`, an equal share of the row,
/// `button_h` tall.
pub(crate) fn footer_button<'a>(
    row: &'a mut ChildSpawnerCommands,
    theme: &Theme,
    label: impl Into<String>,
) -> EntityCommands<'a> {
    let mut b = row.spawn(button(theme, label, ButtonVariant::Normal));
    b.insert(Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        height: px(theme.s.button_h),
        padding: UiRect::horizontal(px(SPACE[4])),
        border: UiRect::all(px(border::HAIRLINE)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: rounded(radius::SM),
        ..default()
    });
    b
}

/// A disabled action's tooltip: its label, and why it is unavailable.
pub(crate) fn action_tooltip(label: &str, enabled: bool, why: Option<&'static str>) -> Tooltip {
    let tip = Tooltip::new(label.to_string());
    if enabled {
        tip
    } else {
        tip.detail(why.unwrap_or(NOT_YET))
    }
}

/// Spawns `def`'s footer, if it has one: its actions' buttons, enabled as
/// its status says, then its content marker.
pub(super) fn spawn_footer(
    commands: &mut Commands,
    theme: &Theme,
    status: &PanelStatus,
    def: &PanelDef,
) -> Option<Entity> {
    if !def.has_footer() {
        return None;
    }
    let mut footer = commands.spawn((footer_bar(), PanelFooter(def.id)));
    footer.with_children(|f| {
        for a in &def.footer {
            let enabled = status.action_enabled_or(def.id, a.id, a.enabled);
            let label = a.label.to_string();
            let mut b = footer_button(f, theme, label.clone());
            b.insert((
                ActionButton {
                    panel: def.id,
                    action: a.id,
                },
                FooterButton,
                action_tooltip(&label, enabled, a.why),
            ));
            if !enabled {
                b.insert(InteractionDisabled);
            }
        }
    });
    if let Some(mark) = def.footer_content {
        mark(&mut footer);
    }
    Some(footer.id())
}
