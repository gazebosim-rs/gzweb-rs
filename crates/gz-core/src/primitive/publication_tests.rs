// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde_json::{json, Value};
fn scene(sequence: u64, epoch: u64) -> ServerMessage {
    parse(&json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":epoch,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":true,"visuals":[{"id":3,"name":"visual","translation":[0,0,0],"rotation":[0,0,0,1],"shape":{"kind":"box","size":[1,1,1]},"color":[1,1,1,1]}]}).to_string()).unwrap()
}
fn catalog(sequence: u64, epoch: u64) -> ServerMessage {
    parse(&json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":epoch,"scene_sequence":sequence,"templates":[{"template_id":9,"label":"Box"}],"models":[{"model_id":2,"name":"box","visual_ids":[3],"can_remove":true,"can_set_state":true}]}).to_string()).unwrap()
}
fn credit(token: u64) -> ServerMessage {
    parse(&json!({"version":1,"kind":"publication_credit","token":token}).to_string()).unwrap()
}
fn enabled() -> Session {
    let mut s = Session::default();
    s.enable_lifecycle(true);
    s.enable_publication_credit(true).unwrap();
    s.opened();
    s
}
fn ready(s: &Session, token: u64) {
    let actual: Value = serde_json::from_str(&s.publication_ready().unwrap()).unwrap();
    assert_eq!(
        actual,
        json!({"version":1,"kind":"publication_ready","token":token})
    );
}
#[test]
fn strict_credit_metadata_and_explicit_opt_in_preserve_default_protocol() {
    for raw in [
        r#"{"version":1,"kind":"publication_credit"}"#,
        r#"{"version":1,"kind":"publication_credit","token":null}"#,
        r#"{"version":1,"kind":"publication_credit","token":1.5}"#,
        r#"{"version":1,"kind":"publication_credit","token":-1}"#,
        r#"{"version":1,"kind":"publication_credit","token":0}"#,
        r#"{"version":2,"kind":"publication_credit","token":1}"#,
        r#"{"version":1,"kind":"publication_credit","token":1,"extra":0}"#,
        r#"{"version":1,"kind":"publication_credit","token":1,"token":2}"#,
        r#"{"version":1,"version":1,"kind":"publication_credit","token":1}"#,
    ] {
        assert!(parse(raw).is_err(), "{raw}");
    }
    let mut s = Session::default();
    assert!(s.enable_publication_credit(true).is_err());
    s.opened();
    assert!(s.receive(credit(1)).is_err());
    s.receive(scene(1, 0)).unwrap();
    assert!(s.can_control());
    assert!(s.publication_ready().is_none());
    s.enable_lifecycle(true);
    s.receive(scene(2, 0)).unwrap();
    s.receive(catalog(2, 0)).unwrap();
    assert!(s.receive(credit(1)).is_err());
}
#[test]
fn fresh_matching_tuple_and_single_flight_are_independent_of_command_ids() {
    let mut s = enabled();
    assert!(s.receive(credit(1)).is_err());
    s.receive(scene(1, 0)).unwrap();
    assert!(s.receive(credit(1)).is_err());
    s.receive(catalog(1, 0)).unwrap();
    assert!(s.publication_ready().is_none());
    s.receive(credit(1)).unwrap();
    ready(&s, 1);
    let control: Value = serde_json::from_str(&s.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(control["request_id"], 1);
    s.receive(scene(2, 0)).unwrap();
    s.receive(catalog(2, 0)).unwrap();
    assert!(s.receive(credit(2)).is_err());
    ready(&s, 1);
    s.publication_ready_sent();
    assert!(s.publication_ready().is_none());
    assert!(s.receive(credit(1)).is_err());
    s.receive(credit(2)).unwrap();
    ready(&s, 2);
    s.publication_ready_sent();
    // Neither a repeated catalog nor an ignored stale Scene is a fresh tuple.
    s.receive(catalog(2, 0)).unwrap();
    s.receive(scene(1, 0)).unwrap();
    assert!(s.receive(credit(3)).is_err());
    s.receive(scene(3, 0)).unwrap();
    s.receive(catalog(3, 0)).unwrap();
    s.receive(credit(u64::MAX)).unwrap();
    ready(&s, u64::MAX);
    s.publication_ready_sent();
    s.receive(scene(4, 0)).unwrap();
    s.receive(catalog(4, 0)).unwrap();
    assert!(s.receive(credit(u64::MAX)).is_err());
    assert!(s.receive(credit(1)).is_err());
}
#[test]
fn same_connection_reset_preserves_old_transport_ready_but_disconnect_restarts_it() {
    let mut s = enabled();
    s.receive(scene(1, 0)).unwrap();
    s.receive(catalog(1, 0)).unwrap();
    s.receive(credit(1)).unwrap();
    let command: Value = serde_json::from_str(&s.control(Action::Reset).unwrap()).unwrap();
    assert_eq!(command["request_id"], 1);
    s.receive(parse(&json!({"version":1,"kind":"ack","request_id":1,"ok":true,"epoch":1,"seconds":0,"nanoseconds":0,"paused":true,"error":null}).to_string()).unwrap()).unwrap();
    assert!(s.latest.is_none());
    assert!(s.catalog.is_none());
    ready(&s, 1);
    s.publication_ready_sent();
    s.receive(scene(1, 1)).unwrap();
    s.receive(catalog(1, 1)).unwrap();
    assert!(s.receive(credit(1)).is_err());
    s.receive(credit(2)).unwrap();
    ready(&s, 2);
    let next: Value = serde_json::from_str(&s.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(next["request_id"], 2);
    s.disconnect("old socket gone");
    assert!(s.publication_ready().is_none());
    s.opened();
    assert!(s.receive(credit(1)).is_err());
    s.receive(scene(1, 0)).unwrap();
    s.receive(catalog(1, 0)).unwrap();
    s.receive(credit(1)).unwrap();
    ready(&s, 1);
}
#[test]
fn committed_lifecycle_reply_does_not_replace_outstanding_normal_token() {
    let mut s = enabled();
    s.receive(scene(1, 0)).unwrap();
    s.receive(catalog(1, 0)).unwrap();
    s.receive(credit(10)).unwrap();
    let command: Value = serde_json::from_str(
        &s.lifecycle(lifecycle::Command::GetState { model_id: 2 })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(command["request_id"], 1);
    s.receive(scene(2, 0)).unwrap();
    s.receive(catalog(2, 0)).unwrap();
    s.receive(parse(&json!({"version":1,"kind":"lifecycle_ack","request_id":1,"action":"get_state","world_id":1,"epoch":0,"scene_sequence":2,"seconds":0,"nanoseconds":0,"paused":true,"ok":true,"error":null,"model_id":2,"state":{"translation":[0,0,0],"rotation":[0,0,0,1],"linear_velocity":[0,0,0],"angular_velocity":[0,0,0]}}).to_string()).unwrap()).unwrap();
    assert!(s.can_control());
    assert_eq!(s.lifecycle_reply.as_ref().unwrap().scene_sequence, 2);
    ready(&s, 10);
    s.publication_ready_sent();
    s.receive(scene(3, 0)).unwrap();
    s.receive(catalog(3, 0)).unwrap();
    s.receive(credit(11)).unwrap();
    ready(&s, 11);
}
