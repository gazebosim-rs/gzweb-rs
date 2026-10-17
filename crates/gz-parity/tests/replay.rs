//! The replay server, driven by a real websocket client.

// Test helpers outside #[test] fns may unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::Write;
use std::net::TcpStream;

use common::*;
use gz_core::frame::Frame;
use gz_core::msgs;
use gz_parity::corpus::{Builder, Corpus};
use gz_parity::replay::{self, ReplayOptions};
use prost::Message as _;
use tungstenite::Message;

fn corpus() -> Corpus {
    let pose = msgs::PoseV::default();
    Builder::new("w")
        .scene(&msgs::Scene {
            name: "w".into(),
            ..Default::default()
        })
        .asset("/a.obj", b"obj")
        .publish("/t", "gz.msgs.Pose_V", &pose)
        .publish("/t", "gz.msgs.Pose_V", &pose)
        .publish("/t", "gz.msgs.Pose_V", &pose)
        .build()
}

fn op(data: &[u8]) -> (String, String) {
    let f = Frame::parse(data).unwrap();
    (f.op.to_string(), f.topic.to_string())
}

#[test]
fn serves_a_full_session_and_logs_client_frames() {
    let tmp = scratch("session");
    let log = tmp.join("client.log");
    let (url, server) = serve(
        corpus(),
        ReplayOptions {
            stream_limit: Some(2),
            ..opts(Some(log.clone()), Some("k"))
        },
    );

    // A plain-TCP probe fails the handshake and doesn't count as a connection.
    let mut probe = TcpStream::connect(url.trim_start_matches("ws://")).unwrap();
    probe.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    drop(probe);

    let mut ws = connect(&url);
    send(&mut ws, "auth,,,wrong");
    assert_eq!(recv(&mut ws), b"invalid");
    send(&mut ws, "auth,,,k");
    assert_eq!(recv(&mut ws), b"authorized");
    send(&mut ws, "protos,,,");
    assert_eq!(recv(&mut ws), b"syntax = \"proto3\";");
    send(&mut ws, "topics-types,,,");
    assert_eq!(op(&recv(&mut ws)), ("pub".into(), "topics-types".into()));
    send(&mut ws, "worlds,,,");
    assert_eq!(op(&recv(&mut ws)), ("pub".into(), "worlds".into()));
    send(&mut ws, "scene,w,,");
    assert_eq!(op(&recv(&mut ws)), ("pub".into(), "scene".into()));

    // Both asset request forms; unknown assets get a StringMsg error.
    send(&mut ws, "asset,,,/a.obj");
    assert_eq!(op(&recv(&mut ws)), ("asset".into(), "/a.obj".into()));
    send(&mut ws, "asset,/a.obj,,");
    assert_eq!(op(&recv(&mut ws)), ("asset".into(), "/a.obj".into()));
    send(&mut ws, "asset,,,/nope");
    let data = recv(&mut ws);
    let f = Frame::parse(&data).unwrap();
    assert_eq!(
        (f.op, f.topic, f.msg_type),
        ("asset", "/nope", "gz.msgs.StringMsg")
    );
    assert_eq!(
        msgs::StringMsg::decode(f.payload).unwrap().data,
        "asset_not_found"
    );

    // Streams honour --stream-limit; unknown topics and ops get nothing.
    send(&mut ws, "sub,/t,,");
    assert_eq!(op(&recv(&mut ws)), ("pub".into(), "/t".into()));
    assert_eq!(op(&recv(&mut ws)), ("pub".into(), "/t".into()));
    send(&mut ws, "sub,/other,,");
    send(&mut ws, "no header");
    send(&mut ws, "frobnicate,,,");
    ws.send(Message::Binary(vec![0xab, 0x01].into())).unwrap();
    ws.send(Message::Ping(vec![].into())).unwrap();
    assert!(silent(&mut ws, 200));
    ws.close(None).unwrap();
    let _ = ws.flush();
    server.join().unwrap().unwrap();

    let log = std::fs::read_to_string(log).unwrap();
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines[0], "auth,,,wrong");
    assert!(lines.contains(&"asset,/a.obj,,"));
    assert!(lines.contains(&"binary:ab01"));
    assert_eq!(lines.last(), Some(&"binary:ab01"));
}

#[test]
fn missing_replies_are_skipped_and_bad_frame_indices_end_the_connection() {
    let mut c = Corpus::default();
    c.manifest.world = "w".into();
    c.manifest.assets.insert("/broken".into(), 42);
    let (url, server) = serve(c, opts(None, None));
    let mut ws = connect(&url);
    // Any key is accepted without --key.
    send(&mut ws, "auth,,,anything");
    assert_eq!(recv(&mut ws), b"authorized");
    send(&mut ws, "protos,,,");
    send(&mut ws, "worlds,,,");
    ws.send(Message::Binary(vec![1].into())).unwrap();
    assert!(silent(&mut ws, 200));
    send(&mut ws, "asset,,,/broken");
    // The server gives up on the connection (and, with connections = 1, exits).
    server.join().unwrap().unwrap();
    assert!(ws.read().is_err() || silent(&mut ws, 200));
}

