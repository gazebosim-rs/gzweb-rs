// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::connection::{GzInbox, GzStatus};
use bevy::prelude::*;
use gz_core::primitive::lifecycle::Command;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

fn scene(sequence: u64) -> Value {
    json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":true,"visuals":[{"id":3,"name":"visual","translation":[0,0,0],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,1,1]},"color":[1,1,1,1]}]})
}
fn catalog(sequence: u64) -> Value {
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":0,"scene_sequence":sequence,"templates":[{"template_id":9,"label":"Box"}],"models":[{"model_id":2,"name":"box","visual_ids":[3],"can_remove":true,"can_set_state":true}]})
}
fn event(value: Value) -> DriverEvent {
    DriverEvent::Message(value.to_string().into_bytes())
}
fn config(lifecycle: bool) -> GzConfig {
    GzConfig {
        backend: crate::BackendKind::Primitive,
        lifecycle,
        reconnect: false,
        ..default()
    }
}

#[test]
fn queued_ack_after_deadline_is_accepted_and_next_unanswered_command_gets_its_own_deadline() {
    for accepted in [true, false] {
        let config = config(false);
        let (driver, tx, out) = driver::fake();
        let slot = Rc::new(RefCell::new(Some(driver)));
        let mut backend = PrimitiveBackend::new(&config).unwrap();
        backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
        let mut world = World::new();
        world.insert_resource(GzStatus::new("ws://test"));
        world.init_resource::<GzInbox>();
        world.resource_scope(|world, mut status: Mut<GzStatus>| {
            let mut io = BackendIo {
                now: 0., config: &config, status: status.reborrow(),
                inbox: world.resource_mut::<GzInbox>(), publishes: vec![],
                replies: vec![], topics: None, arrivals: vec![],
            };
            backend.poll(&mut io);
            tx.send(DriverEvent::Open).unwrap();
            tx.send(event(scene(1))).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            backend.world_control(WorldControl::Pause).unwrap();
            let command: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
            assert_eq!(command["request_id"], 1);
            backend.flush(&mut io);
            assert!(io.status.control_pending);

            // The actual queued ACK is processed before the old seven-second
            // timer is considered, even when this ECS poll arrived late.
            tx.send(event(json!({"version":1,"kind":"ack","request_id":1,"ok":accepted,"epoch":0,"seconds":0,"nanoseconds":0,"paused":true,"error":if accepted {Value::Null} else {json!("refused")}}))).unwrap();
            io.now = 8.;
            backend.poll(&mut io);
            assert!(backend.driver.is_some());
            assert!(!backend.session.pending_control());
            backend.flush(&mut io);
            assert!(!io.status.control_pending);
            assert!(io.status.control_error.as_deref().unwrap().contains(
                if accepted { "acknowledged" } else { "rejected" }
            ));

            // A new user command before the next flush must not inherit the
            // previous command's deadline. Its real unanswered request still
            // fails at the retained strict >7s boundary, without a retry.
            backend.world_control(WorldControl::Pause).unwrap();
            let next: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
            assert_eq!(next["request_id"], 2);
            backend.flush(&mut io);
            io.now = 15.;
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert!(backend.driver.is_some());
            assert!(io.status.control_pending);
            io.now = 15.001;
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert!(backend.driver.is_none());
            let reason = io.status.control_error.as_deref().unwrap();
            assert!(reason.starts_with("Committed acknowledgement timed out"));
            assert!(reason.contains("limit 7s"));
            assert!(reason.contains("outcome unknown"));
            assert!(out.try_recv().is_err());
        });
    }
}

#[test]
fn queued_lifecycle_reply_after_deadline_finishes_and_later_idle_reports_credit_state() {
    let config = config(true);
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo {
            now: 0., config: &config, status: status.reborrow(),
            inbox: world.resource_mut::<GzInbox>(), publishes: vec![],
            replies: vec![], topics: None, arrivals: vec![],
        };
        backend.poll(&mut io);
        assert_eq!(backend.session.publication_tokens(), (None, None));
        tx.send(DriverEvent::Open).unwrap();
        for value in [scene(1), catalog(1), json!({"version":1,"kind":"publication_credit","token":1})] {
            tx.send(event(value)).unwrap();
        }
        backend.poll(&mut io);
        assert_eq!(backend.session.publication_tokens(), (Some(1), Some(1)));
        backend.flush(&mut io);
        let ready: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
        assert_eq!(ready, json!({"version":1,"kind":"publication_ready","token":1}));
        assert_eq!(backend.session.publication_tokens(), (Some(1), None));
        backend.lifecycle(Command::GetState { model_id: 2 }).unwrap();
        let command: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
        assert_eq!(command["request_id"], 1);
        backend.flush(&mut io);
        for value in [scene(2), catalog(2), json!({"version":1,"kind":"lifecycle_ack","request_id":1,"action":"get_state","world_id":1,"epoch":0,"scene_sequence":2,"seconds":0,"nanoseconds":0,"paused":true,"ok":true,"error":null,"model_id":2,"state":{"translation":[0,0,0],"rotation":[0,0,0,1],"linear_velocity":[0,0,0],"angular_velocity":[0,0,0]}})] {
            tx.send(event(value)).unwrap();
        }
        io.now = 8.;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_some());
        assert!(!io.status.control_pending);
        assert!(backend.session.lifecycle_reply.as_ref().unwrap().ok);
        assert_eq!(backend.session.publication_tokens(), (Some(1), None));
        assert!(out.try_recv().is_err());

        // No more server data is supplied: genuine message silence still
        // closes at >10s. It is not described as an unanswered mutation.
        io.now = 18.;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_some());
        io.now = 18.001;
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(backend.driver.is_none());
        let reason = io.status.control_error.as_deref().unwrap();
        assert!(reason.starts_with("Connection idle timed out"));
        assert!(reason.contains("limit 10s"));
        assert!(reason.contains("no control request pending"));
        assert!(reason.contains("last_credit=Some(1), pending_ready=None"));
        assert!(!reason.contains("outcome unknown"));
        assert_eq!(backend.session.publication_tokens(), (None, None));
    });
}

#[test]
fn unopened_and_open_without_initial_scene_keep_distinct_ten_second_timeouts() {
    for opened in [false, true] {
        let config = config(false);
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
                publishes: vec![],
                replies: vec![],
                topics: None,
                arrivals: vec![],
            };
            backend.poll(&mut io);
            if opened {
                tx.send(DriverEvent::Open).unwrap();
            }
            for now in [9., 10.] {
                io.now = now;
                backend.poll(&mut io);
                backend.flush(&mut io);
                assert!(backend.driver.is_some());
            }
            io.now = 10.1;
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert!(backend.driver.is_none());
            let reason = io.status.control_error.as_deref().unwrap();
            assert!(reason.starts_with(if opened {
                "Initial scene timed out"
            } else {
                "Connection opening timed out"
            }));
            assert!(reason.contains("limit 10s"));
            assert!(reason.contains("no control request pending"));
            assert!(!reason.contains("outcome unknown"));
            assert!(reason.contains("last_credit=None, pending_ready=None"));
        });
    }
}
