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

fn mesh_scene() -> Scene {
    let mut value = scene_value(1);
    value["version"] = json!(2);
    value["visuals"][0]["shape"] = json!({"kind":"mesh","uri":"http://127.0.0.1:9002/assets/7/models/arrow.gltf","scale":[2,3,5]});
    value["visuals"][0]["rotation"] = json!([
        0.,
        0.,
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2
    ]);
    match parse(&value.to_string()).unwrap() {
        ServerMessage::Scene(s) => s,
        _ => unreachable!(),
    }
}

#[test]
fn mesh_v2_preserves_materials_and_applies_y_up_rotation_and_scale_once() {
    let s = mesh_scene();
    let desc = s.description().unwrap();
    let node = &desc.roots[0];
    assert_eq!(node.pose.pos, [1., 2., 3.]);
    for value in node.pose.quat {
        assert!((value - 0.5).abs() < 1e-15);
    }
    assert_eq!(node.scale, [2., 5., 3.]);
    let visual = node.visual.as_ref().unwrap();
    assert!(
        !visual.material.present,
        "wire palette must not replace embedded glTF materials"
    );
    assert_eq!(
        visual.geometry,
        crate::scene::GeometryDesc::Mesh {
            uri: "http://127.0.0.1:9002/assets/7/models/arrow.gltf".into(),
            load_uri: "m2mesh://127.0.0.1:9002/7/assets/7/models/arrow.gltf".into(),
            submesh: None,
            center_submesh: false,
        }
    );
    let mut pose_only = s.clone();
    pose_only.sequence += 1;
    pose_only.visuals[0].translation = [8., 9., 10.];
    pose_only.visuals[0].rotation = [0., 0., 0., 1.];
    assert!(s.same_layout(&pose_only));
    let rotation = pose_only.visuals[0].render_rotation();
    assert_eq!(
        rotation,
        [
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            0.,
            std::f64::consts::FRAC_1_SQRT_2
        ]
    );
    if let Shape::Mesh { scale, .. } = &mut pose_only.visuals[0].shape {
        scale[1] = 4.;
    }
    assert!(!s.same_layout(&pose_only));
    let mut changed_uri = s.clone();
    if let Shape::Mesh { uri, .. } = &mut changed_uri.visuals[0].shape {
        *uri = "http://127.0.0.1:9002/assets/7/models/arrow.glb".into();
    }
    assert!(!s.same_layout(&changed_uri));
    let mut wrong_version = s;
    wrong_version.version = 1;
    assert!(wrong_version.validate().is_err());
}

#[test]
fn mesh_uris_scales_and_dependency_scopes_are_bounded() {
    for uri in [
        "https://127.0.0.1:9002/assets/7/a.gltf",
        "http://localhost:9002/assets/7/a.gltf",
        "http://127.0.0.2:9002/assets/7/a.gltf",
        "http://127.0.0.1:0/assets/7/a.gltf",
        "http://127.0.0.1:65536/assets/7/a.gltf",
        "http://127.0.0.1:9002/assets/scope/a.gltf",
        "file:///tmp/a.gltf",
        "model://a/a.gltf",
        "http://remote/assets/7/a.gltf",
        "http://127.0.0.1:9002/assets/7/../a.gltf",
        "http://127.0.0.1:9002/assets/7/a//b.gltf",
        "http://127.0.0.1:9002/assets/7/a%2fb.gltf",
        "http://127.0.0.1:9002/assets/7/a.gltf?q=1",
        "http://127.0.0.1:9002/assets/7/a.gltf#Scene0",
        "http://127.0.0.1:9002/assets/7/a\\b.gltf",
        "http://127.0.0.1:9002/assets/7/a.obj",
    ] {
        assert!(local_mesh_asset_uri(uri).is_err(), "{uri}");
    }
    for value in [
        0.,
        -1.,
        f64::INFINITY,
        f64::NAN,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        let mut s = mesh_scene();
        if let Shape::Mesh { scale, .. } = &mut s.visuals[0].shape {
            scale[0] = value;
        }
        assert!(s.validate().is_err(), "{value}");
    }
    let base = "http://127.0.0.1:9002/assets/7/models/arrow.gltf";
    for relative in [
        "part.bin",
        "../shared.bin",
        "textures/albedo.png",
        "./part.bin",
        "data:application/octet-stream;base64,AAAA",
    ] {
        assert!(local_mesh_dependency_allowed(base, relative), "{relative}");
    }
    for relative in [
        "../../outside.bin",
        "../../../8/assets/8/a.bin",
        "/assets/8/a.bin",
        "https://remote/a.bin",
        "gzhttps://remote/a.bin",
        "%2e%2e/a.bin",
        "",
        "../..",
        "x\\a.bin",
    ] {
        assert!(!local_mesh_dependency_allowed(base, relative), "{relative}");
    }
    assert_eq!(
        local_mesh_http_url("127.0.0.1:9002/7/assets/7/shared.bin").unwrap(),
        "http://127.0.0.1:9002/assets/7/shared.bin"
    );
    assert!(local_mesh_http_url("127.0.0.1:9002/7/assets/8/a.bin").is_err());
    assert!(local_mesh_http_url("127.0.0.1:9002/7/assets/7/../a.bin").is_err());
    assert!(local_mesh_http_url("remote:9002/7/assets/7/a.bin").is_err());
    assert!(!local_mesh_dependency_allowed("not a base", "part.bin"));
}

