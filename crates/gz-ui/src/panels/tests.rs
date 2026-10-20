use bevy::prelude::*;

use super::{
    PanelAction, PanelDef, PanelId, PanelPlugins, PanelRegistry, PanelView, RegisterPanel,
    ScenePanelPlugin, ASSETS, CONSOLE, DATA_GROUP, PLOT, SCENE, SENSORS, TELEOP, TOPICS,
    WORLD_GROUP,
};
use crate::assets::IconId;
use crate::shell::Phase;

fn registry(plugins: impl PluginGroup) -> PanelRegistry {
    let mut app = App::new();
    app.add_plugins(plugins);
    app.world().resource::<PanelRegistry>().clone()
}

#[test]
fn every_panel_registers_once_in_activity_bar_order() {
    let r = registry(PanelPlugins);
    let ids: Vec<PanelId> = r.iter().map(|p| p.id).collect();
    assert_eq!(ids, [SCENE, ASSETS, TOPICS, PLOT, SENSORS, TELEOP, CONSOLE]);
    for p in r.iter() {
        assert!(!p.title.is_empty());
        let _ = (p.view)(Phase::Live);
        assert_eq!(r.get(p.id).map(|d| d.title), Some(p.title));
        assert_eq!(p.id.to_string(), p.id.name());
    }
    // Two groups: the world's panels, then the data panels.
    let groups: Vec<u32> = r.iter().map(|p| p.group).collect();
    assert_eq!(groups, [0, 0, 1, 1, 1, 1, 1]);
    // The shortcuts open their panels; Scene opens at start.
    assert_eq!(r.for_binding("panel.console"), Some(CONSOLE));
    assert_eq!(r.for_binding("panel.teleop"), None);
    assert_eq!(r.get(SCENE).unwrap().icon, IconId::ActivityScene);
    assert_eq!(r.default_open(), Some(SCENE));
}

/// An app leaves a built-in panel out by disabling its plugin, and puts
/// its own in its place by registering the same id.
#[test]
fn a_built_in_panel_can_be_left_out_or_replaced() {
    let r = registry(PanelPlugins.build().disable::<ScenePanelPlugin>());
    assert!(r.get(SCENE).is_none());
    // Without its default panel, the first in the bar opens at start.
    assert_eq!(r.default_open(), Some(ASSETS));
    struct MyScene;
    impl Plugin for MyScene {
        fn build(&self, app: &mut App) {
            app.register_panel(
                PanelDef::new(SCENE, "World", IconId::ActivityScene)
                    .place(WORLD_GROUP, 0)
                    .default_open(),
            );
        }
    }
    let mut app = App::new();
    app.add_plugins((PanelPlugins.build().disable::<ScenePanelPlugin>(), MyScene));
    let r = app.world().resource::<PanelRegistry>();
    assert_eq!(r.get(SCENE).unwrap().title, "World");
    assert_eq!(r.iter().next().unwrap().id, SCENE);
}

#[test]
fn groups_then_order_sort_and_unknown_ids_resolve_to_the_first() {
    let mut r = PanelRegistry::default();
    assert_eq!(r.default_open(), None);
    assert_eq!(r.resolve(SCENE), None);
    let def = |id, title, icon| PanelDef::new(id, title, icon);
    r.register(def(CONSOLE, "Console", IconId::ActivityConsole).place(DATA_GROUP, 60));
    r.register(
        def(PLOT, "Plot", IconId::ActivityPlot)
            .place(DATA_GROUP, 30)
            .binding("panel.plot"),
    );
    // A lower order in a later group still comes after the first group.
    r.register(
        def(SCENE, "World", IconId::ActivityScene)
            .place(WORLD_GROUP, 90)
            .binding("panel.plot"),
    );
    // Unplaced: after every placed panel.
    r.register(def(TOPICS, "Topics", IconId::ActivityTopics));
    let titles: Vec<&str> = r.iter().map(|p| p.title).collect();
    assert_eq!(titles, ["World", "Plot", "Console", "Topics"]);
    // Two panels on one binding: the first in the bar opens.
    assert_eq!(r.for_binding("panel.plot"), Some(SCENE));
    // An id nothing registered stands for the first panel.
    assert_eq!(r.resolve(PanelId::new("nope")), Some(SCENE));
    assert_eq!(r.resolve(PLOT), Some(PLOT));
    // The defaults: ready, closed at start, no binding or content.
    let d = r.get(CONSOLE).unwrap();
    assert_eq!((d.view)(Phase::Idle), PanelView::Ready);
    assert!(d.binding.is_none() && d.content.is_none() && !d.default_open);
    // Content is the panel's body once ready.
    let d = def(PLOT, "Plot", IconId::ActivityPlot).content(|_, _| {});
    assert!(d.content.is_some());
}

/// Replacing a built-in panel means disabling its plugin: registering its
/// id again would leave the first plugin's systems writing its state.
#[test]
#[should_panic(expected = "panel 'scene' is registered twice")]
fn registering_an_id_twice_panics() {
    let mut app = App::new();
    app.add_plugins(PanelPlugins);
    app.register_panel(PanelDef::new(SCENE, "World", IconId::ActivityScene));
}

#[test]
#[should_panic(expected = "the shell has already spawned")]
fn registering_after_the_shell_spawned_panics() {
    let mut app = App::new();
    app.init_resource::<PanelRegistry>();
    app.world_mut().resource_mut::<PanelRegistry>().close();
    app.register_panel(PanelDef::new(PLOT, "Plot", IconId::ActivityPlot));
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
