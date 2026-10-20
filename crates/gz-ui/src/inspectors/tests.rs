use bevy::prelude::*;

use super::{
    InspectorDef, InspectorId, InspectorPlugins, InspectorRegistry, RegisterInspector, WORLD,
};

#[test]
fn the_world_inspector_shows_with_nothing_selected() {
    let mut app = App::new();
    app.add_plugins(InspectorPlugins);
    let r = app.world().resource::<InspectorRegistry>().clone();
    assert_eq!(r.for_selection().map(|i| i.id), Some(WORLD));
    assert_eq!(WORLD.to_string(), WORLD.name());
    // Registering an id again replaces it.
    let mut r = InspectorRegistry::default();
    assert!(r.for_selection().is_none());
    let body: super::BodyFn = |_, _| {};
    r.register(InspectorDef { id: WORLD, body });
    r.register(InspectorDef { id: WORLD, body });
    r.register(InspectorDef {
        id: InspectorId::new("model"),
        body,
    });
    assert_eq!(r.inspectors.len(), 2);
}

#[test]
#[should_panic(expected = "the shell has already spawned")]
fn registering_after_the_shell_spawned_panics() {
    let mut app = App::new();
    app.init_resource::<InspectorRegistry>();
    app.world_mut().resource_mut::<InspectorRegistry>().close();
    let body: super::BodyFn = |_, _| {};
    app.register_inspector(InspectorDef { id: WORLD, body });
}
