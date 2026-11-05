// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::connection::{GzInbox, GzStatus};
use bevy::prelude::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
fn config() -> GzConfig {
    GzConfig {
        backend: crate::BackendKind::Primitive,
        lifecycle: true,
        replay: true,
        reconnect: false,
        gpu: false,
        ..default()
    }
}
fn scene(sequence: u64, epoch: u64, frame: u32) -> Value {
    let mut visuals = vec![
        json!({"id":3,"name":"visual","translation":if frame==2 {[4,5,6]} else {[1,2,3]},"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,2,3]},"color":[1,1,1,1]}),
    ];
    if frame >= 3 {
        visuals.push(json!({"id":7,"name":"visual","translation":[-2,3,4],"rotation":[0,0,0,1],"shape":{"kind":"sphere","radius":1},"color":[1,0,0,1]}));
    }
    if frame == 4 {
        visuals.remove(0);
    }
    json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":epoch,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":frame!=2,"visuals":visuals})
}
fn catalog(sequence: u64, epoch: u64, frame: u32) -> Value {
    let mut models = vec![
        json!({"model_id":2,"name":"box","visual_ids":[3],"can_remove":false,"can_set_state":false}),
    ];
    if frame >= 3 {
        models.push(json!({"model_id":6,"name":"sphere","visual_ids":[7],"can_remove":false,"can_set_state":false}));
    }
    if frame == 4 {
        models.remove(0);
    }
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":epoch,"scene_sequence":sequence,"templates":[],"models":models})
}
fn status(sequence: u64, epoch: u64, frame: u32) -> Value {
    json!({"version":1,"kind":"replay_status","world_id":1,"epoch":epoch,"scene_sequence":sequence,"frame":frame,"frames":4,"recorded_epoch":0,"recorded_sequence":frame,"recorded_seconds":0,"recorded_nanoseconds":0,"recorded_paused":true,"playing":frame==2,"finished":frame==4,"outcomes":if frame==3 {json!([{"action":"spawn","model_id":6,"ok":true}])} else {json!([])}})
}
fn event(value: Value) -> DriverEvent {
    DriverEvent::Message(value.to_string().into_bytes())
}
fn mesh_id(app: &mut App, visual: Entity) -> AssetId<Mesh> {
    let mut meshes = app.world_mut().query::<(&Mesh3d, &ChildOf)>();
    meshes
        .iter(app.world())
        .find(|(_, parent)| parent.parent() == visual)
        .expect("admitted primitive visual has a Mesh3d child")
        .0
         .0
        .id()
}
fn deliver(app: &mut App, backend: &mut PrimitiveBackend) {
    let config = app.world().resource::<GzConfig>().clone();
    app.world_mut()
        .resource_scope(|world, mut status: Mut<GzStatus>| {
            let mut io = BackendIo {
                now: 0.,
                config: &config,
                status: status.reborrow(),
                inbox: world.resource_mut::<GzInbox>(),
                publishes: vec![],
                replies: vec![],
                topics: None,
                arrivals: vec![],
            };
            backend.poll(&mut io);
            backend.flush(&mut io);
        });
    app.update();
}
#[test]
fn explicit_replay_query_has_no_scan_and_preserves_legacy_live_queries() {
    let url = protocol_url("ws://127.0.0.1:9002/", true).unwrap();
    assert_eq!(url, "ws://127.0.0.1:9002/?replay=1");
    assert_eq!(
        lifecycle_url(url, true).unwrap(),
        "ws://127.0.0.1:9002/?replay=1&lifecycle=1&lifecycle_credit=1"
    );
    assert_eq!(
        protocol_url("ws://127.0.0.1:9002/", false).unwrap(),
        "ws://127.0.0.1:9002/?scan=1"
    );
    for query in [
        "scan=1",
        "scan=0",
        "scan",
        "replay=0",
        "replay=1&replay=1",
        "replay",
    ] {
        assert!(
            protocol_url(&format!("ws://127.0.0.1:9002/?{query}"), true).is_err(),
            "{query}"
        );
    }
    assert!(protocol_url("ws://127.0.0.1:9002/?replay=1", false).is_err());
    assert!(PrimitiveBackend::new(&GzConfig {
        replay: true,
        ..default()
    })
    .is_err());
}
#[test]
fn actual_replay_session_flush_updates_ecs_poses_layout_removal_and_restart_without_native_mutation(
) {
    use crate::spawn::{GzIndex, GzWorldRoot};
    let config = config();
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |url| {
        assert!(url.contains("replay=1"));
        assert!(!url.contains("scan="));
        slot.borrow_mut().take().unwrap()
    });
    let mut app = crate::test_util::scene_app(config);
    app.update();
    deliver(&mut app, &mut backend);
    tx.send(DriverEvent::Open).unwrap();
    for value in [
        scene(1, 0, 1),
        catalog(1, 0, 1),
        status(1, 0, 1),
        json!({"version":1,"kind":"publication_credit","token":1}),
    ] {
        tx.send(event(value)).unwrap();
    }
    deliver(&mut app, &mut backend);
    assert_eq!(
        serde_json::from_slice::<Value>(&out.try_recv().unwrap()).unwrap()["token"],
        1
    );
    let initial = app.world().resource::<GzIndex>().by_id[&3];
    let mesh = mesh_id(&mut app, initial);
    for value in [
        scene(2, 0, 2),
        catalog(2, 0, 2),
        status(2, 0, 2),
        json!({"version":1,"kind":"publication_credit","token":2}),
    ] {
        tx.send(event(value)).unwrap();
    }
    deliver(&mut app, &mut backend);
    out.try_recv().unwrap();
    assert_eq!(app.world().resource::<GzIndex>().by_id[&3], initial);
    assert_eq!(mesh_id(&mut app, initial), mesh);
    assert!(
        (app.world()
            .get::<GlobalTransform>(initial)
            .unwrap()
            .translation()
            - Vec3::new(4., 6., -5.))
        .length()
            < 1e-5
    );
    assert!(
        app.world()
            .resource::<GzStatus>()
            .replay
            .as_ref()
            .unwrap()
            .playing
    );
    for frame in 3..=4 {
        for value in [
            scene(u64::from(frame), 0, frame),
            catalog(u64::from(frame), 0, frame),
            status(u64::from(frame), 0, frame),
            json!({"version":1,"kind":"publication_credit","token":frame}),
        ] {
            tx.send(event(value)).unwrap();
        }
        deliver(&mut app, &mut backend);
        out.try_recv().unwrap();
        assert_eq!(
            app.world()
                .resource::<GzStatus>()
                .replay
                .as_ref()
                .unwrap()
                .frame,
            frame
        );
    }
    assert!(!app.world().resource::<GzIndex>().by_id.contains_key(&3));
    assert!(app.world().get_entity(initial).is_err());
    let sphere = app.world().resource::<GzIndex>().by_id[&7];
    assert!(backend.world_control(WorldControl::Play).is_err());
    assert!(out.try_recv().is_err());
    backend.world_control(WorldControl::Reset).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&out.try_recv().unwrap()).unwrap()["request_id"],
        1
    );
    for value in [
        scene(5, 1, 1),
        catalog(5, 1, 1),
        status(5, 1, 1),
        json!({"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}),
    ] {
        tx.send(event(value)).unwrap();
    }
    deliver(&mut app, &mut backend);
    app.update();
    assert!(app.world().get_entity(sphere).is_err());
    assert!(!app.world().resource::<GzIndex>().by_id.contains_key(&7));
    let restarted = app.world().resource::<GzIndex>().by_id[&3];
    assert_ne!(restarted, initial);
    assert!(
        (app.world()
            .get::<GlobalTransform>(restarted)
            .unwrap()
            .translation()
            - Vec3::new(1., 3., -2.))
        .length()
            < 1e-5
    );
    assert_eq!(
        app.world()
            .resource::<GzStatus>()
            .replay
            .as_ref()
            .unwrap()
            .frame,
        1
    );
    assert_eq!(
        app.world()
            .resource::<GzStatus>()
            .partial_clock
            .unwrap()
            .0
            .epoch,
        1
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    let mut roots = app
        .world_mut()
        .query_filtered::<Entity, With<GzWorldRoot>>();
    assert_eq!(roots.iter(app.world()).count(), 1);
    assert!(out.try_recv().is_err()); // No historical lifecycle request or ACK was emitted.
}
#[test]
fn missing_replay_status_or_failed_ready_send_retires_scene_with_visible_error() {
    for missing in [true, false] {
        let config = config();
        let (driver, tx, out) = driver::fake();
        let slot = Rc::new(RefCell::new(Some(driver)));
        let mut backend = PrimitiveBackend::new(&config).unwrap();
        backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
        let mut app = crate::test_util::scene_app(config);
        app.update();
        deliver(&mut app, &mut backend);
        tx.send(DriverEvent::Open).unwrap();
        tx.send(event(scene(1, 0, 1))).unwrap();
        tx.send(event(catalog(1, 0, 1))).unwrap();
        if !missing {
            tx.send(event(status(1, 0, 1))).unwrap();
        }
        tx.send(event(
            json!({"version":1,"kind":"publication_credit","token":1}),
        ))
        .unwrap();
        drop(out); // Also exercises real bounded outgoing channel refusal in the valid-tuple case.
        deliver(&mut app, &mut backend);
        assert!(backend.driver.is_none());
        assert!(app.world().resource::<GzStatus>().replay.is_none());
        assert!(app.world().resource::<GzStatus>().lifecycle.is_none());
        assert!(app
            .world()
            .resource::<GzStatus>()
            .control_error
            .as_deref()
            .unwrap()
            .contains(if missing {
                "incomplete publication"
            } else {
                "Ready send failed"
            }));
        assert!(app
            .world()
            .resource::<crate::spawn::GzIndex>()
            .by_id
            .is_empty());
    }
}
