//! The `gz-parity` command line, run in-process and as the real binary.

// Test helpers outside #[test] fns may unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use common::*;
use gz_core::msgs;
use gz_core::snapshot::{SceneInfo, SnapNode, Snapshot, Xform, FORMAT};
use gz_parity::cli::{main_with, run};
use gz_parity::corpus::{Builder, Corpus};

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(ToString::to_string).collect()
}

fn code(c: ExitCode) -> String {
    format!("{c:?}")
}

fn ok(a: &[&str]) -> String {
    code(run(&args(a)).unwrap())
}

fn err(a: &[&str]) -> String {
    run(&args(a)).unwrap_err()
}

fn success() -> String {
    code(ExitCode::SUCCESS)
}

fn failure() -> String {
    code(ExitCode::FAILURE)
}

fn snapshot(nodes: &[(&str, f64)]) -> Snapshot {
    Snapshot {
        format: FORMAT.into(),
        source: "test".into(),
        world: "w".into(),
        scene: SceneInfo {
            ambient: None,
            clear_color: None,
            camera: None,
            grid_visible: None,
            sky: None,
        },
        nodes: nodes
            .iter()
            .map(|(path, x)| SnapNode {
                path: (*path).into(),
                kind: "model".into(),
                local: Some(Xform {
                    pos: [*x, 0.0, 0.0],
                    quat: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                }),
                world: None,
                visible: true,
                geometry: None,
                material: None,
                light: None,
            })
            .collect(),
    }
}

