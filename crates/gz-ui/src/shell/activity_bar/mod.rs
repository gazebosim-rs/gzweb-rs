//! The activity bar (spec §3.2): a button for each registered panel, by
//! group and order with a separator between groups, a spacer, then
//! Settings. Each button's badge is what its panel wrote into
//! [`PanelStatus`]. A click opens the panel in the sidebar's primary slot,
//! Shift+click in the secondary; clicking the active button collapses the
//! sidebar.

use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::regions::{region, Region};
use super::{PanelStatus, ShellSystems, ShownPanels, UiCommand};
use crate::assets::icons::IconId;
use crate::panels::{PanelId, PanelRegistry};
use crate::theme::visuals::Active;
use crate::theme::{border, ColorRole, Fill, Theme, SPACE};
use crate::widgets::badge::Badge;
use crate::widgets::layout::{column, flex_spacer, separator, set_shown, Orientation};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::{activity_button, BadgeKind, Tooltip};

/// An activity button's panel.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivityFor(pub PanelId);

/// The Settings button (opens the Settings dialog later; for now it
/// switches the theme, DECISIONS.md).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SettingsButton;

/// The length of the separators between the groups.
const SEPARATOR_W: f32 = 24.0;

/// The bar, with a button for each registered panel.
pub(crate) fn activity_bar(theme: &Theme, map: &InputMap, panels: &PanelRegistry) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    let panels = panels.clone();
    (
        region(
            Region::ActivityBar,
            Node {
                width: px(theme.s.activity_bar_w),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::vertical(px(SPACE[4])),
                border: UiRect::right(px(border::HAIRLINE)),
                ..column(SPACE[1])
            },
            ColorRole::Bg,
        ),
        Children::spawn(SpawnWith(move |bar: &mut ChildSpawner| {
            let mut group = None;
            for panel in panels.iter() {
                if group.is_some_and(|g| g != panel.group) {
                    bar.spawn(separator(Orientation::Horizontal, SEPARATOR_W, SPACE[3]));
                }
                group = Some(panel.group);
                let tip = Tooltip::new(panel.title)
                    .shortcut_opt(panel.binding.and_then(|id| shortcut_label(&map, id)))
                    .detail("Shift+click: second slot");
                // Every button has a badge, hidden until its panel sets one
                // (`activity_badges`).
                bar.spawn((
                    activity_button(&t, panel.icon, tip, Some((BadgeKind::Count, 0))),
                    ActivityFor(panel.id),
                ));
            }
            bar.spawn(flex_spacer());
            bar.spawn((
                activity_button(
                    &t,
                    IconId::Settings,
                    Tooltip::new("Settings").detail("Switches the theme for now"),
                    None,
                ),
                SettingsButton,
            ));
        })),
    )
}

/// Activity clicks (Shift from the keyboard state) and Settings.
pub(crate) fn on_activity(
    ev: On<Activate>,
    buttons: Query<&ActivityFor>,
    settings: Query<(), With<SettingsButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(a) = buttons.get(ev.entity) {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        out.write(UiCommand::Activity { panel: a.0, shift });
    } else if settings.contains(ev.entity) {
        out.write(UiCommand::ToggleTheme);
    }
}

/// Active state per §3.2's table: a button is active while a host holds
/// its panel (a slot, the sidebar open or collapsed), and pressed to
/// AccessKit.
pub(crate) fn activity_active(
    hosts: Res<ShownPanels>,
    mut buttons: Query<(Entity, &ActivityFor, Has<Active>, &mut AccessibilityNode)>,
    mut commands: Commands,
) {
    if !hosts.is_changed() {
        return;
    }
    for (e, a, active, mut access) in &mut buttons {
        let want = hosts.is_hosted(a.0);
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
        // A toggle button to AccessKit: pressed while its panel is shown
        // (or collapsed with the sidebar).
        let toggled = if want {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        };
        if access.toggled() != Some(toggled) {
            access.set_toggled(toggled);
        }
    }
}

/// The badges show what their panels set, and hide without one (§3.2).
pub(crate) fn activity_badges(
    status: Res<PanelStatus>,
    buttons: Query<(&ActivityFor, &Children)>,
    mut badge_nodes: Query<(&mut Node, &mut Fill, &Children), With<Badge>>,
    mut texts: Query<&mut Text>,
) {
    if !status.is_changed() {
        return;
    }
    for (a, children) in &buttons {
        let badge = status.badge(a.0);
        for &c in children {
            let Ok((mut node, mut fill, kids)) = badge_nodes.get_mut(c) else {
                continue;
            };
            set_shown(&mut node, badge.is_some());
            if let Some((kind, count)) = badge {
                fill.set_if_neq(Fill(kind.fill()));
                for &k in kids {
                    if let Ok(mut t) = texts.get_mut(k) {
                        set_text(&mut t, &count.to_string());
                    }
                }
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (activity_active, activity_badges).in_set(ShellSystems::Readouts),
    )
    .add_observer(on_activity);
}

#[cfg(test)]
mod tests;
