// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::primitive::{parse, Action, Scene, ServerMessage, Session};
use serde_json::{json, Value};

fn wire(time: u64) -> Value {
    json!({"version":1,"kind":"scan","world_id":1,"epoch":0,"sim_time_ns":time,
        "scan":{"sensor_id":7,"acquired_ns":time,"sensor_position":[1,2,3],"sensor_orientation":[0,0,0,1],
        "angle_min":-1.0,"angle_max":1.0,"angle_step":1.0,"range_min":0.1,"range_max":10.0,"ranges":[2.0,null,4.0]}})
}
fn receive(s: &mut Session, v: Value) {
    s.receive(parse(&v.to_string()).unwrap()).unwrap();
}
fn scene(epoch: u64, sequence: u64, world: u64) -> ServerMessage {
    ServerMessage::Scene(Scene {
        version: 1,
        world: "default".into(),
        world_id: world,
        epoch,
        seconds: 0,
        nanoseconds: 0,
        sequence,
        paused: true,
        visuals: Vec::new(),
    })
}

#[test]
fn scan_wire_keeps_integer_times_nulls_and_inclusive_ray_order() {
    let time = 9_007_199_254_740_993;
    let ServerMessage::Scan(frame) = parse(&wire(time).to_string()).unwrap() else {
        panic!("scan")
    };
    assert_eq!(frame.sim_time_ns, time);
    let scan = frame.scan.unwrap();
    assert_eq!(scan.acquired_ns, time);
    assert_eq!(scan.ranges, [Some(2.0), None, Some(4.0)]);
    assert_eq!(scan.sensor_id, 7);
    assert_eq!(scan.sensor_position, [1.0, 2.0, 3.0]);
    assert_eq!(scan.sensor_orientation, [0.0, 0.0, 0.0, 1.0]);
    let mut clear = wire(time);
    clear["scan"] = Value::Null;
    assert!(matches!(
        parse(&clear.to_string()).unwrap(),
        ServerMessage::Scan(ScanFrame { scan: None, .. })
    ));
}

#[test]
fn scan_rejects_invalid_bounds_identity_count_pose_and_angle_progression() {
    for (key, value) in [
        ("version", json!(2)),
        ("world_id", json!(0)),
        ("world_id", json!(4_294_967_296_u64)),
        ("sim_time_ns", json!(-1)),
    ] {
        let mut v = wire(10);
        v[key] = value;
        assert!(parse(&v.to_string()).is_err(), "{key}");
    }
    for (key, value) in [
        ("sensor_id", json!(0)),
        ("sensor_id", json!(1)),
        ("acquired_ns", json!(11)),
        ("range_min", json!(-0.1)),
        ("range_max", json!(0.1)),
        ("angle_max", json!(-1.0)),
        ("angle_max", json!(8.0)),
        ("angle_step", json!(0.0)),
        ("angle_step", json!(0.5)),
        ("sensor_position", json!([1e40, 0, 0])),
        ("sensor_orientation", json!([0, 0, 0, 2])),
        ("ranges", json!([0.0, 1.0, 2.0])),
        ("ranges", json!([2.0, 3.0, 11.0])),
    ] {
        let mut v = wire(10);
        v["scan"][key] = value;
        assert!(parse(&v.to_string()).is_err(), "{key}");
    }
    for count in [0, 1, 129] {
        let mut v = wire(10);
        v["scan"]["ranges"] = json!(vec![Some(1.0); count]);
        assert!(parse(&v.to_string()).is_err());
    }
    let mut v = wire(10);
    v["scan"]["ranges"] = json!(vec![None::<f64>; 128]);
    v["scan"]["angle_step"] = json!(2.0 / 127.0);
    assert!(parse(&v.to_string()).is_ok());
    let mut missing = wire(10);
    missing.as_object_mut().unwrap().remove("scan");
    assert!(parse(&missing.to_string()).is_err());
    let mut v = wire(10);
    v["scan"]["remote"] = json!("not a supported field");
    assert!(parse(&v.to_string()).is_err());
    assert!(parse(&wire(10).to_string().replace("2.0,null", "NaN,null")).is_err());
    assert!(parse(&format!("{}{}", wire(10), " ".repeat(65536))).is_err());
}