#[test]
fn recovery_reports_leave_the_clock_and_pending_control_authoritative() {
    let report = |phase: &str, tick: u64, pending: bool| {
        json!({"version":1,"kind":"recovery_status","phase":phase,"epoch":0,"tick":tick,
            "attempt":1,"budget":3,"pause_pending":pending})
    };
    let active = report("reopening", 150, false);
    assert!(ready()
        .receive(parse(&active.to_string()).unwrap())
        .is_err());
    let mut session = Session::default();
    session.enable_recovery(true).unwrap();
    session.opened();
    session.receive(ServerMessage::Scene(scene(1))).unwrap();
    let clock = session.clock();
    session
        .receive(parse(&active.to_string()).unwrap())
        .unwrap();
    session.control(Action::Pause).unwrap();
    assert!(!session.waiting_for_recovery_pause());
    session
        .receive(parse(&report("reopening", 150, true).to_string()).unwrap())
        .unwrap();
    assert!(session.waiting_for_recovery_pause());
    assert_eq!(session.clock(), clock);
    assert!(session
        .recovery
        .unwrap()
        .label()
        .contains("Pause requested"));
    // An old idle message cannot clear a more recent report.
    session
        .receive(parse(&report("idle", 149, false).to_string()).unwrap())
        .unwrap();
    assert!(session.recovery.is_some());
    session
        .receive(parse(&report("idle", 150, false).to_string()).unwrap())
        .unwrap();
    assert!(session.recovery.is_none());
    assert!(session.pending_control());
    session.receive(ack(1, 0, true)).unwrap();
    assert_eq!(session.paused(), Some(true));
    session
        .receive(parse(&active.to_string()).unwrap())
        .unwrap();
    session.disconnect("test");
    assert!(session.recovery.is_none());

    for (key, value) in [
        ("version", json!(2)),
        ("attempt", json!(0)),
        ("budget", json!(0)),
        ("budget", json!(9)),
        ("phase", json!("unknown")),
        ("other", json!(1)),
    ] {
        let mut bad = active.clone();
        bad[key] = value;
        assert!(parse(&bad.to_string()).is_err(), "{bad}");
    }
}

const FENCE_TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn operator_fence(token: &str, eligible: bool) -> ServerMessage {
    parse(
        &json!({"version":1,"kind":"operator_fence","epoch":3,"tick":2150,"token":token,
        "eligible":eligible,"reason":"Transport loss at the held checkpoint"})
        .to_string(),
    )
    .unwrap()
}

fn operator_scene(paused: bool) -> ServerMessage {
    let mut scene = scene(1);
    scene.epoch = 3;
    scene.seconds = 2;
    scene.nanoseconds = 150_000_000;
    scene.paused = paused;
    ServerMessage::Scene(scene)
}

fn parked(eligible: bool) -> Session {
    let mut s = Session::default();
    s.enable_operator(true).unwrap();
    s.opened();
    s.receive(operator_fence(FENCE_TOKEN, eligible)).unwrap();
    assert!(s.recover_workers(FENCE_TOKEN).is_err());
    s.receive(operator_scene(false)).unwrap();
    s
}

