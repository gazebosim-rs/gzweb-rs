use bevy::prelude::*;

use super::{
    InspectKind, InspectorDef, InspectorId, InspectorPlugins, InspectorRegistry, RegisterInspector,
    WORLD,
};

#[test]
fn the_world_inspector_shows_with_nothing_selected() {
    let mut app = App::new();
    app.add_plugins(InspectorPlugins);
    let r = app.world().resource::<InspectorRegistry>().clone();
    assert_eq!(r.for_kind(InspectKind::World).map(|i| i.id), Some(WORLD));
    assert_eq!(WORLD.to_string(), WORLD.name());
    // Inspectors keep their registration order; the first for a kind wins.
    let mut r = InspectorRegistry::default();
    assert!(r.for_kind(InspectKind::World).is_none());
    r.register(InspectorDef::new(WORLD, InspectKind::World));
    r.register(InspectorDef::new(
        InspectorId::new("model"),
        InspectKind::Model,
    ));
    r.register(InspectorDef::new(
        InspectorId::new("model2"),
        InspectKind::Model,
    ));
    let ids: Vec<&str> = r.iter().map(|i| i.id.name()).collect();
    assert_eq!(ids, ["world", "model", "model2"]);
    assert_eq!(
        r.for_kind(InspectKind::Model).map(|i| i.id.name()),
        Some("model")
    );
    assert!(r.for_kind(InspectKind::Joint).is_none());
}

/// Replacing a built-in inspector means disabling its plugin: registering
/// its id again is a mistake.
#[test]
#[should_panic(expected = "inspector 'world' is registered twice")]
fn registering_an_id_twice_panics() {
    let mut app = App::new();
    app.add_plugins(InspectorPlugins);
    app.register_inspector(InspectorDef::new(WORLD, InspectKind::World));
}

#[test]
#[should_panic(expected = "the shell has already spawned")]
fn registering_after_the_shell_spawned_panics() {
    let mut app = App::new();
    app.init_resource::<InspectorRegistry>();
    app.world_mut().resource_mut::<InspectorRegistry>().close();
    app.register_inspector(InspectorDef::new(WORLD, InspectKind::World));
}

mod frame {
    use bevy::prelude::*;
    use bevy::ui_widgets::Activate;
    use gz_bevy::spawn::GzEntity;
    use gz_bevy::view::Selection;
    use gz_core::scene::NodeKind;

    use crate::inspectors::{InspectKind, Inspected};
    use crate::shell::inspector::{InspectorContent, InspectorHeader};
    use crate::test_support::frames;
    use crate::test_support::shell::*;

    fn spawn(app: &mut App, name: &str, kind: NodeKind) -> Entity {
        app.world_mut()
            .spawn((
                GzEntity {
                    name: name.into(),
                    id: 0,
                    kind,
                    path: name.into(),
                },
                Transform::default(),
                Visibility::default(),
            ))
            .id()
    }

    fn select(app: &mut App, entities: Vec<Entity>) {
        app.world_mut().resource_mut::<Selection>().entities = entities;
        frames(app, 2);
    }

    fn inspected(app: &App) -> Inspected {
        app.world().resource::<Inspected>().clone()
    }

    fn contents(app: &mut App) -> Vec<(Entity, InspectorContent)> {
        let world = app.world_mut();
        world
            .query::<(Entity, &InspectorContent)>()
            .iter(world)
            .map(|(e, c)| (e, c.clone()))
            .collect()
    }

