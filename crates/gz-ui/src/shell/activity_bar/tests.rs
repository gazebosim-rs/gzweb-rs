//! The activity bar: open, collapse and the second slot, the active look
//! and AccessKit state, badges, and Settings.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{ActivityFor, SettingsButton};
use crate::panels::{ConsoleUnread, PanelId, PlotCharts, ASSETS, CONSOLE, PLOT, SCENE, TOPICS};
use crate::shell::sidebar::SlotView;
use crate::shell::PanelStatus;
use crate::shell::{ShellLayout, Side, Slot};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::BadgeKind;

#[test]
fn activity_buttons_open_collapse_and_use_the_second_slot() {
    let mut l = ShellLayout::for_width(1440.0, Some(SCENE));
    assert!(l.is_active(SCENE));
    // The active one collapses the sidebar and stays selected; another
    // click brings it back.
    l.activity_click(SCENE, false);
    assert!(!l.sidebar_open && l.is_active(SCENE));
    l.activity_click(SCENE, false);
    assert!(l.sidebar_open && l.is_active(SCENE));
    // Collapsed, a different panel expands the sidebar with that panel.
    l.activity_click(SCENE, false);
    l.activity_click(ASSETS, false);
    assert!(l.sidebar_open && l.primary == Some(ASSETS));
    l.activity_click(SCENE, false);
    // An inactive one replaces the primary.
    l.activity_click(TOPICS, false);
    assert_eq!((l.primary, l.secondary), (Some(TOPICS), None));
    // Shift: the secondary.
    l.activity_click(PLOT, true);
    assert_eq!((l.primary, l.secondary), (Some(TOPICS), Some(PLOT)));
    assert!(l.is_active(TOPICS) && l.is_active(PLOT));
    // A panel already in the other slot swaps there.
    l.activity_click(TOPICS, true);
    assert_eq!((l.primary, l.secondary), (Some(PLOT), Some(TOPICS)));
    l.open_in(TOPICS, Slot::Primary);
    assert_eq!((l.primary, l.secondary), (Some(TOPICS), Some(PLOT)));
    l.swap_slots();
    assert_eq!((l.primary, l.secondary), (Some(PLOT), Some(TOPICS)));
    l.close_secondary();
    assert_eq!((l.primary, l.secondary), (Some(PLOT), None));
    // Shift on the only panel keeps it in the primary.
    l.activity_click(PLOT, true);
    assert_eq!((l.primary, l.secondary), (Some(PLOT), None));
    l.swap_slots();
    assert_eq!((l.primary, l.secondary), (Some(PLOT), None));
    // A collapsed sidebar reopens with a click on a panel it holds.
    l.show(Side::Sidebar, false);
    l.activity_click(PLOT, false);
    assert!(l.sidebar_open);

    // Through the buttons.
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let world = app.world_mut();
    let buttons: Vec<(Entity, PanelId)> = world
        .query::<(Entity, &ActivityFor)>()
        .iter(world)
        .map(|(e, a)| (e, a.0))
        .collect();
    let button = |p: PanelId| buttons.iter().find(|b| b.1 == p).unwrap().0;
    let active = |app: &App, p: PanelId| app.world().get::<Active>(button(p)).is_some();
    assert!(active(&app, SCENE) && !active(&app, TOPICS));
    app.world_mut().trigger(Activate {
        entity: button(TOPICS),
    });
    frames(&mut app, 2);
    assert!(active(&app, TOPICS) && !active(&app, SCENE));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    app.world_mut().trigger(Activate {
        entity: button(CONSOLE),
    });
    frames(&mut app, 2);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    let l = layout(&app);
    assert_eq!((l.primary, l.secondary), (Some(TOPICS), Some(CONSOLE)));
    assert!(active(&app, TOPICS) && active(&app, CONSOLE));
    // Both slots are laid out, 55/45, with the row splitter between.
    let world = app.world_mut();
    let slots: Vec<(Slot, Entity)> = world
        .query::<(&SlotView, Entity)>()
        .iter(world)
        .map(|(s, e)| (s.0, e))
        .collect();
    let h = |s: Slot| rect(&app, slots.iter().find(|x| x.0 == s).unwrap().1).1.y;
    let (p, s) = (h(Slot::Primary), h(Slot::Secondary));
    assert!((p + s + 6.0 - 828.0).abs() < 0.5, "{p} {s}");
    assert!((p / (p + s) - 0.55).abs() < 0.01, "{p} {s}");
    // Clicking the active one collapses the sidebar; both stay selected,
    // and pressed to AccessKit.
    app.world_mut().trigger(Activate {
        entity: button(TOPICS),
    });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
    assert!(active(&app, TOPICS) && active(&app, CONSOLE));
    let toggled = |app: &App, p: PanelId| {
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(button(p))
            .unwrap()
            .toggled()
    };
    assert_eq!(toggled(&app, TOPICS), Some(accesskit::Toggled::True));
    assert_eq!(toggled(&app, SCENE), Some(accesskit::Toggled::False));
    // Clicked again, it expands.
    app.world_mut().trigger(Activate {
        entity: button(TOPICS),
    });
    frames(&mut app, 2);
    assert!(layout(&app).sidebar_open);
    // Badges show what their panels set, and hide without one: Plot's
    // charts, Console's unread messages, any panel's status.
    let badge = |app: &App, p: PanelId| {
        descendants(app.world(), button(p))
            .into_iter()
            .find(|e| {
                app.world()
                    .get::<crate::widgets::badge::Badge>(*e)
                    .is_some()
            })
            .unwrap()
    };
    assert!(!displayed(&app, badge(&app, PLOT)));
    app.world_mut().resource_mut::<PlotCharts>().0 = 2;
    app.world_mut().resource_mut::<ConsoleUnread>().errors = 1;
    frames(&mut app, 2);
    assert!(displayed(&app, badge(&app, PLOT)));
    assert!(displayed(&app, badge(&app, CONSOLE)));
    assert!(all_text(&mut app).contains('2'));
    app.world_mut()
        .resource_mut::<PanelStatus>()
        .set_badge(TOPICS, Some((BadgeKind::Count, 7)));
    frames(&mut app, 1);
    assert!(displayed(&app, badge(&app, TOPICS)));
    app.world_mut().resource_mut::<PlotCharts>().0 = 0;
    frames(&mut app, 2);
    assert!(!displayed(&app, badge(&app, PLOT)));
    // Settings switches the theme (until the Settings dialog exists).
    let settings = find::<SettingsButton>(&mut app);
    app.world_mut().trigger(Activate { entity: settings });
    frames(&mut app, 1);
    assert!(app.world().resource::<Theme>().is_dark);
}