#[test]
fn serve_binds_the_requested_port() {
    let port = free_port();
    let server = std::thread::spawn(move || {
        replay::serve(
            &corpus(),
            &ReplayOptions {
                port,
                ..opts(None, None)
            },
        )
    });
    let mut ws = connect(&format!("ws://127.0.0.1:{port}"));
    send(&mut ws, "protos,,,");
    assert_eq!(recv(&mut ws), b"syntax = \"proto3\";");
    ws.close(None).unwrap();
    let _ = ws.flush();
    server.join().unwrap().unwrap();
}

#[test]
fn serve_reports_setup_errors() {
    // The port is taken.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = replay::serve(
        &corpus(),
        &ReplayOptions {
            port,
            ..opts(None, None)
        },
    );
    assert!(err.is_err());

    // The client log can't be created.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bad_log = scratch("bad-log").join("no/such/dir/log");
    assert!(replay::serve_on(listener, &corpus(), &opts(Some(bad_log), None)).is_err());
}

#[test]
fn serves_successive_connections_and_survives_bad_clients() {
    let (url, server) = serve(
        corpus(),
        ReplayOptions {
            connections: 3,
            ..opts(None, None)
        },
    );
    // 1: vanishes without a close handshake.
    let mut ws = connect(&url);
    send(&mut ws, "protos,,,");
    recv(&mut ws);
    drop(ws);

    // 2: sends a text frame that isn't UTF-8.
    let stream = TcpStream::connect(url.trim_start_matches("ws://")).unwrap();
    let (mut ws, _) = tungstenite::client(url.as_str(), stream).unwrap();
    // FIN + text, masked (zero mask), 2 bytes: 0xff 0xfe.
    ws.get_mut()
        .write_all(&[0x81, 0x82, 0, 0, 0, 0, 0xff, 0xfe])
        .unwrap();
    ws.get_mut().flush().unwrap();

    // 3: a normal session still works.
    let mut ws = connect(&url);
    send(&mut ws, "protos,,,");
    assert_eq!(recv(&mut ws), b"syntax = \"proto3\";");
    ws.close(None).unwrap();
    let _ = ws.flush();
    server.join().unwrap().unwrap();
}

#[test]
fn answers_services_and_topics_like_gz_sim() {
    let tmp = scratch("services");
    let log = tmp.join("client.log");
    let (url, server) = serve(corpus(), opts(Some(log.clone()), None));
    let mut ws = connect(&url);

    send(
        &mut ws,
        "req,/world/w/control,gz.msgs.WorldControl,\x10\x01",
    );
    let reply = recv(&mut ws);
    let f = Frame::parse(&reply).unwrap();
    assert_eq!(
        (f.op, f.topic, f.msg_type),
        ("req", "/world/w/control", "gz.msgs.Boolean")
    );
    assert!(msgs::Boolean::decode(f.payload).unwrap().data);

    // Binary frames (non-UTF-8 payloads) are handled and logged as hex.
    let mut frame = b"req,/server_control,gz.msgs.ServerControl,".to_vec();
    frame.extend([0x30, 0x81, 0x01]);
    ws.send(Message::Binary(frame.into())).unwrap();
    assert_eq!(op(&recv(&mut ws)), ("req".into(), "/server_control".into()));

    send(&mut ws, "req,/nope,gz.msgs.Empty,\x0a\x02");
    let reply = recv(&mut ws);
    let f = Frame::parse(&reply).unwrap();
    assert_eq!(
        msgs::StringMsg::decode(f.payload).unwrap().data,
        "service_not_found"
    );

    send(&mut ws, "topics,,,");
    let reply = recv(&mut ws);
    let f = Frame::parse(&reply).unwrap();
    assert_eq!(msgs::StringMsgV::decode(f.payload).unwrap().data, ["/t"]);
    ws.close(None).unwrap();
    while ws.read().is_ok() {}
    server.join().unwrap().unwrap();
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("binary:7265712c"), "{log}");
}

#[test]
fn drops_the_first_connection_when_asked() {
    let (url, server) = serve(
        corpus(),
        ReplayOptions {
            connections: 2,
            drop_first_after: Some(std::time::Duration::from_millis(100)),
            ..opts(None, None)
        },
    );
    // The first connection is closed by the server.
    let mut first = connect(&url);
    loop {
        match first.read() {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    // The second one is not.
    let mut second = connect(&url);
    send(&mut second, "protos,,,");
    assert!(!recv(&mut second).is_empty());
    std::thread::sleep(std::time::Duration::from_millis(200));
    send(&mut second, "worlds,,,");
    assert_eq!(op(&recv(&mut second)), ("pub".into(), "worlds".into()));
    second.close(None).unwrap();
    while second.read().is_ok() {}
    server.join().unwrap().unwrap();
}
