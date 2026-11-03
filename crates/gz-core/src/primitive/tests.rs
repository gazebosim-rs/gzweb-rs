// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::{json, Value};

fn scene_value(sequence: u64) -> Value {
    json!({"version":1,"kind":"scene","world":"default","world_id":12,"epoch":0,
        "seconds":0,"nanoseconds":sequence,"sequence":sequence,"paused":false,
        "visuals":[{"id":4294967295_u64,"name":"body::box","translation":[1,2,3],
        "rotation":[0,0,0,1],"shape":{"kind":"box","size":[0.6,0.4,0.8]},"color":[0.2,0.4,0.8,1]}]})
}
fn scene(sequence: u64) -> Scene {
    match parse(&scene_value(sequence).to_string()).unwrap() {
        ServerMessage::Scene(s) => s,
        _ => unreachable!(),
    }
}
fn ack(id: u64, epoch: u64, paused: bool) -> ServerMessage {
    parse(
        &json!({"version":1,"kind":"ack","request_id":id,"ok":true,
        "epoch":epoch,"seconds":0,"nanoseconds":10,"paused":paused,"error":null})
        .to_string(),
    )
    .unwrap()
}
fn ready() -> Session {
    let mut state = Session::default();
    state.opened();
    state.receive(ServerMessage::Scene(scene(1))).unwrap();
    state
}

#[test]
fn raw_z_up_full_dimensions_and_checked_ids() {
    let s = scene(1);
    let desc = s.description().unwrap();
    assert_eq!(desc.roots[0].id, u32::MAX);
    assert_eq!(desc.roots[0].pose.pos, [1., 2., 3.]);
    assert_eq!(desc.roots[0].pose.quat, [0., 0., 0., 1.]);
    assert_eq!(
        desc.roots[0].visual.as_ref().unwrap().geometry,
        crate::scene::GeometryDesc::Box {
            size: [0.6, 0.4, 0.8]
        }
    );
    for id in [0_u64, 12, 4294967296, 9007199254740993, u64::MAX] {
        let mut v = scene_value(1);
        v["visuals"][0]["id"] = json!(id);
        assert!(parse(&v.to_string()).is_err());
    }
    let mut v = scene_value(1);
    v["world_id"] = json!(4294967296_u64);
    assert!(parse(&v.to_string()).is_err());
    let mut s = scene(1);
    let mut v = s.visuals[0].clone();
    v.id = 3;
    s.visuals.push(v);
    let desc = s.description().unwrap();
    assert_eq!(desc.roots[0].name, desc.roots[1].name);
    assert_ne!(desc.roots[0].path, desc.roots[1].path);
}

#[test]
fn invalid_envelopes_shapes_counts_names_and_numeric_domains_fail_closed() {
    for invalid in ["", "[]", "null", "{}", "{", "{\"kind\":\"other\"}"] {
        assert!(parse(invalid).is_err());
    }
    assert!(parse(&" ".repeat(MAX_SCENE_BYTES + 1)).is_err());
    for (field, value) in [
        ("version", json!(2)),
        ("world_id", json!(0)),
        ("world", json!("other")),
        ("nanoseconds", json!(1_000_000_000_u64)),
        ("epoch", json!(-1)),
        ("sequence", json!(1.5)),
        ("extra", json!(0)),
    ] {
        let mut input = scene_value(1);
        input[field] = value;
        assert!(parse(&input.to_string()).is_err(), "{field}");
    }
    for (field, value) in [
        ("id", json!(0)),
        ("translation", json!([1, 2])),
        ("rotation", json!([0, 0, 0, 0])),
        ("rotation", json!([0, 0, 0, 2])),
        ("color", json!([0, 0, 0, 1.1])),
        ("color", json!([0, 0, 0, 0.5])),
        ("shape", json!({"kind":"cylinder","radius":1})),
        ("shape", json!({"kind":"box","size":[1,0,1]})),
        ("shape", json!({"kind":"sphere","radius":-1})),
        ("shape", json!({"kind":"sphere","radius":1,"extra":0})),
        ("name", json!("é".repeat(129))),
    ] {
        let mut input = scene_value(1);
        input["visuals"][0][field] = value;
        assert!(parse(&input.to_string()).is_err(), "{field}");
    }
    let mut s = scene(1);
    s.visuals = vec![s.visuals[0].clone(); 2];
    assert!(s.validate().is_err());
    s.visuals = (0..=MAX_VISUALS)
        .map(|id| {
            let mut v = s.visuals[0].clone();
            v.id = id as u64 + 1;
            v
        })
        .collect();
    assert!(s.validate().is_err());
    for value in [f64::NAN, f64::INFINITY, f64::MAX] {
        let mut s = scene(1);
        s.visuals[0].translation[0] = value;
        assert!(s.validate().is_err());
    }
    for value in [0., -1., f64::MIN_POSITIVE, f64::INFINITY] {
        let mut s = scene(1);
        s.visuals[0].shape = Shape::Sphere { radius: value };
        assert!(s.validate().is_err());
    }
    let mut s = scene(1);
    s.visuals[0].shape = Shape::Sphere { radius: 0.3 };
    s.visuals[0].name = "é".repeat(128);
    s.validate().unwrap();
    s.visuals.clear();
    s.validate().unwrap();
}

