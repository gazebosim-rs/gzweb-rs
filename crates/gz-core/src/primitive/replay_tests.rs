// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde_json::{json, Value};

fn scene(sequence: u64, epoch: u64, paused: bool) -> Value {
    json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":epoch,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":paused,"visuals":[{"id":3,"name":"visual","translation":[1,2,3],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,2,3]},"color":[1,1,1,1]}]})
}
fn catalog(sequence: u64, epoch: u64) -> Value {
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":epoch,"scene_sequence":sequence,"templates":[],"models":[{"model_id":2,"name":"box","visual_ids":[3],"can_remove":false,"can_set_state":false}]})
}
fn status(sequence: u64, epoch: u64, frame: u32, playing: bool) -> Value {
    json!({"version":1,"kind":"replay_status","world_id":1,"epoch":epoch,"scene_sequence":sequence,"frame":frame,"frames":2,"recorded_epoch":0,"recorded_sequence":frame,"recorded_seconds":0,"recorded_nanoseconds":0,"recorded_paused":true,"playing":playing,"finished":false,"outcomes":[]})
}
fn receive(session: &mut Session, value: Value) -> Result<(), String> {
    session.receive(parse(&value.to_string())?)
}
fn session() -> Session {
    let mut s = Session::default();
    s.enable_lifecycle(true);
    s.enable_publication_credit(true).unwrap();
    s.enable_replay(true).unwrap();
    s.opened();
    s
}
fn tuple(s: &mut Session, seq: u64, epoch: u64, frame: u32, playing: bool) {
    for value in [
        scene(seq, epoch, !playing),
        catalog(seq, epoch),
        status(seq, epoch, frame, playing),
    ] {
        receive(s, value).unwrap();
    }
}
#[test]
fn strict_replay_metadata_and_outcome_domains_preserve_large_refused_ids() {
    let base = status(1, 0, 1, false);
    for (field, value) in [
        ("version", json!(2)),
        ("world_id", json!(0)),
        ("frame", json!(0)),
        ("frames", json!(4097)),
        ("recorded_nanoseconds", json!(1_000_000_000)),
        ("finished", json!(true)),
        ("kind", json!("replay")),
        ("extra", json!(0)),
    ] {
        let mut bad = base.clone();
        bad[field] = value;
        assert!(parse(&bad.to_string()).is_err(), "{field}");
    }
    for action in [
        "pause",
        "resume",
        "reset",
        "spawn",
        "remove",
        "get_state",
        "set_state",
    ] {
        let world = matches!(action, "pause" | "resume" | "reset");
        let mut good = base.clone();
        good["outcomes"] = json!([{"action":action,"model_id":if world { Value::Null } else { json!(42) },"ok":true}]);
        assert!(parse(&good.to_string()).is_ok());
        let mut bad = good.clone();
        bad["outcomes"][0]["model_id"] = json!(0);
        assert!(parse(&bad.to_string()).is_err());
        bad["outcomes"][0]
            .as_object_mut()
            .unwrap()
            .remove("model_id");
        assert!(parse(&bad.to_string()).is_err());
        if !world {
            good["outcomes"][0]["ok"] = json!(false);
            good["outcomes"][0]["model_id"] = json!(u64::MAX);
            assert!(parse(&good.to_string()).is_ok());
            good["outcomes"][0]["ok"] = json!(true);
            assert!(parse(&good.to_string()).is_err());
        }
    }
    let mut oversized = base.clone();
    oversized["outcomes"] = json!((0..9)
        .map(|_| json!({"action":"pause","model_id":null,"ok":true}))
        .collect::<Vec<_>>());
    assert!(parse(&oversized.to_string()).is_err());
    let text = base
        .to_string()
        .replacen("\"frame\":1", "\"frame\":1,\"frame\":1", 1);
    assert!(parse(&text).is_err());
}
#[test]
fn replay_requires_opt_in_matching_read_only_tuple_before_credit_or_controls() {
    let mut live = Session::default();
    live.opened();
    assert!(receive(&mut live, status(1, 0, 1, false)).is_err());
    assert!(live.enable_replay(true).is_err());
    for field in [
        "epoch",
        "scene_sequence",
        "recorded_seconds",
        "recorded_nanoseconds",
        "playing",
    ] {
        let mut s = session();
        receive(&mut s, scene(1, 0, true)).unwrap();
        receive(&mut s, catalog(1, 0)).unwrap();
        assert!(!s.can_control());
        assert!(receive(
            &mut s,
            json!({"version":1,"kind":"publication_credit","token":1})
        )
        .is_err());
        let mut bad = status(1, 0, 1, false);
        bad[field] = if field == "playing" {
            json!(true)
        } else {
            json!(9)
        };
        assert!(receive(&mut s, bad).is_err(), "{field}");
        assert!(s.replay.is_none());
    }
    let mut s = session();
    receive(&mut s, scene(1, 0, true)).unwrap();
    let mut writable = catalog(1, 0);
    writable["models"][0]["can_remove"] = json!(true);
    assert!(receive(&mut s, writable).is_err());
    tuple(&mut s, 2, 0, 1, false);
    assert!(s.can_control());
    for command in [
        lifecycle::Command::GetState { model_id: 2 },
        lifecycle::Command::Remove { model_id: 2 },
        lifecycle::Command::SetState {
            model_id: 2,
            state: lifecycle::State::default(),
        },
        lifecycle::Command::Spawn {
            template_id: 4,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
    ] {
        assert!(s.lifecycle(command).is_err());
        assert!(!s.pending_control());
    }
    assert!(receive(&mut s, status(2, 0, 1, false)).is_err());
    let request: Value = serde_json::from_str(&s.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(request["request_id"], 1);
}
#[test]
fn replay_restart_keeps_old_ready_once_and_requires_fresh_status_before_ack() {
    let mut s = session();
    tuple(&mut s, 1, 0, 1, false);
    receive(
        &mut s,
        json!({"version":1,"kind":"publication_credit","token":1}),
    )
    .unwrap();
    s.control(Action::Reset).unwrap();
    let ack = json!({"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null});
    receive(&mut s, scene(2, 1, true)).unwrap();
    assert!(receive(&mut s, ack.clone()).is_err());
    assert!(s.pending_control());
    receive(&mut s, catalog(2, 1)).unwrap();
    receive(&mut s, status(2, 1, 1, false)).unwrap();
    receive(&mut s, ack).unwrap();
    assert!(!s.pending_control());
    assert_eq!(s.publication_tokens(), (Some(1), Some(1)));
    assert_eq!(
        serde_json::from_str::<Value>(&s.publication_ready().unwrap()).unwrap()["token"],
        1
    );
    s.publication_ready_sent();
    assert!(s.publication_ready().is_none());
    tuple(&mut s, 3, 1, 1, false);
    receive(
        &mut s,
        json!({"version":1,"kind":"publication_credit","token":2}),
    )
    .unwrap();
    assert_eq!(s.publication_tokens(), (Some(2), Some(2)));
    s.disconnect("closed");
    assert!(s.replay.is_none());
    assert!(s.publication_ready().is_none());
}
#[test]
fn finished_cursor_refuses_play_without_consuming_id_and_frame_count_is_connection_bound() {
    let mut s = session();
    tuple(&mut s, 1, 0, 1, false);
    receive(&mut s, scene(2, 0, true)).unwrap();
    receive(&mut s, catalog(2, 0)).unwrap();
    let mut changed = status(2, 0, 2, false);
    changed["frames"] = json!(3);
    assert!(receive(&mut s, changed).is_err());
    let mut eof = status(2, 0, 2, false);
    eof["finished"] = json!(true);
    receive(&mut s, eof).unwrap();
    assert!(s.control(Action::Resume).is_err());
    assert!(!s.pending_control());
    let reset: Value = serde_json::from_str(&s.control(Action::Reset).unwrap()).unwrap();
    assert_eq!(reset["request_id"], 1);
}
