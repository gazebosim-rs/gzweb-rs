//! Replays a recorded corpus as a websocket server (`.agent/PARITY.md` §2)
//! and logs every client frame.

use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use gz_core::frame::{self, Frame};
use gz_core::msgs;
use prost::Message as _;
use tungstenite::Message;

use crate::corpus::Corpus;

#[derive(Default)]
pub struct ReplayOptions {
    pub port: u16,
    pub client_log: Option<PathBuf>,
    pub stream_limit: Option<usize>,
    /// Exit after this many connections (0 = serve forever).
    pub connections: usize,
    /// When set, `auth` must carry this key (gz-launch `authorization_key`);
    /// otherwise every key is accepted.
    pub key: Option<String>,
    /// Close the first connection this long after it opens, to exercise a
    /// client's reconnect.
    pub drop_first_after: Option<Duration>,
}

pub fn serve(corpus: &Corpus, opts: &ReplayOptions) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", opts.port)).map_err(crate::message)?;
    serve_on(listener, corpus, opts)
}

/// Like [`serve`], on an already bound listener (`opts.port` is ignored).
pub fn serve_on(
    listener: TcpListener,
    corpus: &Corpus,
    opts: &ReplayOptions,
) -> Result<(), String> {
    let port = listener.local_addr().map_err(crate::message)?.port();
    eprintln!(
        "replay: serving '{}' on ws://127.0.0.1:{port}",
        corpus.manifest.world
    );
    if let Some(path) = &opts.client_log {
        std::fs::write(path, "").map_err(crate::message)?;
    }

    let mut served = 0;
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("replay: accept failed: {e}");
                continue;
            }
        };
        // Browsers probe with plain HTTP sometimes; ignore failed handshakes.
        let mut ws = match tungstenite::accept(stream) {
            Ok(ws) => ws,
            Err(e) => {
                eprintln!("replay: handshake failed: {e}");
                continue;
            }
        };
        let drop_at = opts
            .drop_first_after
            .filter(|_| served == 0)
            .map(|d| std::time::Instant::now() + d);
        if let Err(e) = handle(&mut ws, corpus, opts, drop_at) {
            eprintln!("replay: connection ended: {e}");
        }
        served += 1;
        if opts.connections > 0 && served >= opts.connections {
            break;
        }
    }
    Ok(())
}

fn handle(
    ws: &mut tungstenite::WebSocket<std::net::TcpStream>,
    corpus: &Corpus,
    opts: &ReplayOptions,
    drop_at: Option<std::time::Instant>,
) -> Result<(), String> {
    if drop_at.is_some() {
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(10)))
            .map_err(crate::message)?;
    }
    let mut log = match &opts.client_log {
        Some(p) => Some(
            std::fs::OpenOptions::new()
                .append(true)
                .open(p)
                .map_err(crate::message)?,
        ),
        None => None,
    };
    let m = &corpus.manifest;
    let send_idx =
        |ws: &mut tungstenite::WebSocket<std::net::TcpStream>, idx: usize| -> Result<(), String> {
            let data = corpus
                .frame(idx)
                .ok_or(format!("corpus has no frame {idx}"))?;
            ws.send(Message::Binary(data.to_vec().into()))
                .map_err(crate::message)
        };

    loop {
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if drop_at.is_some_and(|at| std::time::Instant::now() >= at) {
                    eprintln!("replay: dropping the connection");
                    let _ = ws.close(None);
                    return Ok(());
                }
                continue;
            }
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::Protocol(_)) => {
                return Ok(())
            }
            Err(e) => return Err(e.to_string()),
        };
        // Clients send text frames, or binary ones for non-UTF-8 payloads.
        let data: Vec<u8> = match msg {
            Message::Text(t) => {
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{t}");
                }
                t.as_bytes().to_vec()
            }
            Message::Binary(b) => {
                if let Some(f) = log.as_mut() {
                    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                    let _ = writeln!(f, "binary:{hex}");
                }
                b.to_vec()
            }
            Message::Close(_) => return Ok(()),
            _ => continue,
        };

        let Some(fr) = Frame::parse(&data) else {
            continue;
        };
        match fr.op {
            "auth" => {
                let ok = opts
                    .key
                    .as_deref()
                    .is_none_or(|k| k.as_bytes() == fr.payload);
                let reply = if ok { "authorized" } else { "invalid" };
                ws.send(Message::Text(reply.into()))
                    .map_err(crate::message)?;
            }
            "protos" => {
                if let Some(i) = m.protos {
                    send_idx(ws, i)?;
                }
            }
            "topics-types" | "worlds" | "scene" => {
                if let Some(&i) = m.responses.get(fr.op) {
                    send_idx(ws, i)?;
                }
            }
            "asset" => {
                // gzweb sends `asset,,,<uri>`; accept `asset,<uri>,,` too.
                let uri = if fr.topic.is_empty() {
                    String::from_utf8_lossy(fr.payload).into_owned()
                } else {
                    fr.topic.to_string()
                };
                match m.assets.get(&uri) {
                    Some(&i) => send_idx(ws, i)?,
                    None => {
                        let err = msgs::StringMsg {
                            data: "asset_not_found".into(),
                            ..Default::default()
                        };
                        let data = frame::build_binary(
                            "asset",
                            &uri,
                            "gz.msgs.StringMsg",
                            &err.encode_to_vec(),
                        );
                        ws.send(Message::Binary(data.into()))
                            .map_err(crate::message)?;
                    }
                }
            }
            "topics" => {
                let names = msgs::StringMsgV {
                    data: m.streams.keys().cloned().collect(),
                    ..Default::default()
                };
                let data = frame::build_binary(
                    "pub",
                    "topics",
                    "gz.msgs.StringMsg_V",
                    &names.encode_to_vec(),
                );
                ws.send(Message::Binary(data.into()))
                    .map_err(crate::message)?;
            }
            // Like gz-sim: world control and server control answer
            // `Boolean { data: true }`; nothing else exists.
            "req" => {
                let world_control = format!("/world/{}/control", m.world);
                let data = if fr.topic == world_control || fr.topic == "/server_control" {
                    let ok = msgs::Boolean {
                        data: true,
                        ..Default::default()
                    };
                    frame::build_binary("req", fr.topic, "gz.msgs.Boolean", &ok.encode_to_vec())
                } else {
                    let err = msgs::StringMsg {
                        data: "service_not_found".into(),
                        ..Default::default()
                    };
                    frame::build_binary("req", fr.topic, "gz.msgs.StringMsg", &err.encode_to_vec())
                };
                ws.send(Message::Binary(data.into()))
                    .map_err(crate::message)?;
            }
            "sub" => {
                if let Some(frames) = m.streams.get(fr.topic) {
                    let limit = opts.stream_limit.unwrap_or(usize::MAX);
                    for &i in frames.iter().take(limit) {
                        send_idx(ws, i)?;
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
            _ => {}
        }
    }
}
