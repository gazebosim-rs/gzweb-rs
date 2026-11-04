// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::connection::{GzInbox, GzStatus};
use bevy::prelude::*;
use std::{cell::RefCell, rc::Rc};

fn scene(sequence: u64, epoch: u64) -> DriverEvent {
    DriverEvent::Message(serde_json::json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":epoch,"seconds":0,"nanoseconds":sequence,"sequence":sequence,"paused":true,
    "visuals":[{"id":2,"name":"visual","translation":[1,2,3],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[0.6,0.4,0.8]},"color":[0.2,0.4,0.8,1]}]}).to_string().into_bytes())
}
#[test]
fn complete_frames_use_pose_lane_and_committed_reset_replaces_atomically() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        ..Default::default()
    };
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world,mut status:Mut<GzStatus>| {
        let inbox=world.resource_mut::<GzInbox>();
        let mut io=BackendIo {now:0.0,config:&config,status:status.reborrow(),inbox,publishes:Vec::new(),replies:Vec::new(),topics:None,arrivals:Vec::new()};
        backend.poll(&mut io);backend.flush(&mut io);
        tx.send(DriverEvent::Open).unwrap();tx.send(scene(1,0)).unwrap();
        backend.poll(&mut io);backend.flush(&mut io);
        let desc=io.inbox.prepared_scene.take().unwrap();
        assert_eq!(desc.roots[0].pose.pos,[1.,2.,3.]);
        assert!(io.status.sim.is_none());assert_eq!(io.status.paused(),Some(true));
        tx.send(scene(2,0)).unwrap();tx.send(scene(3,0)).unwrap();backend.poll(&mut io);backend.flush(&mut io);
        assert!(io.inbox.prepared_scene.is_none());assert_eq!(io.inbox.poses.len(),1);
        assert_eq!(io.inbox.poses[0].id,2);
        backend.world_control(WorldControl::Play).unwrap();
        assert!(backend.world_control(WorldControl::Reset).is_err());
        let command:serde_json::Value=serde_json::from_slice(&out.try_recv().unwrap()).unwrap();assert_eq!(command["action"],"resume");
        backend.flush(&mut io);assert!(io.status.control_pending);assert_eq!(io.status.paused(),Some(true));
        tx.send(DriverEvent::Message(br#"{"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":0,"seconds":0,"nanoseconds":3,"paused":false,"error":null}"#.to_vec())).unwrap();
        backend.poll(&mut io);backend.flush(&mut io);assert_eq!(io.status.paused(),Some(false));
        backend.world_control(WorldControl::Reset).unwrap();out.try_recv().unwrap();
        tx.send(DriverEvent::Message(br#"{"version":1,"kind":"ack","request_id":2,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}"#.to_vec())).unwrap();
        backend.poll(&mut io);backend.flush(&mut io);assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());
        tx.send(scene(4,0)).unwrap();backend.poll(&mut io);backend.flush(&mut io);assert!(io.inbox.prepared_scene.is_none());
        tx.send(scene(0,1)).unwrap();backend.poll(&mut io);backend.flush(&mut io);assert_eq!(io.inbox.prepared_scene.take().unwrap().roots[0].id,2);
        tx.send(DriverEvent::Closed(Some("gone".into()))).unwrap();backend.poll(&mut io);backend.flush(&mut io);
        assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());assert!(io.status.partial_clock.is_none());
        assert!(io.status.reconnect_at.is_some());assert!(io.status.control_error.as_deref().unwrap().contains("gone"));
    });
}
#[test]
fn unsupported_gui_commands_and_timeouts_never_claim_success() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        reconnect: false,
        ..Default::default()
    };
    let (driver, tx, _out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    assert!(backend.world_control(WorldControl::Step(1)).is_err());
    assert!(backend.world_control(WorldControl::Stop).is_err());
    assert!(backend.publish("a", "b", &[]).is_err());
    assert!(backend.request("a", "b", &[]).is_err());
    assert!(backend.list_topics().is_err());
    assert!(backend.subscribe("a").is_err());
    assert!(backend.unsubscribe("a").is_err());
    assert!(backend.throttle("a", 1).is_err());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo {
            now: 0.0,
            config: &config,
            status: status.reborrow(),
            inbox: world.resource_mut::<GzInbox>(),
            publishes: Vec::new(),
            replies: Vec::new(),
            topics: None,
            arrivals: Vec::new(),
        };
        backend.poll(&mut io);
        tx.send(DriverEvent::Open).unwrap();
        tx.send(scene(1, 0)).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        backend.world_control(WorldControl::Pause).unwrap();
        backend.flush(&mut io);
        io.now = 8.;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(matches!(io.status.status, Status::Error(_)));
        assert!(io.status.reconnect_at.is_none());
        assert!(io.status.partial_clock.is_none());
        assert!(io
            .status
            .control_error
            .as_deref()
            .unwrap()
            .contains("outcome unknown"));
    });
}