    #[test]
    fn the_subject_follows_the_selection_and_the_pin() {
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let model = spawn(&mut app, "amr_01", NodeKind::Model);
        let link = spawn(&mut app, "base_link", NodeKind::Link);
        let world_body = contents(&mut app);
        assert_eq!(world_body.len(), 1);
        assert_eq!(
            world_body[0].1,
            InspectorContent(InspectKind::World, None, 0, None, None)
        );
        select(&mut app, vec![model]);
        let i = inspected(&app);
        assert_eq!((i.kind, i.entity), (InspectKind::Model, Some(model)));
        assert_eq!(i.path.as_deref(), Some("amr_01"));
        // One content at a time, rebuilt for the new subject.
        let c = contents(&mut app);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].1,
            InspectorContent(InspectKind::Model, Some(model), 0, None, None)
        );
        select(&mut app, vec![model, link]);
        assert_eq!(inspected(&app).kind, InspectKind::Multi);
        select(&mut app, vec![link]);
        assert_eq!(inspected(&app).kind, InspectKind::Link);
        // Pinned, it stays on the link.
        let pin = {
            let world = app.world_mut();
            world
                .query::<(Entity, &InspectorHeader)>()
                .iter(world)
                .find(|(_, h)| **h == InspectorHeader::Pin)
                .unwrap()
                .0
        };
        app.world_mut().trigger(Activate { entity: pin });
        frames(&mut app, 2);
        assert!(inspected(&app).pinned);
        select(&mut app, vec![model]);
        assert_eq!(inspected(&app).entity, Some(link));
        app.world_mut().trigger(Activate { entity: pin });
        frames(&mut app, 2);
        select(&mut app, vec![]);
        assert_eq!(inspected(&app), Inspected::default());
    }

    #[test]
    fn a_removed_subject_follows_its_respawn_or_shows_removed() {
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let model = spawn(&mut app, "amr_01", NodeKind::Model);
        select(&mut app, vec![model]);
        // Respawned at the same path (a scene rebuild): followed.
        app.world_mut().entity_mut(model).despawn();
        let again = spawn(&mut app, "amr_01", NodeKind::Model);
        frames(&mut app, 2);
        let i = inspected(&app);
        assert_eq!((i.entity, i.removed), (Some(again), false));
        assert_eq!(app.world().resource::<Selection>().entities, [again]);
        // Gone for good: removed, until dismissed.
        app.world_mut().entity_mut(again).despawn();
        frames(&mut app, 3);
        let i = inspected(&app);
        assert!(i.removed && i.kind == InspectKind::Model, "{i:?}");
        assert!(app.world().resource::<Selection>().entities.is_empty());
        // Something else selected: that is the subject.
        let other = spawn(&mut app, "crate_01", NodeKind::Model);
        select(&mut app, vec![other]);
        let i = inspected(&app);
        assert_eq!((i.entity, i.removed), (Some(other), false));
    }

    #[test]
    fn a_new_selection_wins_over_a_respawn_in_the_same_frame() {
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let model = spawn(&mut app, "amr_01", NodeKind::Model);
        let other = spawn(&mut app, "crate_01", NodeKind::Model);
        select(&mut app, vec![model]);
        app.world_mut().entity_mut(model).despawn();
        spawn(&mut app, "amr_01", NodeKind::Model);
        select(&mut app, vec![other]);
        frames(&mut app, 2);
        let i = inspected(&app);
        assert_eq!((i.entity, i.removed), (Some(other), false));
        assert_eq!(app.world().resource::<Selection>().entities, [other]);
    }

    #[test]
    fn a_subject_shown_removed_is_followed_to_a_later_respawn() {
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let model = spawn(&mut app, "amr_01", NodeKind::Model);
        select(&mut app, vec![model]);
        app.world_mut().entity_mut(model).despawn();
        frames(&mut app, 3);
        assert!(inspected(&app).removed);
        // Some other entity arriving isn't it.
        spawn(&mut app, "crate_01", NodeKind::Model);
        frames(&mut app, 2);
        assert!(inspected(&app).removed);
        let again = spawn(&mut app, "amr_01", NodeKind::Model);
        frames(&mut app, 2);
        let i = inspected(&app);
        assert_eq!((i.entity, i.removed), (Some(again), false));
        assert_eq!(app.world().resource::<Selection>().entities, [again]);
    }

    #[test]
    fn the_world_cant_be_pinned() {
        use bevy::ui::InteractionDisabled;
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let model = spawn(&mut app, "amr_01", NodeKind::Model);
        let pin = {
            let world = app.world_mut();
            world
                .query::<(Entity, &InspectorHeader)>()
                .iter(world)
                .find(|(_, h)| **h == InspectorHeader::Pin)
                .unwrap()
                .0
        };
        assert!(app.world().get::<InteractionDisabled>(pin).is_some());
        app.world_mut().trigger(Activate { entity: pin });
        frames(&mut app, 2);
        assert!(!inspected(&app).pinned);
        select(&mut app, vec![model]);
        assert_eq!(inspected(&app).entity, Some(model));
        assert!(app.world().get::<InteractionDisabled>(pin).is_none());
    }
}