#[test]
fn complete_scene_coalescing_discards_stale_epochs_sequences_and_replaces_worlds() {
    let mut state = ready();
    for seq in 2..=30 {
        state.receive(ServerMessage::Scene(scene(seq))).unwrap();
    }
    assert_eq!(state.latest.as_ref().unwrap().sequence, 30);
    state.receive(ServerMessage::Scene(scene(20))).unwrap();
    assert_eq!(state.latest.as_ref().unwrap().sequence, 30);
    let mut next = scene(0);
    next.epoch = 1;
    next.visuals.clear();
    state.receive(ServerMessage::Scene(next)).unwrap();
    state.receive(ServerMessage::Scene(scene(31))).unwrap();
    assert_eq!(state.latest.as_ref().unwrap().epoch, 1);
    let mut backwards = scene(2);
    backwards.epoch = 1;
    backwards.seconds = 1;
    state.receive(ServerMessage::Scene(backwards)).unwrap();
    let mut backwards = scene(3);
    backwards.epoch = 1;
    assert!(state.receive(ServerMessage::Scene(backwards)).is_err());
    let mut world = scene(0);
    world.world_id = 99;
    state.clear_scene = false;
    state.receive(ServerMessage::Scene(world)).unwrap();
    assert!(state.clear_scene);
    assert_eq!(state.latest.as_ref().unwrap().world_id, 99);
}