fn write_json(path: &Path, snap: &Snapshot) -> String {
    std::fs::write(path, serde_json::to_string(snap).unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn argument_errors() {
    assert!(err(&[]).starts_with("usage: gz-parity"));
    assert_eq!(err(&["frobnicate"]), "unknown command 'frobnicate'");
    assert_eq!(err(&["compare", "--expected"]), "--expected needs a value");
    assert_eq!(err(&["compare"]), "missing --expected");
    assert_eq!(err(&["record", "--url", "ws://x"]), "missing --out");
    assert_eq!(
        err(&[
            "record",
            "--out",
            "x",
            "--url",
            "ws://x",
            "--duration-ms",
            "soon"
        ]),
        "bad --duration-ms value 'soon'"
    );
    assert_eq!(err(&["replay"]), "missing --corpus");
}

#[test]
fn main_with_maps_errors_to_exit_code_2() {
    assert_eq!(code(main_with(args(&[]))), code(ExitCode::from(2)));
    let tmp = scratch("main-with");
    let s = write_json(&tmp.join("s.json"), &snapshot(&[("a", 0.0)]));
    assert_eq!(
        code(main_with(args(&[
            "compare",
            "--expected",
            &s,
            "--actual",
            &s
        ]))),
        success()
    );
}

#[test]
fn compare_passes_fails_suppresses_and_reports() {
    let tmp = scratch("compare");
    let e = write_json(&tmp.join("e.json"), &snapshot(&[("a", 0.0), ("b", 0.0)]));
    let a = write_json(&tmp.join("a.json"), &snapshot(&[("a", 1.0), ("c", 0.0)]));
    let report = tmp.join("report.json");
    let report_s = report.to_string_lossy().into_owned();

    assert_eq!(
        ok(&["compare", "--expected", &e, "--actual", &e]),
        success()
    );
    assert_eq!(
        ok(&[
            "compare",
            "--expected",
            &e,
            "--actual",
            &a,
            "--report",
            &report_s
        ]),
        failure()
    );
    let r: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(r["diffs"].as_array().unwrap().len(), 3);

    // Allow-list everything that differs: PASS, with suppressed diffs.
    let allow = tmp.join("allow.toml");
    std::fs::write(
        &allow,
        "[[allow]]\nid = \"all\"\nfield = \"*\"\nreason = \"test\"\n",
    )
    .unwrap();
    let allow_s = allow.to_string_lossy().into_owned();
    assert_eq!(
        ok(&[
            "compare",
            "--expected",
            &e,
            "--actual",
            &a,
            "--allow",
            &allow_s
        ]),
        success()
    );

    // Bad inputs.
    let missing = tmp.join("missing.json").to_string_lossy().into_owned();
    assert!(err(&["compare", "--expected", &missing, "--actual", &a]).contains("missing.json"));
    let garbage = tmp.join("garbage.json");
    std::fs::write(&garbage, "{").unwrap();
    let garbage_s = garbage.to_string_lossy().into_owned();
    assert!(err(&["compare", "--expected", &e, "--actual", &garbage_s]).contains("garbage.json"));
    assert!(err(&[
        "compare",
        "--expected",
        &e,
        "--actual",
        &a,
        "--allow",
        &missing
    ])
    .contains("missing.json"));
    let bad_report = tmp
        .join("no/such/dir/r.json")
        .to_string_lossy()
        .into_owned();
    assert!(err(&[
        "compare",
        "--expected",
        &e,
        "--actual",
        &a,
        "--report",
        &bad_report
    ])
    .contains("r.json"));
}

#[test]
fn compare_protocol_passes_fails_and_reports() {
    let tmp = scratch("protocol");
    let write = |name: &str, text: &str| {
        let p = tmp.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    };
    let e = write("e.log", "protos,,,\nasset,,,/a\n");
    let same = write("same.log", "protos,,,\nasset,,,/a\n");
    let a = write("a.log", "worlds,,,\nasset,,,/b\n");
    let report = tmp.join("r.json").to_string_lossy().into_owned();

    assert_eq!(ok(&["compare-protocol", &e, &same]), success());
    assert_eq!(
        ok(&["compare-protocol", &e, &a, "--report", &report]),
        failure()
    );
    let r: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(r["sequence_matches"], false);

    assert_eq!(
        err(&["compare-protocol", &e]),
        "compare-protocol needs EXPECTED.log ACTUAL.log"
    );
    let missing = tmp.join("missing.log").to_string_lossy().into_owned();
    assert!(err(&["compare-protocol", &e, &missing]).contains("missing.log"));
}

fn saved_corpus(name: &str) -> String {
    let pose = msgs::PoseV::default();
    let dir = scratch(name).join("corpus");
    Builder::new("w")
        .scene(&msgs::Scene::default())
        .publish("/world/w/dynamic_pose/info", "gz.msgs.Pose_V", &pose)
        .publish("/world/w/dynamic_pose/info", "gz.msgs.Pose_V", &pose)
        .build()
        .save(&dir)
        .unwrap();
    dir.to_string_lossy().into_owned()
}

#[test]
fn replay_serves_a_saved_corpus() {
    let corpus = saved_corpus("replay");
    let port = free_port().to_string();
    let p = port.clone();
    let server = std::thread::spawn(move || {
        code(
            run(&args(&[
                "replay",
                "--corpus",
                &corpus,
                "--port",
                &p,
                "--connections",
                "1",
                "--stream-limit",
                "1",
                "--key",
                "k",
            ]))
            .unwrap(),
        )
    });
    let mut ws = connect(&format!("ws://127.0.0.1:{port}"));
    send(&mut ws, "auth,,,k");
    assert_eq!(recv(&mut ws), b"authorized");
    send(&mut ws, "sub,/world/w/dynamic_pose/info,,");
    recv(&mut ws);
    assert!(silent(&mut ws, 200), "--stream-limit 1");
    ws.close(None).unwrap();
    let _ = ws.flush();
    assert_eq!(server.join().unwrap(), success());

    let missing = scratch("replay-missing")
        .join("nope")
        .to_string_lossy()
        .into_owned();
    assert!(!err(&["replay", "--corpus", &missing]).is_empty());
    let corpus = saved_corpus("replay-bad");
    assert_eq!(
        err(&["replay", "--corpus", &corpus, "--stream-limit", "lots"]),
        "bad --stream-limit"
    );
    assert_eq!(
        err(&["replay", "--corpus", &corpus, "--port", "high"]),
        "bad --port value 'high'"
    );
}

#[test]
fn record_writes_a_corpus() {
    let corpus = Corpus::load(Path::new(&saved_corpus("record-src"))).unwrap();
    let (url, server) = serve(corpus, opts(None, None));
    let out = scratch("record").join("out");
    let out_s = out.to_string_lossy().into_owned();
    assert_eq!(
        ok(&[
            "record",
            "--url",
            &url,
            "--out",
            &out_s,
            "--duration-ms",
            "100",
            "--max-stream-frames",
            "1",
            "--world-file",
            "w.sdf",
            "--timeout-ms",
            "5000",
        ]),
        success()
    );
    server.join().unwrap().unwrap();
    let c = Corpus::load(&out).unwrap();
    assert_eq!(c.manifest.world, "w");
    assert_eq!(c.manifest.streams["/world/w/dynamic_pose/info"].len(), 1);

    // Recording fails when nothing is listening.
    let url = format!("ws://127.0.0.1:{}", free_port());
    assert!(!err(&["record", "--url", &url, "--out", &out_s]).is_empty());
}

#[test]
fn binary_exits_2_with_usage() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_gz-parity"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: gz-parity"));
}
