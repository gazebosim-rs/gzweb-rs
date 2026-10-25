use bevy::prelude::*;
use gz_core::insert::Shape;
use gz_core::parts::Part;
use gz_core::scene::{NodeKind, Pose};

use super::super::{Editing, EditingModel};
use super::*;
use crate::spawn::{GzEntity, GzWorldRoot};
use crate::test_util::scene_app;
use crate::GzConfig;

/// With a renderer's assets, a link is built as the scene message's are:
/// its visual's meshes, under it.
#[test]
fn a_link_is_built_with_its_meshes() {
    let mut app = scene_app(GzConfig::default());
    app.init_resource::<EditParts>()
        .init_resource::<Selection>()
        .add_message::<PartCommand>()
        .add_message::<PartsDiscarded>()
        .add_message::<UndoCommand>()
        .add_systems(Update, run_parts);
    let world = app.world_mut();
    let root = world.spawn((Transform::default(), GzWorldRoot)).id();
    let model = world
        .spawn((
            GzEntity {
                name: "m".into(),
                id: 1,
                kind: NodeKind::Model,
                path: "m".into(),
            },
            Transform::default(),
            ChildOf(root),
        ))
        .id();
    world.insert_resource(EditingModel(Some(Editing {
        model,
        resume: false,
        undecided: false,
        lit: Vec::new(),
    })));
    app.update();
    app.world_mut().write_message(PartCommand::Add(Part::Link {
        name: "link_1".into(),
        shape: Shape::Cylinder,
        pose: Pose::IDENTITY,
    }));
    app.update();
    let world = app.world_mut();
    let (link, _) = world
        .query::<(Entity, &GzEntity)>()
        .iter(world)
        .find(|(_, g)| g.path == "m/link_1")
        .expect("the link");
    assert!(world.get::<DraftPart>(link).is_some());
    let meshes = world
        .query_filtered::<Entity, With<Mesh3d>>()
        .iter(world)
        .filter(|e| world.get::<ChildOf>(*e).is_some())
        .count();
    assert_eq!(meshes, 1, "the visual's cylinder");
}