#[test]
fn a_parked_scene_refuses_control_and_a_recover_click_binds_to_its_offered_token() {
    let mut s = parked(true);
    for action in [Action::Pause, Action::Resume, Action::Reset] {
        assert!(s.control(action).is_err());
    }
    assert!(s.recover_workers("stale offer").is_err());
    let req: Value = serde_json::from_str(&s.recover_workers(FENCE_TOKEN).unwrap()).unwrap();
    assert_eq!(
        req,
        json!({"version":1,"kind":"recover_workers","request_id":1,
        "epoch":3,"tick":2150,"token":FENCE_TOKEN})
    );
    s.receive(operator_fence(FENCE_TOKEN, true)).unwrap();
    assert!(s.recover_workers(FENCE_TOKEN).is_err());
    for (id, accepted, error) in [(2, true, Value::Null), (1, false, json!("Reopen refused"))] {
        let incoming = parse(&json!({"version":1,"kind":"operator_reply","request_id":id,"accepted":accepted,"error":error}).to_string()).unwrap();
        assert_eq!(s.receive(incoming).is_ok(), id == 1);
    }
    assert!(s
        .operator
        .as_ref()
        .unwrap()
        .label()
        .contains("Reopen refused"));
    assert!(s.recover_workers(FENCE_TOKEN).is_err());
    assert!(parked(false).recover_workers(FENCE_TOKEN).is_err());
}

#[test]
fn admitted_recovery_requires_a_new_verified_paused_scene_or_a_new_parked_offer() {
    for fresh_fence in [false, true] {
        let mut s = parked(true);
        s.recover_workers(FENCE_TOKEN).unwrap();
        s.receive(parse(r#"{"version":1,"kind":"operator_reply","request_id":1,"accepted":true,"error":null}"#).unwrap()).unwrap();
        assert!(!s.can_control());
        s.disconnect("handoff");
        s.opened();
        assert!(s.recover_workers(FENCE_TOKEN).is_err());
        if fresh_fence {
            let replacement = "1123456789abcdef0123456789abcdef";
            s.receive(operator_fence(replacement, true)).unwrap();
            s.receive(operator_scene(false)).unwrap();
            assert!(s.recover_workers(FENCE_TOKEN).is_err());
            assert!(s.recover_workers(replacement).is_ok());
        } else {
            s.receive(restored(FENCE_TOKEN)).unwrap();
            s.receive(operator_scene(true)).unwrap();
            assert!(s.can_control());
            assert!(s.operator.is_none());
            assert_eq!(s.paused(), Some(true));
        }
    }
}

#[test]
fn operator_fence_requires_negotiation_and_cannot_be_replaced_during_a_connection() {
    let mut legacy = ready();
    assert!(legacy.receive(operator_fence(FENCE_TOKEN, true)).is_err());
    let mut s = parked(true);
    assert!(s
        .receive(operator_fence("22222222222222222222222222222222", true))
        .is_err());
    assert!(s.receive(operator_fence(FENCE_TOKEN, false)).is_err());
    for token in ["", "AA", "abcd0000abcd0000abcd0000abcd000Z"] {
        assert!(parse(&json!({"version":1,"kind":"operator_fence","epoch":3,"tick":0,"token":token,"eligible":false,"reason":"inspect"}).to_string()).is_err());
    }
}

fn restored(token: &str) -> ServerMessage {
    parse(
        &json!({"version":1,"kind":"operator_restored","epoch":3,"tick":2150,"token":token})
            .to_string(),
    )
    .unwrap()
}

#[test]
fn missing_or_mismatched_restoration_identity_leaves_a_replacement_world_read_only() {
    for case in ["missing", "token", "clock", "running", "world"] {
        let mut s = parked(true);
        s.recover_workers(FENCE_TOKEN).unwrap();
        s.disconnect("possible handoff");
        s.opened();
        if case != "missing" {
            s.receive(restored(if case == "token" {
                "1123456789abcdef0123456789abcdef"
            } else {
                FENCE_TOKEN
            }))
            .unwrap();
        }
        let ServerMessage::Scene(mut scene) = operator_scene(case != "running") else {
            panic!()
        };
        if case == "clock" {
            scene.nanoseconds += 1_000_000;
        }
        if case == "world" {
            scene.world_id += 1;
        }
        s.receive(ServerMessage::Scene(scene)).unwrap();
        assert!(s.latest.is_some());
        assert!(!s.can_control());
        assert!(matches!(
            s.operator.as_ref().unwrap().phase,
            operator::OperatorPhase::Unverified(_)
        ));
        assert!(s.recover_workers(FENCE_TOKEN).is_err());
        assert!(s.receive(restored(FENCE_TOKEN)).is_err());
    }
    let mut ordinary = Session::default();
    ordinary.enable_operator(true).unwrap();
    ordinary.opened();
    ordinary.receive(restored(FENCE_TOKEN)).unwrap();
    ordinary.receive(operator_scene(false)).unwrap();
    assert!(ordinary.can_control());
}
