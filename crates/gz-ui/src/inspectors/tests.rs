use bevy::prelude::*;

use super::{InspectorDef, InspectorKind, InspectorRegistry, InspectorsPlugin};

#[test]
fn the_world_inspector_shows_with_nothing_selected() {
    let mut app = App::new();
    app.add_plugins(InspectorsPlugin);
    let r = app.world().resource::<InspectorRegistry>().clone();
    assert_eq!(
        r.for_selection().map(|i| i.kind),
        Some(InspectorKind::World)
    );
    // Registering a kind again replaces it.
    let mut r = InspectorRegistry::default();
    assert!(r.for_selection().is_none());
    let body: super::BodyFn = |_, _| {};
    r.register(InspectorDef {
        kind: InspectorKind::World,
        body,
    });
    r.register(InspectorDef {
        kind: InspectorKind::World,
        body,
    });
    assert_eq!(r.inspectors.len(), 1);
}
