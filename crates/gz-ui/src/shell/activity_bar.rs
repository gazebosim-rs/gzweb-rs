//! The activity bar (spec §3.2): Scene and Assets, a separator, Topics,
//! Plot, Sensors, Teleop and Console, a spacer, then Settings. A click
//! opens the panel in the sidebar's primary slot, Shift+click in the
//! secondary; clicking the active button collapses the sidebar.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::{region, Panel, Region, ShellLayout, UiCommand};
use crate::icons::IconId;
use crate::theme::{border, ColorRole, Fill, Theme, SPACE};
use crate::visuals::Active;
use crate::widgets::badge::Badge;
use crate::widgets::menu::shortcut_label;
use crate::widgets::{activity_button, spacer, BadgeKind, Tooltip};

/// An activity button's panel.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityFor(pub Panel);

/// The Settings button (opens the Settings dialog later; for now it
/// switches the theme, DECISIONS.md).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SettingsButton;

/// Badge counts (§3.2): active charts on Plot, unread warnings and errors
/// on Console. Zero hides the badge. Filled by the panels (M7).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivityBadges {
    pub plot: u32,
    pub console_warnings: u32,
    pub console_errors: u32,
}

impl ActivityBadges {
    /// The badge for `panel`: kind and count, if it shows.
    pub fn badge(&self, panel: Panel) -> Option<(BadgeKind, u32)> {
        match panel {
            Panel::Plot if self.plot > 0 => Some((BadgeKind::Count, self.plot)),
            Panel::Console if self.console_errors > 0 => Some((
                BadgeKind::Error,
                self.console_errors + self.console_warnings,
            )),
            Panel::Console if self.console_warnings > 0 => {
                Some((BadgeKind::Warning, self.console_warnings))
            }
            _ => None,
        }
    }
}

fn activity_separator() -> impl Bundle {
    (
        Node {
            width: px(24),
            height: px(1),
            flex_shrink: 0.0,
            margin: UiRect::vertical(px(SPACE[3])),
            ..default()
        },
        Fill(ColorRole::Line),
    )
}

/// The bar.
pub fn activity_bar(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        region(
            Region::ActivityBar,
            Node {
                width: px(theme.s.activity_bar_w),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(SPACE[1]),
                padding: UiRect::vertical(px(SPACE[4])),
                border: UiRect::right(px(border::HAIRLINE)),
                ..default()
            },
            ColorRole::Bg,
        ),
        Children::spawn(SpawnWith(move |bar: &mut ChildSpawner| {
            for panel in Panel::ALL {
                let tip = Tooltip::new(panel.title())
                    .shortcut_opt(panel.binding().and_then(|id| shortcut_label(&map, id)))
                    .detail("Shift+click: second slot");
                // Badged buttons get a hidden badge for `badge_states`.
                let badge =
                    matches!(panel, Panel::Plot | Panel::Console).then_some((BadgeKind::Count, 0));
                bar.spawn((
                    activity_button(&t, panel.icon(), tip, badge),
                    ActivityFor(panel),
                ));
                if panel == Panel::Assets {
                    bar.spawn(activity_separator());
                }
            }
            bar.spawn(spacer());
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
pub fn on_activity(
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

/// Active state per §3.2's table, and the badges.
#[allow(clippy::type_complexity)]
pub fn activity_states(
    layout: Res<ShellLayout>,
    badges: Res<ActivityBadges>,
    buttons: Query<(Entity, &ActivityFor, Has<Active>, &Children)>,
    mut badge_nodes: Query<(&mut Node, &mut Fill, &Children), With<Badge>>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    for (e, a, active, children) in &buttons {
        let want = layout.is_active(a.0);
        if want != active {
            if want {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
        let badge = badges.badge(a.0);
        for &c in children {
            let Ok((mut node, mut fill, kids)) = badge_nodes.get_mut(c) else {
                continue;
            };
            let display = if badge.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
            if let Some((kind, count)) = badge {
                fill.set_if_neq(Fill(kind.fill()));
                for &k in kids {
                    if let Ok(mut t) = texts.get_mut(k) {
                        let s = count.to_string();
                        if t.0 != s {
                            t.0 = s;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<ActivityBadges>()
        .add_observer(on_activity);
}
