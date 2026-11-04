// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::connection::{GzInbox, GzStatus};
use bevy::prelude::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
fn scene(sequence: u64) -> Value {
    json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":0,"seconds":0,"nanoseconds":(sequence-1)*10,"sequence":sequence,"paused":sequence==1,"visuals":[{"id":3,"name":"visual","translation":[1,2,3],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,1,1]},"color":[1,1,1,1]}]})
}
fn catalog(sequence: u64) -> Value {
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":0,"scene_sequence":sequence,"templates":[{"template_id":9,"label":"Box"}],"models":[{"model_id":2,"name":"box","visual_ids":[3],"can_remove":true,"can_set_state":true}]})
}
fn marker(token: u64) -> Value {
    json!({"version":1,"kind":"publication_credit","token":token})
}
fn event(value: Value) -> DriverEvent {
    DriverEvent::Message(value.to_string().into_bytes())
}
fn config() -> GzConfig {
    GzConfig {
        backend: crate::BackendKind::Primitive,
        lifecycle: true,
        reconnect: false,
        ..default()
    }
}
#[test]
fn explicit_credit_queries_are_strict_and_default_urls_remain_unchanged() {
    let base = scan_url("ws://127.0.0.1:9002/").unwrap();
    assert_eq!(lifecycle_url(base.clone(), false).unwrap(), base);
    assert_eq!(
        lifecycle_url(base, true).unwrap(),
        "ws://127.0.0.1:9002/?scan=1&lifecycle=1&lifecycle_credit=1"
    );
    for query in [
        "lifecycle=0",
        "lifecycle=1&lifecycle=1",
        "lifecycle_credit=0",
        "lifecycle_credit",
        "lifecycle_credit=1&lifecycle_credit=1",
    ] {
        let url = format!("ws://127.0.0.1:9002/?scan=1&{query}");
        assert!(lifecycle_url(url.clone(), true).is_err(), "{query}");
        assert!(lifecycle_url(url, false).is_err(), "{query}");
    }
    let url = "ws://127.0.0.1:9002/?scan=1&lifecycle=1&lifecycle_credit=1";
    assert_eq!(lifecycle_url(url.into(), true).unwrap(), url);
    assert!(lifecycle_url(url.into(), false).is_err());
}
#[test]
fn reset_command_precedes_old_ready_and_flush_sends_that_ready_exactly_once() {
    let config = config();
    let (driver, tx, out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |url| {
        assert!(url.ends_with("scan=1&lifecycle=1&lifecycle_credit=1"));
        slot.borrow_mut().take().unwrap()
    });
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world, mut status: Mut<GzStatus>| {
        let mut io = BackendIo { now: 0., config: &config, status: status.reborrow(), inbox: world.resource_mut::<GzInbox>(), publishes: vec![], replies: vec![], topics: None, arrivals: vec![] };
        backend.poll(&mut io);
        tx.send(DriverEvent::Open).unwrap();
        for value in [scene(1), catalog(1), marker(1)] { tx.send(event(value)).unwrap(); }
        backend.poll(&mut io);
        assert!(out.try_recv().is_err());
        backend.world_control(WorldControl::Reset).unwrap();
        let command: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
        assert_eq!(command["action"], "reset");
        assert_eq!(command["request_id"], 1);
        tx.send(event(json!({"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}))).unwrap();
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());
        let ready: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
        assert_eq!(ready, json!({"version":1,"kind":"publication_ready","token":1}));
        backend.flush(&mut io);
        assert!(out.try_recv().is_err());
        // The command is acknowledged, but the existing UI fence deliberately
        // stays locked until a fresh committed reset-epoch Scene arrives.
        assert!(!backend.session.pending_control());
        assert!(io.status.control_pending);
        let mut fresh_scene = scene(1);
        fresh_scene["epoch"] = json!(1);
        let mut fresh_catalog = catalog(1);
        fresh_catalog["epoch"] = json!(1);
        for value in [fresh_scene, fresh_catalog, marker(2)] {
            tx.send(event(value)).unwrap();
        }
        backend.poll(&mut io);
        backend.flush(&mut io);
        assert!(!io.status.control_pending);
        assert_eq!(io.inbox.prepared_scene.take().unwrap().roots[0].id, 3);
        let next_ready: Value = serde_json::from_slice(&out.try_recv().unwrap()).unwrap();
        assert_eq!(next_ready, json!({"version":1,"kind":"publication_ready","token":2}));
        backend.flush(&mut io);
        assert!(out.try_recv().is_err());
        assert!(backend.driver.is_some());
    });
}
#[test]
fn malformed_tuple_or_failed_bounded_ready_send_closes_without_retry() {
    for malformed in [true, false] {
        let config = config();
        let (driver, tx, out) = driver::fake();
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
            tx.send(DriverEvent::Open).unwrap();
            for value in [scene(1), catalog(1), marker(1)] {
                tx.send(event(value)).unwrap();
            }
            if malformed {
                tx.send(DriverEvent::Message(b"{broken".to_vec())).unwrap();
            }
            backend.poll(&mut io);
            if malformed {
                backend.flush(&mut io);
                assert!(out.try_recv().is_err());
                assert_eq!(io.status.malformed, 1);
            } else {
                drop(out);
                backend.flush(&mut io);
                assert!(io
                    .status
                    .control_error
                    .as_deref()
                    .unwrap()
                    .contains("Ready send failed"));
            }
            assert!(backend.driver.is_none());
            assert!(backend.session.publication_ready().is_none());
            assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());
            for _ in 0..3 {
                backend.poll(&mut io);
                backend.flush(&mut io);
                assert!(backend.driver.is_none());
            }
        });
    }
}
#[test]
fn real_bounded_socket_holds_one_flight_until_validated_flush_for_paused_and_advancing_time() {
    use std::{
        net::TcpListener,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use tungstenite::Message;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());
    let (start_tx, start_rx) = mpsc::sync_channel(1);
    let (offered_tx, offered_rx) = mpsc::sync_channel(1);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        start_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        for sequence in 1..=3 {
            let scan = json!({"version":1,"kind":"scan","world_id":1,"epoch":0,"sim_time_ns":(sequence-1)*10,"scan":null});
            for value in [scene(sequence), catalog(sequence), scan, marker(sequence)] {
                socket
                    .send(Message::Text(value.to_string().into()))
                    .unwrap();
            }
            offered_tx.send(sequence).unwrap();
            let Message::Text(text) = socket.read().unwrap() else {
                panic!("Ready text")
            };
            let actual: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                actual,
                json!({"version":1,"kind":"publication_ready","token":sequence})
            );
            ready_tx.send(sequence).unwrap();
        }
    });
    let config = GzConfig { url, ..config() };
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    let mut world = World::new();
    world.insert_resource(GzStatus::new(config.url.clone()));
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
        // The initial poll may have consumed Open but no publication: the
        // producer cannot send one until this explicit start barrier.
        let open_was_consumed = backend.session.connected;
        start_tx.send(()).unwrap();
        for sequence in 1..=3 {
            assert_eq!(
                offered_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                sequence
            );
            // Deliberately do not consume ECS events until the entire normal
            // flight is queued. The native driver is the actual bounded path.
            let expected = if sequence == 1 && !open_was_consumed {
                5
            } else {
                4
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while backend.driver.as_ref().unwrap().events.len() < expected {
                assert!(Instant::now() < deadline, "complete bounded flight");
                std::thread::yield_now();
            }
            assert_eq!(backend.driver.as_ref().unwrap().events.len(), expected);
            assert!(ready_rx.try_recv().is_err());
            backend.poll(&mut io);
            assert!(backend.session.publication_ready().is_some());
            assert!(ready_rx.try_recv().is_err());
            backend.flush(&mut io);
            assert_eq!(
                ready_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                sequence
            );
            assert_eq!(
                backend.session.clock().unwrap().0.nanoseconds,
                u32::try_from((sequence - 1) * 10).unwrap()
            );
            assert_eq!(
                io.status.lifecycle.as_ref().unwrap().scene_sequence,
                sequence
            );
            assert_eq!(backend.session.paused(), Some(sequence == 1));
            assert!(backend.driver.as_ref().unwrap().events.len() <= 4);
            io.now += 1.;
        }
    });
    producer.join().unwrap();
}