#[test]
fn actual_ecs_keeps_public_identity_geometry_and_one_root_basis_for_live_poses() {
    use crate::spawn::{GzEntity, GzIndex, GzWorldRoot};
    let mut app = crate::test_util::scene_app(GzConfig {
        gpu: false,
        ..Default::default()
    });
    app.update();
    let mut session = Session::default();
    session.opened();
    let DriverEvent::Message(bytes) = scene(1, 0) else {
        unreachable!()
    };
    session
        .receive(primitive::parse(std::str::from_utf8(&bytes).unwrap()).unwrap())
        .unwrap();
    let mut initial = session.latest.take().unwrap();
    let mut duplicate = initial.visuals[0].clone();
    duplicate.id = 3;
    duplicate.translation = [-1., 2., 3.];
    initial.visuals.push(duplicate);
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(initial.description().unwrap());
    app.update();
    let first = app.world().resource::<GzIndex>().by_id[&2];
    let second = app.world().resource::<GzIndex>().by_id[&3];
    assert_ne!(first, second);
    assert_eq!(
        app.world().get::<GzEntity>(first).unwrap().name,
        app.world().get::<GzEntity>(second).unwrap().name
    );
    assert_ne!(
        app.world().get::<GzEntity>(first).unwrap().path,
        app.world().get::<GzEntity>(second).unwrap().path
    );
    assert_eq!(
        app.world().get::<Transform>(first).unwrap().translation,
        Vec3::new(1., 2., 3.)
    );
    let actual = app
        .world()
        .get::<GlobalTransform>(first)
        .unwrap()
        .translation();
    assert!((actual - Vec3::new(1., 3., -2.)).length() < 1e-5);
    let mut roots = app
        .world_mut()
        .query_filtered::<Entity, With<GzWorldRoot>>();
    assert_eq!(roots.iter(app.world()).count(), 1);
    let mesh_count = app.world().resource::<Assets<Mesh>>().len();
    let mut mesh_query = app.world_mut().query::<(Entity, &Mesh3d)>();
    let meshes: Vec<_> = mesh_query
        .iter(app.world())
        .map(|(e, m)| (e, m.0.id()))
        .collect();
    app.world_mut()
        .resource_mut::<GzInbox>()
        .poses
        .push(gz_core::msgs::Pose {
            id: 2,
            position: Some(gz_core::msgs::Vector3d {
                x: 4.,
                y: 5.,
                z: 6.,
                ..Default::default()
            }),
            ..Default::default()
        });
    app.update();
    assert_eq!(app.world().resource::<GzIndex>().by_id[&2], first);
    assert_eq!(
        app.world().get::<Transform>(first).unwrap().translation,
        Vec3::new(4., 5., 6.)
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
    let actual_meshes: Vec<_> = mesh_query
        .iter(app.world())
        .map(|(e, m)| (e, m.0.id()))
        .collect();
    assert_eq!(meshes, actual_meshes);
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(empty_scene());
    app.update();
    assert!(app.world().get_entity(first).is_err());
    assert!(app.world().get_entity(second).is_err());
    assert!(app.world().resource::<GzIndex>().by_id.is_empty());
    initial.epoch = 1;
    app.world_mut().resource_mut::<GzInbox>().prepared_scene = Some(initial.description().unwrap());
    app.update();
    let fresh = app.world().resource::<GzIndex>().by_id[&2];
    assert_ne!(fresh, first);
    assert_eq!(app.world().get::<GzEntity>(fresh).unwrap().id, 2);
    assert_eq!(
        app.world().get::<Transform>(fresh).unwrap().translation,
        Vec3::new(1., 2., 3.)
    );
}
#[test]
fn primitive_auth_is_explicitly_unsupported() {
    assert!(PrimitiveBackend::new(&GzConfig {
        key: Some("key".into()),
        ..Default::default()
    })
    .is_err());
}

#[test]
fn normal_async_connect_is_not_terminal_but_real_send_failure_and_timeout_are() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        reconnect: false,
        ..Default::default()
    };
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo {
            now: 0.0,
            config: &config,
            status: status.reborrow(),
            inbox: world.resource_mut::<GzInbox>(),
            publishes: Vec::new(),
            replies: Vec::new(),
            topics: None,
            arrivals: Vec::new(),
        };
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_some());
        assert!(matches!(io.status.status, Status::Connecting));
        // Text is presentation only. Neither Connecting nor other informational
        // text can authorize socket retirement while the asynchronous Open waits.
        backend.session.status = "Informational connection status".into();
        io.now = 9.0;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_some());
        assert!(matches!(io.status.status, Status::Connecting));
        tx.send(DriverEvent::Open).unwrap();
        tx.send(scene(1, 0)).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(matches!(io.status.status, Status::Ready));
        assert!(backend.session.can_control());
        drop(out); // The real native outgoing channel is now disconnected.
        assert!(matches!(
            backend.world_control(WorldControl::Play),
            Err(SendError::Failed)
        ));
        assert!(backend.pending_send_failure.is_some());
        assert_eq!(io.status.paused(), Some(true));
        backend.flush(&mut io);
        assert!(backend.driver.is_none());
        assert!(backend.pending_send_failure.is_none());
        assert!(matches!(io.status.status, Status::Error(_)));
        assert!(io.status.partial_clock.is_none());
        assert!(io.inbox.prepared_scene.as_ref().unwrap().roots.is_empty());
        assert!(io
            .status
            .control_error
            .as_deref()
            .unwrap()
            .contains("outcome unknown"));
    });
    let (driver, _tx, _out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo {
            now: 0.0,
            config: &config,
            status: status.reborrow(),
            inbox: world.resource_mut::<GzInbox>(),
            publishes: Vec::new(),
            replies: Vec::new(),
            topics: None,
            arrivals: Vec::new(),
        };
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_some());
        assert!(matches!(io.status.status, Status::Connecting));
        io.now = 10.1;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_none());
        assert!(matches!(io.status.status, Status::Error(_)));
        assert!(io.status.reconnect_at.is_none());
        assert!(io
            .status
            .control_error
            .as_deref()
            .unwrap()
            .contains("timed out"));
    });
}

