// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::connection::{GzInbox, GzStatus};
use bevy::prelude::*;
use gz_core::primitive::lifecycle::Command;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
fn event(value: Value) -> DriverEvent {
    DriverEvent::Message(value.to_string().into_bytes())
}
fn scene(sequence: u64) -> Value {
    json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":true,"visuals":[{"id":3,"name":"visual","translation":[1,2,3],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,1,1]},"color":[1,1,1,1]}]})
}
fn catalog(sequence: u64) -> Value {
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":0,"scene_sequence":sequence,"templates":[{"template_id":9,"label":"Box"}],"models":[{"model_id":2,"name":"same","visual_ids":[3],"can_remove":true,"can_set_state":true}]})
}
#[test]
fn opted_in_backend_retains_scene_until_actual_snapshot_and_clears_unknown_outcome_on_disconnect() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        lifecycle: true,
        reconnect: false,
        ..default()
    };
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |url| {
        assert!(url.contains("scan=1&lifecycle=1"));
        slot.borrow_mut().take().unwrap()
    });
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world,mut status:Mut<GzStatus>|{
        let inbox=world.resource_mut::<GzInbox>();let mut io=BackendIo{now:0.,config:&config,status:status.reborrow(),inbox,publishes:vec![],replies:vec![],topics:None,arrivals:vec![]};
        backend.poll(&mut io);backend.flush(&mut io);tx.send(DriverEvent::Open).unwrap();tx.send(event(scene(1))).unwrap();tx.send(event(catalog(1))).unwrap();backend.poll(&mut io);backend.flush(&mut io);
        assert_eq!(io.status.lifecycle.as_ref().unwrap().models[0].model_id,2);assert_eq!(io.inbox.prepared_scene.take().unwrap().roots[0].id,3);
        backend.lifecycle(Command::Remove{model_id:2}).unwrap();assert!(backend.world_control(WorldControl::Reset).is_err());let request:Value=serde_json::from_slice(&out.try_recv().unwrap()).unwrap();assert_eq!(request["model_id"],2);
        backend.flush(&mut io);assert!(io.status.control_pending);assert!(io.inbox.prepared_scene.is_none());
        tx.send(event(json!({"version":1,"kind":"lifecycle_ack","request_id":1,"action":"remove","world_id":1,"epoch":0,"scene_sequence":1,"seconds":0,"nanoseconds":0,"paused":true,"ok":false,"error":"not removable","model_id":null,"state":null}))).unwrap();backend.poll(&mut io);backend.flush(&mut io);assert!(!io.status.control_pending);assert!(io.status.control_error.as_deref().unwrap().contains("not removable"));
        backend.lifecycle(Command::GetState{model_id:2}).unwrap();out.try_recv().unwrap();backend.flush(&mut io);io.now=11.;backend.poll(&mut io);backend.flush(&mut io);
        assert!(io.status.lifecycle.is_none());assert!(io.status.lifecycle_reply.is_none());assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());assert!(io.status.control_error.as_deref().unwrap().contains("unknown"));
    });
}

#[test]
fn committed_replacements_spawn_and_remove_actual_ecs_assets_with_one_world_basis() {
    use crate::spawn::{GzEntity, GzIndex, GzWorldRoot};
    let mut app = crate::test_util::scene_app(GzConfig {
        gpu: false,
        ..default()
    });
    app.update();
    let primitive::ServerMessage::Scene(initial) = primitive::parse(&scene(1).to_string()).unwrap()
    else {
        panic!("scene")
    };
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(initial.description().unwrap());
    app.update();
    let old = app.world().resource::<GzIndex>().by_id[&3];
    let count = app.world().resource::<Assets<Mesh>>().len();
    let mut inserted = initial;
    inserted.sequence = 2;
    let mut visual = inserted.visuals[0].clone();
    visual.id = 4;
    visual.translation = [-2., 3., 4.];
    inserted.visuals.push(visual);
    app.world_mut().resource_mut::<GzInbox>().prepared_scene =
        Some(inserted.description().unwrap());
    app.update();
    assert!(app.world().get_entity(old).is_err());
    let inserted_entity = app.world().resource::<GzIndex>().by_id[&4];
    assert_eq!(app.world().get::<GzEntity>(inserted_entity).unwrap().id, 4);
    assert!(
        (app.world()
            .get::<GlobalTransform>(inserted_entity)
            .unwrap()
            .translation()
            - Vec3::new(-2., 4., -3.))
        .length()
            < 1e-5
    );
    let mut meshes = app.world_mut().query::<&Mesh3d>();
    let retired_meshes: Vec<_> = meshes.iter(app.world()).map(|mesh| mesh.0.id()).collect();
    inserted.sequence = 3;
    inserted.visuals.retain(|v| v.id == 4);
    app.world_mut().resource_mut::<GzInbox>().prepared_scene =
        Some(inserted.description().unwrap());
    app.update();
    assert!(!app.world().resource::<GzIndex>().by_id.contains_key(&3));
    assert!(app.world().get_entity(inserted_entity).is_err());
    // apply_scene drops the old Mesh3d handles in Update. The maintained
    // Assets<Mesh>::track_assets runs in PreUpdate, so its drop-event collection
    // belongs to the next normal frame, not the replacement frame itself.
    app.update();
    let assets = app.world().resource::<Assets<Mesh>>();
    assert!(retired_meshes.iter().all(|id| !assets.contains(*id)));
    let current_meshes: Vec<_> = meshes.iter(app.world()).map(|mesh| mesh.0.id()).collect();
    assert_eq!(current_meshes.len(), 1);
    assert!(assets.contains(current_meshes[0]));
    assert_eq!(assets.len(), count);
    let mut roots = app
        .world_mut()
        .query_filtered::<Entity, With<GzWorldRoot>>();
    assert_eq!(roots.iter(app.world()).count(), 1);
}
