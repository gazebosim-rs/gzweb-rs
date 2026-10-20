use bevy::prelude::*;

use super::{Panel, PanelAction, PanelDef, PanelRegistry, PanelView, PanelsPlugin};
use crate::assets::icons::IconId;
use crate::shell::connection::Phase;
use crate::shell::ActivityBadges;
use crate::widgets::BadgeKind;

fn registry() -> PanelRegistry {
    let mut app = App::new();
    app.add_plugins(PanelsPlugin);
    app.world().resource::<PanelRegistry>().clone()
}

#[test]
fn every_panel_registers_once_in_activity_bar_order() {
    let r = registry();
    let ids: Vec<Panel> = r.iter().map(|p| p.id).collect();
    assert_eq!(ids, Panel::ALL);
    for p in r.iter() {
        assert!(!p.title.is_empty());
        let _ = (p.view)(Phase::Live);
        assert_eq!(r.get(p.id).map(|d| d.title), Some(p.title));
    }
    // The separator follows Assets; the shortcuts open their panels.
    let separated: Vec<Panel> = r
        .iter()
        .filter(|p| p.separator_after)
        .map(|p| p.id)
        .collect();
    assert_eq!(separated, [Panel::Assets]);
    assert_eq!(r.for_binding("panel.console"), Some(Panel::Console));
    assert_eq!(r.for_binding("panel.teleop"), None);
    assert_eq!(r.get(Panel::Scene).unwrap().icon, IconId::ActivityScene);
    // Only Plot and Console have badges.
    let badged: Vec<Panel> = r
        .iter()
        .filter(|p| p.badge.is_some())
        .map(|p| p.id)
        .collect();
    assert_eq!(badged, [Panel::Plot, Panel::Console]);
    let b = ActivityBadges {
        plot: 4,
        ..default()
    };
    assert_eq!(r.badge(Panel::Plot, &b), Some((BadgeKind::Count, 4)));
    assert_eq!(r.badge(Panel::Scene, &b), None);
    // Panels without a count have none.
    assert_eq!((r.get(Panel::Plot).unwrap().count)(Phase::Live, None), None);
}

#[test]
fn registering_again_replaces_and_order_sorts() {
    let mut r = PanelRegistry::default();
    r.register(PanelDef::new(
        Panel::Console,
        "Console",
        IconId::ActivityConsole,
        60,
    ));
    r.register(PanelDef::new(
        Panel::Scene,
        "Scene",
        IconId::ActivityScene,
        0,
    ));
    r.register(PanelDef::new(Panel::Plot, "Plot", IconId::ActivityPlot, 30));
    r.register(PanelDef::new(
        Panel::Scene,
        "World",
        IconId::ActivityScene,
        0,
    ));
    let titles: Vec<&str> = r.iter().map(|p| p.title).collect();
    assert_eq!(titles, ["World", "Plot", "Console"]);
    // The defaults: ready, no count, badge, binding or content.
    let d = r.get(Panel::Plot).unwrap();
    assert_eq!((d.view)(Phase::Idle), PanelView::Ready);
    assert!(d.binding.is_none() && d.badge.is_none() && d.content.is_none());
}

#[test]
fn panel_actions_say_what_they_do() {
    for a in [
        PanelAction::Reconnect,
        PanelAction::OpenWorld,
        PanelAction::NewWorld,
        PanelAction::AddField,
    ] {
        assert_eq!(a.available(), a == PanelAction::Reconnect);
        assert!(!a.label().is_empty());
    }
}