#[test]
fn mesh_complete_frames_keep_corrected_pose_lane_and_rebuild_on_asset_change() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        ..Default::default()
    };
    let (driver, tx, _out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    let mut message = serde_json::json!({"version":2,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":1,"sequence":1,"paused":true,
    "visuals":[{"id":2,"name":"visual","translation":[1,2,3],"rotation":[0,0,0,1],"shape":{"kind":"mesh","uri":"http://127.0.0.1:9002/assets/0/arrow.gltf","scale":[2,3,5]},"color":[1,0,0,1]}]});
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let inbox = world.resource_mut::<GzInbox>();
        let mut io = BackendIo {
            now: 0.,
            config: &config,
            status: status.reborrow(),
            inbox,
            publishes: Vec::new(),
            replies: Vec::new(),
            topics: None,
            arrivals: Vec::new(),
        };
        backend.poll(&mut io);
        tx.send(DriverEvent::Open).unwrap();
        tx.send(DriverEvent::Message(message.to_string().into_bytes()))
            .unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        let desc = io.inbox.prepared_scene.take().unwrap();
        assert_eq!(desc.roots[0].scale, [2., 5., 3.]);
        assert_eq!(
            desc.roots[0].pose.quat,
            [
                std::f64::consts::FRAC_1_SQRT_2,
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2
            ]
        );
        message["sequence"] = serde_json::json!(2);
        message["nanoseconds"] = serde_json::json!(2);
        message["visuals"][0]["translation"] = serde_json::json!([4, 5, 6]);
        tx.send(DriverEvent::Message(message.to_string().into_bytes()))
            .unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(io.inbox.prepared_scene.is_none());
        assert_eq!(io.inbox.poses.len(), 1);
        let pose = &io.inbox.poses[0];
        assert_eq!(pose.id, 2);
        assert_eq!(pose.position.as_ref().unwrap().x, 4.);
        let q = pose.orientation.as_ref().unwrap();
        assert_eq!(
            [q.x, q.y, q.z, q.w],
            [
                std::f64::consts::FRAC_1_SQRT_2,
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2
            ]
        );
        message["sequence"] = serde_json::json!(3);
        message["nanoseconds"] = serde_json::json!(3);
        message["visuals"][0]["shape"]["scale"] = serde_json::json!([3, 4, 5]);
        tx.send(DriverEvent::Message(message.to_string().into_bytes()))
            .unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert_eq!(
            io.inbox.prepared_scene.take().unwrap().roots[0].scale,
            [3., 5., 4.]
        );
        assert!(io.inbox.poses.is_empty());
        message["sequence"] = serde_json::json!(4);
        message["nanoseconds"] = serde_json::json!(4);
        message["visuals"][0]["shape"]["uri"] =
            serde_json::json!("http://127.0.0.1:9002/assets/0/arrow.glb");
        tx.send(DriverEvent::Message(message.to_string().into_bytes()))
            .unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(
            io.inbox.prepared_scene.take().unwrap().roots[0]
                .visual
                .as_ref()
                .unwrap()
                .geometry
                .kind_name()
                == "mesh"
        );
        assert!(io.inbox.poses.is_empty());
    });
}

#[test]
fn closed_cause_survives_retirement_without_repeated_connection_work() {
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        reconnect: false,
        ..Default::default()
    };
    let (driver, tx, _out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo {
            now: 0.,
            config: &config,
            status: status.reborrow(),
            inbox: world.resource_mut::<GzInbox>(),
            publishes: Vec::new(),
            replies: Vec::new(),
            topics: None,
            arrivals: Vec::new(),
        };
        backend.poll(&mut io);
        tx.send(DriverEvent::Open).unwrap();
        tx.send(scene(1, 0)).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert_eq!(io.status.status, Status::Ready);
        let cause = "event queue full; WebSocket close code 4002: bounded queue";
        tx.send(DriverEvent::Closed(Some(cause.into()))).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_none());
        assert_eq!(io.status.control_error.as_deref(), Some(cause));
        assert!(io.status.lifecycle.is_none());
        assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());
        for _ in 0..10 {
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert_eq!(io.status.control_error.as_deref(), Some(cause));
            assert!(backend.driver.is_none());
        }
    });
}
