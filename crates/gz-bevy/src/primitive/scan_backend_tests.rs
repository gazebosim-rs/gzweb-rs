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
fn primitive_scan_opt_in_uses_current_url_without_affecting_control_or_scene_identity() {
    assert_eq!(
        scan_url("ws://127.0.0.1:9002/").unwrap(),
        "ws://127.0.0.1:9002/?scan=1"
    );
    assert_eq!(
        scan_url("ws://127.0.0.1:9002/world?other=ok").unwrap(),
        "ws://127.0.0.1:9002/world?other=ok&scan=1"
    );
    assert_eq!(
        scan_url("ws://127.0.0.1:9002/?scan=1").unwrap(),
        "ws://127.0.0.1:9002/?scan=1"
    );
    for query in ["scan=0", "scan=1&scan=1", "scan", "scan=2&other=ok"] {
        assert!(scan_query(query).is_err());
    }
    assert!(scan_url("not a websocket URL").is_err());
    let config = GzConfig {
        backend: crate::BackendKind::Primitive,
        ..Default::default()
    };
    let (driver, tx, _out) = driver::fake();
    let slot = Rc::new(RefCell::new(Some(driver)));
    let mut backend = PrimitiveBackend::new(&config).unwrap();
    backend.connector = Box::new(move |url| {
        assert!(url.ends_with("?scan=1"));
        slot.borrow_mut().take().unwrap()
    });
    let mut world = World::new();
    world.insert_resource(GzStatus::new("ws://test"));
    world.init_resource::<GzInbox>();
    world.resource_scope(|world,mut status:Mut<GzStatus>| {
        let inbox=world.resource_mut::<GzInbox>();
        let mut io=BackendIo {now:0.0,config:&config,status:status.reborrow(),inbox,publishes:Vec::new(),replies:Vec::new(),topics:None,arrivals:Vec::new()};
        backend.poll(&mut io);backend.flush(&mut io);assert_eq!(io.inbox.primitive_scan.take(),Some(None));
        tx.send(DriverEvent::Open).unwrap();tx.send(scene(1,0)).unwrap();
        let scan=|time,range| DriverEvent::Message(serde_json::json!({"version":1,"kind":"scan","world_id":1,"epoch":0,"sim_time_ns":time,
            "scan":{"sensor_id":7,"acquired_ns":time,"sensor_position":[0,0,0],"sensor_orientation":[0,0,0,1],
            "angle_min":0.0,"angle_max":1.0,"angle_step":1.0,"range_min":0.1,"range_max":10.0,"ranges":[range,null]}}).to_string().into_bytes());
        tx.send(scan(10,2.0)).unwrap();tx.send(scan(20,3.0)).unwrap();backend.poll(&mut io);backend.flush(&mut io);
        let measured=io.inbox.primitive_scan.take().unwrap().unwrap();assert_eq!(measured.sim_time_ns,20);
        assert_eq!(measured.scan.unwrap().ranges,[Some(3.0),None]);
        let desc=io.inbox.prepared_scene.take().unwrap();assert_eq!(desc.roots.len(),1);assert_eq!(desc.roots[0].id,2);
        assert!(io.inbox.poses.is_empty());assert_eq!(io.status.partial_clock.unwrap().0.nanoseconds,1);
        tx.send(DriverEvent::Closed(Some("gone".into()))).unwrap();backend.poll(&mut io);backend.flush(&mut io);
        assert_eq!(io.inbox.primitive_scan.take(),Some(None));assert!(io.inbox.prepared_scene.take().unwrap().roots.is_empty());
        assert!(!backend.session.can_control());
    });
}
