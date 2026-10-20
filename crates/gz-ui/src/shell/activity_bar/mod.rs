//! The activity bar (spec §3.2): Scene and Assets, a separator, Topics,
//! Plot, Sensors, Teleop and Console, a spacer, then Settings. A click
//! opens the panel in the sidebar's primary slot, Shift+click in the
//! secondary; clicking the active button collapses the sidebar.

use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::regions::{region, Region};
use super::{Panel, ShellLayout, ShellSystems, UiCommand};
use crate::assets::icons::IconId;
use crate::panels::PanelRegistry;
use crate::theme::visuals::Active;
use crate::theme::{border, ColorRole, Fill, Theme, SPACE};
use crate::widgets::badge::Badge;
use crate::widgets::layout::{column, flex_spacer, separator, set_shown, Orientation};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::{activity_button, BadgeKind, Tooltip};

/// An activity button's panel.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityFor(pub Panel);

/// The Settings button (opens the Settings dialog later; for now it
/// switches the theme, DECISIONS.md).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SettingsButton;

/// Badge counts (§3.2): active charts on Plot, unread warnings and errors
/// on Console. Filled by the panels (M7); each panel's
/// [`PanelDef::badge`](crate::panels::PanelDef::badge) says what its
/// button shows of them (zero hides the badge).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivityBadges {
    pub plot: u32,
    pub console_warnings: u32,
    pub console_errors: u32,
}

/// The length of the separator after Assets.
const SEPARATOR_W: f32 = 24.0;

/// The bar, with a button for each registered panel.
pub fn activity_bar(theme: &Theme, map: &InputMap, panels: &PanelRegistry) -> impl Bundle {
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
            for panel in panels.iter() {
                let tip = Tooltip::new(panel.title)
                    .shortcut_opt(panel.binding.and_then(|id| shortcut_label(&map, id)))
                    .detail("Shift+click: second slot");
                // Badged buttons get a hidden badge for `activity_badges`.
                let badge = panel.badge.is_some().then_some((BadgeKind::Count, 0));
                bar.spawn((
                    activity_button(&t, panel.icon, tip, badge),
                    ActivityFor(panel.id),
                ));
                if panel.separator_after {
                    bar.spawn(separator(Orientation::Horizontal, SEPARATOR_W, SPACE[3]));
                }
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

/// Active state per §3.2's table: a button is active while its panel is
/// in either slot, and pressed to AccessKit.
pub fn activity_active(
    layout: Res<ShellLayout>,
    mut buttons: Query<(Entity, &ActivityFor, Has<Active>, &mut AccessibilityNode)>,
    mut commands: Commands,
) {
    for (e, a, active, mut access) in &mut buttons {
        let want = layout.is_active(a.0);
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

/// The badges show their counts and hide at zero (§3.2).
pub fn activity_badges(
    badges: Res<ActivityBadges>,
    panels: Res<PanelRegistry>,
    buttons: Query<(&ActivityFor, &Children)>,
    mut badge_nodes: Query<(&mut Node, &mut Fill, &Children), With<Badge>>,
    mut texts: Query<&mut Text>,
) {
    for (a, children) in &buttons {
        let badge = panels.badge(a.0, &badges);
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
    app.init_resource::<ActivityBadges>()
        .add_systems(
            Update,
            (activity_active, activity_badges).in_set(ShellSystems::Readouts),
        )
        .add_observer(on_activity);
}
