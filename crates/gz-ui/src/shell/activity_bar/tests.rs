//! The activity bar: open, collapse and the second slot, the active look
//! and AccessKit state, badges, and Settings.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{ActivityBadges, ActivityFor, SettingsButton};
use crate::shell::sidebar::SlotView;
use crate::shell::{Panel, ShellLayout, Side, Slot};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::Theme;

#[test]
fn activity_buttons_open_collapse_and_use_the_second_slot() {
    let mut l = ShellLayout::for_width(1440.0);
    assert!(l.is_active(Panel::Scene));
    // The active one collapses the sidebar and stays selected; another
    // click brings it back.
    l.activity_click(Panel::Scene, false);
    assert!(!l.sidebar_open && l.is_active(Panel::Scene));
    l.activity_click(Panel::Scene, false);
    assert!(l.sidebar_open && l.is_active(Panel::Scene));
    // Collapsed, a different panel expands the sidebar with that panel.
    l.activity_click(Panel::Scene, false);
    l.activity_click(Panel::Assets, false);
    assert!(l.sidebar_open && l.primary == Some(Panel::Assets));
    l.activity_click(Panel::Scene, false);
    // An inactive one replaces the primary.
    l.activity_click(Panel::Topics, false);
    assert_eq!((l.primary, l.secondary), (Some(Panel::Topics), None));
    // Shift: the secondary.
    l.activity_click(Panel::Plot, true);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Plot))
    );
    assert!(l.is_active(Panel::Topics) && l.is_active(Panel::Plot));
    // A panel already in the other slot swaps there.
    l.activity_click(Panel::Topics, true);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Topics))
    );
    l.open_in(Panel::Topics, Slot::Primary);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Plot))
    );
    l.swap_slots();
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Topics))
    );
    l.close_secondary();
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    // Shift on the only panel keeps it in the primary.
    l.activity_click(Panel::Plot, true);
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    l.swap_slots();
    assert_eq!((l.primary, l.secondary), (Some(Panel::Plot), None));
    // A collapsed sidebar reopens with a click on a panel it holds.
    l.show(Side::Sidebar, false);
    l.activity_click(Panel::Plot, false);
    assert!(l.sidebar_open);

    // Through the buttons.
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 3);
    let world = app.world_mut();
    let buttons: Vec<(Entity, Panel)> = world
        .query::<(Entity, &ActivityFor)>()
        .iter(world)
        .map(|(e, a)| (e, a.0))
        .collect();
    let button = |p: Panel| buttons.iter().find(|b| b.1 == p).unwrap().0;
    let active = |app: &App, p: Panel| app.world().get::<Active>(button(p)).is_some();
    assert!(active(&app, Panel::Scene) && !active(&app, Panel::Topics));
    app.world_mut().trigger(Activate {
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(active(&app, Panel::Topics) && !active(&app, Panel::Scene));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    app.world_mut().trigger(Activate {
        entity: button(Panel::Console),
    });
    frames(&mut app, 2);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    let l = layout(&app);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Topics), Some(Panel::Console))
    );
    assert!(active(&app, Panel::Topics) && active(&app, Panel::Console));
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
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
    assert!(active(&app, Panel::Topics) && active(&app, Panel::Console));
    let toggled = |app: &App, p: Panel| {
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(button(p))
            .unwrap()
            .toggled()
    };
    assert_eq!(toggled(&app, Panel::Topics), Some(accesskit::Toggled::True));
    assert_eq!(toggled(&app, Panel::Scene), Some(accesskit::Toggled::False));
    // Clicked again, it expands.
    app.world_mut().trigger(Activate {
        entity: button(Panel::Topics),
    });
    frames(&mut app, 2);
    assert!(layout(&app).sidebar_open);
    // Badges show their counts and hide at zero.
    app.world_mut().resource_mut::<ActivityBadges>().plot = 2;
    frames(&mut app, 1);
    let plot = button(Panel::Plot);
    let badge = descendants(app.world(), plot)
        .into_iter()
        .find(|e| {
            app.world()
                .get::<crate::widgets::badge::Badge>(*e)
                .is_some()
        })
        .unwrap();
    assert!(displayed(&app, badge));
    // Settings switches the theme (until the Settings dialog exists).
    let settings = find::<SettingsButton>(&mut app);
    app.world_mut().trigger(Activate { entity: settings });
    frames(&mut app, 1);
    assert!(app.world().resource::<Theme>().is_dark);
}
