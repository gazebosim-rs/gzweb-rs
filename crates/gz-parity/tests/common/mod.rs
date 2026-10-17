//! Helpers shared by the gz-parity integration tests.
#![allow(dead_code)]

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gz_parity::corpus::Corpus;
use gz_parity::replay::{self, ReplayOptions};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub type Client = WebSocket<MaybeTlsStream<TcpStream>>;

/// A fresh, empty temp directory unique to this process and `name`.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gz-parity-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn opts(client_log: Option<PathBuf>, key: Option<&str>) -> ReplayOptions {
    ReplayOptions {
        port: 0,
        client_log,
        stream_limit: None,
        connections: 1,
        key: key.map(str::to_string),
        ..Default::default()
    }
}

/// Serves `corpus` on an ephemeral port in a thread; returns the URL and the
/// thread, which ends after `opts.connections` connections.
pub fn serve(
    corpus: Corpus,
    opts: ReplayOptions,
) -> (String, std::thread::JoinHandle<Result<(), String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || replay::serve_on(listener, &corpus, &opts));
    (url, handle)
}

/// A port nothing is listening on (bound, then released).
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Connects, retrying while the server starts.
pub fn connect(url: &str) -> Client {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tungstenite::connect(url) {
            Ok((ws, _)) => return ws,
            Err(e) if Instant::now() > deadline => panic!("connect {url}: {e}"),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

pub fn send(ws: &mut Client, text: &str) {
    ws.send(Message::Text(text.into())).unwrap();
}

/// The next text or binary message's bytes.
pub fn recv(ws: &mut Client) -> Vec<u8> {
    loop {
        match ws.read().unwrap() {
            Message::Text(t) => return t.as_bytes().to_vec(),
            Message::Binary(b) => return b.to_vec(),
            _ => {}
        }
    }
}

/// True if no data message arrives within `ms`.
pub fn silent(ws: &mut Client, ms: u64) -> bool {
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        tcp.set_read_timeout(Some(Duration::from_millis(ms)))
            .unwrap();
    }
    let quiet = loop {
        match ws.read() {
            Ok(Message::Text(_) | Message::Binary(_)) => break false,
            Ok(_) => {}
            Err(_) => break true,
        }
    };
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        tcp.set_read_timeout(None).unwrap();
    }
    quiet
}
