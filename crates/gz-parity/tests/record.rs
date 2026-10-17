//! The recorder, against the replay server and against scripted servers that
//! misbehave in specific ways.

// Test helpers outside #[test] fns may unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use common::*;
use gz_core::frame;
use gz_core::msgs;
use gz_parity::corpus::Builder;
use gz_parity::record::{record, RecordOptions};
use tungstenite::{Message, WebSocket};

fn options(url: &str) -> RecordOptions {
    RecordOptions {
        url: url.into(),
        duration: Duration::from_millis(150),
        max_stream_frames: 2,
        world_file: Some("w.sdf".into()),
        reply_timeout: Duration::from_secs(5),
        ca_cert: None,
    }
}

fn mesh(name: &str, filename: &str) -> msgs::Visual {
    msgs::Visual {
        name: name.into(),
        geometry: Some(msgs::Geometry {
            mesh: Some(msgs::MeshGeom {
                filename: filename.into(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn records_what_the_replay_server_serves() {
    let pbr = msgs::Visual {
        material: Some(msgs::Material {
            pbr: Some(msgs::material::Pbr {
                albedo_map: "file:///t/albedo.png".into(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..mesh(
            "pbr",
            "https://fuel.gazebosim.org/1.0/o/models/m/1/files/x.dae",
        )
    };
    let scene = msgs::Scene {
        name: "w".into(),
        model: vec![msgs::Model {
            name: "m".into(),
            visual: vec![mesh("model_level", "model://pkg/a.obj")],
            link: vec![msgs::Link {
                name: "l".into(),
                visual: vec![pbr, msgs::Visual::default()],
                ..Default::default()
            }],
            model: vec![msgs::Model {
                name: "inner".into(),
                link: vec![msgs::Link {
                    visual: vec![mesh("nested", "/n/b.stl")],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let pose = msgs::PoseV::default();
    let topic = "/world/w/dynamic_pose/info";
    let corpus = Builder::new("w")
        .scene(&scene)
        .asset("pkg/a.obj", b"obj")
        .asset("/t/albedo.png", b"png")
        .publish(topic, "gz.msgs.Pose_V", &pose)
        .publish(topic, "gz.msgs.Pose_V", &pose)
        .publish(topic, "gz.msgs.Pose_V", &pose)
        .build();
    let (url, server) = serve(corpus, opts(None, None));

    let c = record(&options(&url)).unwrap();
    server.join().unwrap().unwrap();
    let m = &c.manifest;
    assert_eq!(m.world, "w");
    assert_eq!(m.source["world_file"], "w.sdf");
    assert!(m.protos.is_some());
    for r in ["topics-types", "worlds", "scene"] {
        assert!(m.responses.contains_key(r), "{r}");
    }
    // HTTP URIs are left to the client; the unserved mesh is recorded as its
    // error reply.
    let assets: Vec<_> = m.assets.keys().map(String::as_str).collect();
    assert_eq!(assets, ["/n/b.stl", "/t/albedo.png", "pkg/a.obj"]);
    // Streams are capped at max_stream_frames.
    assert_eq!(m.streams[topic].len(), 2);
}

/// Runs `script` as a websocket server for one connection; returns its URL.
fn scripted(script: impl FnOnce(&mut WebSocket<TcpStream>) + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        script(&mut ws);
    });
    url
}

fn read_text(ws: &mut WebSocket<TcpStream>) -> String {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return t.to_string(),
            Ok(_) => {}
            Err(e) => panic!("server read: {e}"),
        }
    }
}

fn bin(ws: &mut WebSocket<TcpStream>, data: Vec<u8>) {
    ws.send(Message::Binary(data.into())).unwrap();
}

fn pub_frame(topic: &str, msg: &impl prost::Message) -> Vec<u8> {
    frame::build("pub", topic, "", &msg.encode_to_vec())
}

fn worlds(names: &[&str]) -> msgs::StringMsgV {
    msgs::StringMsgV {
        data: names.iter().map(ToString::to_string).collect(),
        ..Default::default()
    }
}

/// Drain until the client goes away.
fn drain(ws: &mut WebSocket<TcpStream>) {
    while ws.read().is_ok() {}
}

#[test]
fn tolerates_noise_and_warns_about_unanswered_assets() {
    let url = scripted(|ws| {
        assert_eq!(read_text(ws), "protos,,,");
        // Text frames work as well as binary ones.
        ws.send(Message::Text("syntax = \"proto3\";".into()))
            .unwrap();
        assert_eq!(read_text(ws), "topics-types,,,");
        assert_eq!(read_text(ws), "worlds,,,");
        ws.send(Message::Ping(vec![].into())).unwrap();
        bin(ws, b"no header".to_vec());
        bin(ws, frame::build("asset", "/x", "", b""));
        bin(ws, pub_frame("unrelated", &msgs::Empty::default()));
        bin(ws, pub_frame("worlds", &worlds(&["w"])));
        assert_eq!(read_text(ws), "scene,w,,");
        let scene = msgs::Scene {
            model: vec![msgs::Model {
                link: vec![msgs::Link {
                    visual: vec![mesh("v", "/never.obj")],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        bin(ws, pub_frame("scene", &scene));
        assert_eq!(read_text(ws), "asset,,,/never.obj");
        // Noise during the stream phase; the asset is never answered.
        bin(ws, b"no header".to_vec());
        bin(ws, frame::build("frobnicate", "", "", b""));
        drain(ws);
    });
    let c = record(&RecordOptions {
        duration: Duration::from_millis(50),
        reply_timeout: Duration::from_millis(400),
        ..options(&url)
    })
    .unwrap();
    assert_eq!(c.manifest.world, "w");
    assert!(c.manifest.assets.is_empty());
    assert!(c.manifest.streams.is_empty());
}

fn record_err(url: &str) -> String {
    record(&RecordOptions {
        reply_timeout: Duration::from_millis(300),
        ..options(url)
    })
    .unwrap_err()
}

#[test]
fn a_bad_ca_file_is_an_error() {
    let err = record(&RecordOptions {
        ca_cert: Some("/nonexistent/ca.pem".into()),
        ..options("wss://localhost:1")
    })
    .unwrap_err();
    assert!(err.contains("ca.pem"), "{err}");
}

#[test]
fn reports_handshake_failures() {
    // Nothing listening.
    let err = record_err(&format!("ws://127.0.0.1:{}", free_port()));
    assert!(!err.is_empty());

    // No reply to protos.
    let url = scripted(drain);
    assert_eq!(record_err(&url), "no reply to protos");

    // Protos, then silence.
    let url = scripted(|ws| {
        read_text(ws);
        bin(ws, b"protos".to_vec());
        drain(ws);
    });
    assert_eq!(record_err(&url), "timed out waiting for worlds/scene");

    // No worlds.
    let url = scripted(|ws| {
        read_text(ws);
        bin(ws, b"protos".to_vec());
        bin(ws, pub_frame("worlds", &worlds(&[])));
        drain(ws);
    });
    assert_eq!(record_err(&url), "server reported no worlds");

    // Undecodable worlds and scene.
    for topic in ["worlds", "scene"] {
        let url = scripted(move |ws| {
            read_text(ws);
            bin(ws, b"protos".to_vec());
            bin(ws, frame::build("pub", topic, "", &[0xff, 0xff]));
            drain(ws);
        });
        assert!(!record_err(&url).is_empty(), "{topic}");
    }

    // A clean close.
    let url = scripted(|ws| {
        read_text(ws);
        ws.close(None).unwrap();
        drain(ws);
    });
    assert_eq!(record_err(&url), "server closed the connection");

    // The TCP connection drops without a close handshake.
    let url = scripted(|ws| {
        read_text(ws);
    });
    assert!(!record_err(&url).is_empty());
}