#[test]
fn commands_are_single_pending_monotonic_and_success_waits_for_actual_ack() {
    let mut state = Session::default();
    assert!(state.control(Action::Pause).is_err());
    state.opened();
    assert!(state.control(Action::Pause).is_err());
    state.receive(ServerMessage::Scene(scene(1))).unwrap();
    let request: Value = serde_json::from_str(&state.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(
        request,
        json!({"version":1,"kind":"control","request_id":1,"action":"pause"})
    );
    assert!(state.control(Action::Reset).is_err());
    assert_eq!(state.paused(), Some(false));
    assert!(state.receive(ack(2, 0, true)).is_err());
    assert!(!state.can_control());
    state.receive(ack(1, 0, true)).unwrap();
    assert_eq!(state.paused(), Some(true));
    assert!(state.can_control());
    let request: Value = serde_json::from_str(&state.control(Action::Resume).unwrap()).unwrap();
    assert_eq!(request["request_id"], 2);
    assert!(state.receive(ack(2, 0, true)).is_err());
    state.receive(ack(2, 0, false)).unwrap();
    state.control(Action::Reset).unwrap();
    assert!(state.receive(ack(3, 0, false)).is_err());
    assert!(state.receive(parse(&json!({"version":1,"kind":"ack","request_id":3,"ok":true,"epoch":2,"seconds":0,"nanoseconds":0,"paused":true,"error":null}).to_string()).unwrap()).is_err());
    for (paused, seconds, nanoseconds) in [(false, 0, 0), (true, 1, 0), (true, 0, 1)] {
        let contradictory = parse(&json!({"version":1,"kind":"ack","request_id":3,"ok":true,"epoch":1,"seconds":seconds,"nanoseconds":nanoseconds,"paused":paused,"error":null}).to_string()).unwrap();
        assert!(state.receive(contradictory).is_err());
    }
    state.receive(parse(&json!({"version":1,"kind":"ack","request_id":3,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}).to_string()).unwrap()).unwrap();
    assert!(state.clear_scene);
    assert!(state.latest.is_none());
    assert!(!state.can_control());
    state.receive(ServerMessage::Scene(scene(99))).unwrap();
    assert!(state.latest.is_none());
    let mut fresh = scene(0);
    fresh.epoch = 1;
    fresh.nanoseconds = 0;
    fresh.paused = true;
    state.receive(ServerMessage::Scene(fresh)).unwrap();
    assert!(state.can_control());
}

#[test]
fn rejected_missing_and_cross_world_acknowledgements_never_invent_success() {
    let mut state = ready();
    assert!(state.receive(ack(1, 0, true)).is_err());
    state.control(Action::Pause).unwrap();
    let mut other = scene(1);
    other.world_id = 88;
    assert!(state.receive(ServerMessage::Scene(other)).is_err());
    let rejection=parse(&json!({"version":1,"kind":"ack","request_id":1,"ok":false,"epoch":0,"seconds":0,"nanoseconds":1,"paused":false,"error":"not accepted"}).to_string()).unwrap();
    state.receive(rejection).unwrap();
    assert!(state.can_control());
    assert!(state.notice.as_ref().unwrap().contains("rejected"));
    assert_eq!(state.paused(), Some(false));
    for (field, value) in [
        ("version", json!(2)),
        ("request_id", json!(0)),
        ("nanoseconds", json!(1_000_000_000)),
        ("ok", json!(false)),
        ("extra", json!(1)),
    ] {
        let mut input = json!({"version":1,"kind":"ack","request_id":2,"ok":true,"epoch":0,"seconds":0,"nanoseconds":0,"paused":true,"error":null});
        input[field] = value;
        assert!(parse(&input.to_string()).is_err());
    }
    let mut input = json!({"version":1,"kind":"ack","request_id":2,"ok":true,"epoch":0,"seconds":0,"nanoseconds":0,"paused":true,"error":null});
    input.as_object_mut().unwrap().remove("error");
    assert!(parse(&input.to_string()).is_err());
}

#[test]
fn reconnect_clears_every_old_scene_and_pending_command_without_reusing_ids() {
    let mut state = ready();
    state.control(Action::Reset).unwrap();
    state.disconnect("Connection lost; control outcome unknown");
    assert!(state.latest.is_none());
    assert!(state.clear_scene);
    assert!(!state.can_control());
    assert!(state.status.contains("unknown"));
    assert!(state.receive(ServerMessage::Scene(scene(2))).is_err());
    state.opened();
    state.receive(ServerMessage::Scene(scene(0))).unwrap();
    let request: Value = serde_json::from_str(&state.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(request["request_id"], 2);
    state.next_request = Some(u64::MAX);
    state.pending = None;
    state.control(Action::Resume).unwrap();
    state.receive(ack(u64::MAX, 0, false)).unwrap();
    assert!(!state.can_control());
}

#[test]
fn acknowledgement_fence_prevents_an_older_running_scene_overwriting_pause() {
    let mut state = ready();
    state.control(Action::Pause).unwrap();
    state.receive(ack(1, 0, true)).unwrap();
    state.receive(ServerMessage::Scene(scene(5))).unwrap();
    assert_eq!(state.latest.as_ref().unwrap().sequence, 1);
    assert_eq!(state.paused(), Some(true));
    let mut committed = scene(10);
    committed.paused = true;
    state.receive(ServerMessage::Scene(committed)).unwrap();
    assert_eq!(state.latest.as_ref().unwrap().sequence, 10);
    assert_eq!(state.paused(), Some(true));
}
