// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde_json::{json, Value};
fn scene(sequence: u64, epoch: u64) -> ServerMessage {
    parse(&json!({"version":1,"kind":"scene","world":"default","world_id":1,"epoch":epoch,"seconds":0,"nanoseconds":0,"sequence":sequence,"paused":true,"visuals":[{"id":3,"name":"visual","translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.],"shape":{"kind":"box","size":[1.,1.,1.]},"color":[1.,1.,1.,1.]}]}).to_string()).unwrap()
}
fn catalog(sequence: u64) -> Value {
    json!({"version":1,"kind":"lifecycle_catalog","world_id":1,"epoch":0,"scene_sequence":sequence,"templates":[{"template_id":9,"label":"Local template"}],"models":[{"model_id":2,"name":"duplicate name","visual_ids":[3],"can_remove":true,"can_set_state":true}]})
}
fn ready() -> Session {
    let mut s = Session::default();
    s.enable_lifecycle(true);
    s.opened();
    s.receive(scene(1, 0)).unwrap();
    s.receive(parse(&catalog(1).to_string()).unwrap()).unwrap();
    s
}
fn ack(id: u64, action: &str, sequence: u64, ok: bool, state: Value) -> ServerMessage {
    parse(&json!({"version":1,"kind":"lifecycle_ack","request_id":id,"action":action,"world_id":1,"epoch":0,"scene_sequence":sequence,"seconds":0,"nanoseconds":0,"paused":true,"ok":ok,"error":if ok {Value::Null}else{json!("refused")},"model_id":if ok {json!(2)}else{Value::Null},"state":state}).to_string()).unwrap()
}
#[test]
fn lifecycle_and_controls_share_one_allocator_and_pending_slot_without_optimistic_scene_edits() {
    let mut s = ready();
    let request: Value = serde_json::from_str(
        &s.lifecycle(lifecycle::Command::Remove { model_id: 2 })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        request,
        json!({"version":1,"kind":"lifecycle","request_id":1,"world_id":1,"epoch":0,"action":"remove","model_id":2})
    );
    assert!(s.control(Action::Pause).is_err());
    assert!(s
        .lifecycle(lifecycle::Command::GetState { model_id: 2 })
        .is_err());
    assert_eq!(s.latest.as_ref().unwrap().visuals.len(), 1);
    s.receive(ack(1, "remove", 1, false, Value::Null)).unwrap();
    assert!(s.catalog.is_some());
    let control: Value = serde_json::from_str(&s.control(Action::Pause).unwrap()).unwrap();
    assert_eq!(control["request_id"], 2);
    assert!(s
        .lifecycle(lifecycle::Command::Remove { model_id: 2 })
        .is_err());
}
#[test]
fn scene_sequence_fences_paused_edits_and_reconnect_epoch_retire_lifecycle_state() {
    let mut s = ready();
    s.lifecycle(lifecycle::Command::GetState { model_id: 2 })
        .unwrap();
    assert!(s
        .receive(ack(
            1,
            "get_state",
            2,
            true,
            serde_json::to_value(lifecycle::State::default()).unwrap()
        ))
        .is_err());
    s.receive(scene(2, 0)).unwrap();
    s.receive(ack(
        1,
        "get_state",
        2,
        true,
        serde_json::to_value(lifecycle::State::default()).unwrap(),
    ))
    .unwrap();
    assert!(s.lifecycle_reply.as_ref().unwrap().state.is_some());
    assert!(
        s.lifecycle(lifecycle::Command::Remove { model_id: 2 })
            .is_err(),
        "old catalog cannot authorize a newer Scene"
    );
    s.receive(parse(&catalog(2).to_string()).unwrap()).unwrap();
    s.receive(scene(1, 1)).unwrap();
    assert!(s.catalog.is_none());
    assert!(s.lifecycle_reply.is_none());
    s.disconnect("closed");
    assert!(!s.can_control());
    assert!(s.catalog.is_none());
    s.opened();
    s.receive(scene(3, 0)).unwrap();
    assert!(s
        .lifecycle(lifecycle::Command::Remove { model_id: 2 })
        .is_err());
    let mut legacy = Session::default();
    legacy.opened();
    legacy.receive(scene(1, 0)).unwrap();
    assert!(legacy
        .receive(parse(&catalog(1).to_string()).unwrap())
        .is_err());
}
#[test]
fn catalog_and_ack_reject_unbounded_ownership_aliases_missing_fields_and_nonfinite_state() {
    for field in ["world_id", "model_id", "visual_ids"] {
        let mut c = catalog(1);
        match field {
            "world_id" => c[field] = json!(0),
            "model_id" => c["models"][0][field] = json!(1),
            _ => c["models"][0][field] = json!([]),
        };
        assert!(parse(&c.to_string()).is_err());
    }
    let mut c = catalog(1);
    let m = c["models"][0].clone();
    c["models"].as_array_mut().unwrap().push(m);
    assert!(parse(&c.to_string()).is_err());
    let mut c = catalog(1);
    c["models"][0]["model_id"] = json!(3);
    assert!(parse(&c.to_string()).is_err());
    let mut c = catalog(1);
    c["templates"] = json!((0..129)
        .map(|i| json!({"template_id":i+1,"label":"t"}))
        .collect::<Vec<_>>());
    assert!(parse(&c.to_string()).is_err());
    let mut s = ready();
    let state = lifecycle::State {
        rotation: [0.; 4],
        ..lifecycle::State::default()
    };
    assert!(s
        .lifecycle(lifecycle::Command::SetState { model_id: 2, state })
        .is_err());
    assert!(s
        .lifecycle(lifecycle::Command::Spawn {
            template_id: 55,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.]
        })
        .is_err());
    let mut c = catalog(1);
    c["models"][0]["visual_ids"] = json!([999]);
    assert!(s.receive(parse(&c.to_string()).unwrap()).is_err());
    let mut a = json!({"version":1,"kind":"lifecycle_ack","request_id":1,"action":"remove","world_id":1,"epoch":0,"scene_sequence":1,"seconds":0,"nanoseconds":0,"paused":true,"ok":false,"error":"refused","model_id":null,"state":null});
    a.as_object_mut().unwrap().remove("state");
    assert!(parse(&a.to_string()).is_err());
    let mut state = lifecycle::State::default();
    state.linear_velocity[0] = f64::INFINITY;
    assert!(state.validate().is_err());
}
#[test]
fn state_and_spawn_commands_preserve_world_basis_and_reject_contradictory_results() {
    let mut s = ready();
    let state = lifecycle::State {
        translation: [1., 2., 3.],
        rotation: [0., 0., 1., 0.],
        linear_velocity: [4., 5., 6.],
        angular_velocity: [7., 8., 9.],
    };
    let text = s
        .lifecycle(lifecycle::Command::SetState {
            model_id: 2,
            state: state.clone(),
        })
        .unwrap();
    let wire: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(wire["translation"], json!([1., 2., 3.]));
    assert_eq!(wire["rotation"], json!([0., 0., 1., 0.]));
    assert_eq!(wire["linear_velocity"], json!([4., 5., 6.]));
    assert!(s
        .receive(ack(1, "set_state", 1, true, Value::Null))
        .is_err());
    s.receive(ack(
        1,
        "set_state",
        1,
        true,
        serde_json::to_value(&state).unwrap(),
    ))
    .unwrap();
    assert_eq!(s.lifecycle_reply.as_ref().unwrap().state, Some(state));
    let text = s
        .lifecycle(lifecycle::Command::Spawn {
            template_id: 9,
            translation: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
        })
        .unwrap();
    let wire: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(wire["request_id"], 2);
    assert!(wire.get("uri").is_none());
    assert!(wire.get("name").is_none());
}

#[test]
fn successful_spawn_cannot_alias_visual_identity_and_state_quaternion_matches_server_unit_domain() {
    let mut s = ready();
    s.lifecycle(lifecycle::Command::Spawn {
        template_id: 9,
        translation: [0.; 3],
        rotation: [0., 0., 0., 1.],
    })
    .unwrap();
    let ServerMessage::LifecycleAck(mut reply) = ack(1, "spawn", 1, true, Value::Null) else {
        panic!("reply")
    };
    reply.model_id = Some(3);
    assert!(s.receive(ServerMessage::LifecycleAck(reply)).is_err());
    assert!(
        s.pending_control(),
        "invalid acknowledgement cannot retire the pending operation"
    );
    let mut state = lifecycle::State::default();
    state.rotation[3] = 1.0 + 1e-9;
    assert!(
        state.validate().is_err(),
        "new lifecycle commands use the server's strict unit domain"
    );
    state.rotation = [
        0.,
        0.,
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ];
    assert!(state.validate().is_ok());
}
