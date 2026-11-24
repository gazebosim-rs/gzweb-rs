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

#[test]
fn recovery_extends_only_a_server_confirmed_pending_pause() {
    for action in [WorldControl::Pause, WorldControl::Reset] {
        let config = config(false);
        let (driver, tx, _out) = driver::fake();
        let slot = Rc::new(RefCell::new(Some(driver)));
        let mut backend = PrimitiveBackend::new(&config).unwrap();
        backend.connector = Box::new(move |_| slot.borrow_mut().take().unwrap());
        let mut world = World::new();
        world.insert_resource(GzStatus::new("ws://test"));
        world.init_resource::<GzInbox>();
        world.resource_scope(|world, mut status: Mut<GzStatus>| {
            let mut io = BackendIo { now:0.,config:&config,status:status.reborrow(),inbox:world.resource_mut::<GzInbox>(),
                publishes:vec![],replies:vec![],topics:None,arrivals:vec![] };
            backend.poll(&mut io);
            tx.send(DriverEvent::Open).unwrap();
            let mut current = scene(1);
            current["paused"] = json!(false);
            tx.send(event(current)).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            let is_pause = matches!(action, WorldControl::Pause);
            backend.world_control(action).unwrap();
            backend.flush(&mut io);
            let progress = json!({"version":1,"kind":"recovery_status","phase":"restoring","epoch":0,
                "tick":10,"attempt":0,"budget":3,"pause_pending":true});
            for now in [6., 12.] {
                io.now = now;
                tx.send(event(progress.clone())).unwrap();
                backend.poll(&mut io);
                backend.flush(&mut io);
            }
            assert_eq!(backend.driver.is_some(), is_pause);
            if is_pause {
                assert!(io.status.control_pending);
                assert_eq!(io.status.paused(), Some(false));
                // Losing worker progress is still a timeout.
                io.now = 20.;
                backend.poll(&mut io);
                assert!(backend.driver.is_none());
                assert!(io.status.recovery.is_none());
            }
        });
    }
}

#[test]
fn fenced_refusal_and_close_survive_one_poll_and_retries_until_a_scene_or_new_url() {
    for change_url in [false, true] {
        let config = config(false);
        let (first, first_tx, _out) = driver::fake();
        let (retry, retry_tx, _retry_out) = driver::fake();
        let (last, last_tx, _last_out) = driver::fake();
        let drivers = RefCell::new([first, retry, last].into_iter());
        let mut backend = PrimitiveBackend::new(&config).unwrap();
        backend.connector = Box::new(move |_| drivers.borrow_mut().next().unwrap());
        let mut world = World::new();
        world.insert_resource(GzStatus::new(&config.url));
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
            first_tx.send(DriverEvent::Open).unwrap();
            let mut running = scene(1);
            running["paused"] = json!(false);
            first_tx.send(event(running)).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            backend.world_control(WorldControl::Pause).unwrap();
            let fenced = json!({
                "version":1, "kind":"recovery_status", "phase":"fenced", "epoch":0,
                "tick":150, "attempt":1, "budget":1, "pause_pending":false
            });
            let refused = json!({
                "version":1, "kind":"ack", "request_id":1, "ok":false, "epoch":0,
                "seconds":0, "nanoseconds":150_000_000, "paused":false,
                "error":"worker recovery exhausted; session fenced"
            });
            first_tx.send(event(fenced)).unwrap();
            first_tx.send(event(refused)).unwrap();
            first_tx.send(DriverEvent::Closed(None)).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert_eq!(io.status.malformed, 0);
            assert_eq!(io.status.paused(), None);
            assert!(io.status.recovery.is_none());
            assert!(!io.status.control_pending);
            assert_eq!(io.status.fenced_recovery.unwrap().tick, 150);

            backend.connect(None, &mut io);
            retry_tx.send(DriverEvent::Closed(None)).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert_eq!(io.status.fenced_recovery.unwrap().tick, 150);

            backend.connect(change_url.then_some("ws://other"), &mut io);
            last_tx.send(DriverEvent::Open).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert_eq!(io.status.fenced_recovery.is_some(), !change_url);
            last_tx.send(event(scene(1))).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert_eq!(io.status.paused(), Some(true));
            assert!(io.status.fenced_recovery.is_none());

            last_tx.send(DriverEvent::Closed(None)).unwrap();
            backend.poll(&mut io);
            backend.flush(&mut io);
            assert!(io.status.fenced_recovery.is_none());
        });
    }
}

#[test]
fn admitted_operator_handoff_keeps_the_parked_scene_and_reconnects_without_replaying_input() {
    let config = config(false);
    assert!(!config.reconnect);
    let (parked, first, outgoing) = driver::fake();
    let (live, second, resumed_outgoing) = driver::fake();
    let drivers = RefCell::new([parked, live].into_iter());
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |_| drivers.borrow_mut().next().unwrap());
    let mut world = World::new();
    world.insert_resource(GzStatus::new(&config.url));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world,mut status:Mut<GzStatus>| {
        let mut io=BackendIo {now:0.,config:&config,status:status.reborrow(),inbox:world.resource_mut::<GzInbox>(),publishes:vec![],replies:vec![],topics:None,arrivals:vec![]};
        backend.poll(&mut io);
        first.send(DriverEvent::Open).unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        first.send(event(json!({"version":1,"kind":"operator_fence","token":token,"tick":0,"epoch":0,"reason":"Transport lost","eligible":true}))).unwrap();
        let mut running=scene(1);running["paused"]=false.into();
        first.send(event(running)).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        let world=io.inbox.prepared_scene.take().unwrap();
        assert_eq!(world.roots.len(),1);
        assert!(backend.world_control(WorldControl::Play).is_err());
        assert!(backend.world_control(WorldControl::Reset).is_err());
        backend.recover_workers(token).unwrap();
        let sent:Value=serde_json::from_slice(&outgoing.try_recv().unwrap()).unwrap();
        assert_eq!(sent["token"],token);
        assert!(backend.recover_workers(token).is_err());
        first.send(event(json!({"version":1,"kind":"operator_reply","request_id":sent["request_id"],"accepted":true,"error":null}))).unwrap();
        first.send(DriverEvent::Closed(None)).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(io.inbox.prepared_scene.is_none(), "the rendered parked scene must survive the handoff");
        assert_eq!(io.status.paused(),Some(false));
        assert!(!io.status.operator.as_ref().unwrap().can_recover());
        io.now=io.status.reconnect_at.unwrap();
        backend.poll(&mut io);
        second.send(DriverEvent::Open).unwrap();
        second.send(event(json!({"version":1,"kind":"operator_restored","epoch":0,"tick":0,"token":token}))).unwrap();
        second.send(event(scene(1))).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(io.status.operator.is_none());
        assert_eq!(io.status.paused(),Some(true));
        assert!(resumed_outgoing.is_empty());
        assert!(backend.world_control(WorldControl::Play).is_ok());
    });
}