#[test]
fn scan_ignores_wrong_world_epoch_and_older_measurements_without_resurrection() {
    let mut s = Session::default();
    s.opened();
    receive(&mut s, wire(10));
    assert!(s.latest_scan.is_none());
    s.receive(scene(0, 1, 1)).unwrap();
    receive(&mut s, wire(20));
    assert_eq!(s.latest_scan.take().unwrap().unwrap().sim_time_ns, 20);
    for (key, value) in [
        ("world_id", json!(2)),
        ("epoch", json!(1)),
        ("sim_time_ns", json!(19)),
    ] {
        let mut v = wire(20);
        v[key] = value;
        if key == "sim_time_ns" {
            v["scan"]["acquired_ns"] = json!(19);
        }
        receive(&mut s, v);
        assert!(s.latest_scan.is_none());
    }
    let mut clear = wire(30);
    clear["scan"] = Value::Null;
    receive(&mut s, clear);
    assert_eq!(s.latest_scan.take(), Some(None));
    receive(&mut s, wire(25));
    assert!(s.latest_scan.is_none());
    receive(&mut s, wire(30));
    assert!(s.latest_scan.is_none());
    let mut stale = wire(31);
    stale["scan"]["acquired_ns"] = json!(19);
    receive(&mut s, stale);
    assert!(s.latest_scan.is_none());
    receive(&mut s, wire(31));
    assert_eq!(s.latest_scan.take().unwrap().unwrap().sim_time_ns, 31);
    s.receive(scene(1, 2, 1)).unwrap();
    assert_eq!(s.latest_scan.take(), Some(None));
    receive(&mut s, wire(40));
    assert!(s.latest_scan.is_none());
    let mut fresh = wire(1);
    fresh["epoch"] = json!(1);
    receive(&mut s, fresh);
    assert!(s.latest_scan.take().unwrap().is_some());
    s.receive(scene(1, 3, 2)).unwrap();
    assert_eq!(s.latest_scan.take(), Some(None));
    s.disconnect("closed");
    assert_eq!(s.latest_scan.take(), Some(None));
    assert!(s.receive(parse(&wire(50).to_string()).unwrap()).is_err());
}

#[test]
fn scan_coalesces_and_reset_ack_clears_without_advancing_world_clock() {
    let mut s = Session::default();
    s.opened();
    s.receive(scene(0, 1, 1)).unwrap();
    for t in 1..=100 {
        receive(&mut s, wire(t));
    }
    assert_eq!(s.clock().unwrap().0.nanoseconds, 0);
    assert_eq!(s.latest_scan.take().unwrap().unwrap().sim_time_ns, 100);
    s.control(Action::Reset).unwrap();
    receive(
        &mut s,
        json!({"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}),
    );
    assert_eq!(s.latest_scan.take(), Some(None));
    receive(&mut s, wire(101));
    assert!(s.latest_scan.is_none());
    s.receive(scene(1, 2, 1)).unwrap();
    let mut fresh = wire(1);
    fresh["epoch"] = json!(1);
    receive(&mut s, fresh);
    assert_eq!(
        s.latest_scan
            .take()
            .unwrap()
            .unwrap()
            .scan
            .unwrap()
            .acquired_ns,
        1
    );
}

#[test]
fn absolute_angle_offsets_do_not_hide_wrong_steps_or_collapsed_progressions() {
    let offset = 9_007_199_254_740_992.0;
    let mut v = wire(1);
    v["scan"]["angle_min"] = json!(offset);
    v["scan"]["angle_max"] = json!(offset + 2.0);
    v["scan"]["angle_step"] = json!(2.0 / 127.0);
    v["scan"]["ranges"] = json!(vec![Some(1.0); 128]);
    assert!(
        parse(&v.to_string()).is_err(),
        "repeated f64 angles must be refused"
    );
    v["scan"]["ranges"] = json!([1.0, 2.0]);
    v["scan"]["angle_step"] = json!(4.0);
    assert!(
        parse(&v.to_string()).is_err(),
        "large offsets must not relax step consistency"
    );
    v["scan"]["angle_step"] = json!(2.0);
    let ServerMessage::Scan(frame) = parse(&v.to_string()).unwrap() else {
        panic!("scan")
    };
    let s = frame.scan.unwrap();
    assert_eq!(s.angle(0), offset);
    assert_eq!(s.angle(1), offset + 2.0);
    // A harmless rounded progression still ends on the explicit maximum.
    let mut v = wire(1);
    v["scan"]["angle_step"] = json!(f64::from_bits(1.0_f64.to_bits() + 1));
    let ServerMessage::Scan(frame) = parse(&v.to_string()).unwrap() else {
        panic!("scan")
    };
    let s = frame.scan.unwrap();
    assert_eq!(s.angle(2), s.angle_max);
}

#[test]
fn null_clear_fences_acquisition_even_when_a_newer_envelope_repeats_it() {
    let mut s = Session::default();
    s.opened();
    s.receive(scene(0, 1, 1)).unwrap();
    receive(&mut s, wire(20));
    s.latest_scan.take();
    // Repeated acquisitions are allowed while live, independently of envelopes.
    let mut repeated = wire(21);
    repeated["scan"]["acquired_ns"] = json!(20);
    receive(&mut s, repeated);
    assert!(s.latest_scan.take().unwrap().is_some());
    let mut clear = wire(30);
    clear["scan"] = Value::Null;
    receive(&mut s, clear);
    assert_eq!(s.latest_scan.take(), Some(None));
    for acquired in [20, 30] {
        let mut stale = wire(31);
        stale["scan"]["acquired_ns"] = json!(acquired);
        receive(&mut s, stale);
        assert!(s.latest_scan.is_none());
    }
    receive(&mut s, wire(31));
    assert_eq!(
        s.latest_scan
            .take()
            .unwrap()
            .unwrap()
            .scan
            .unwrap()
            .acquired_ns,
        31
    );
}
