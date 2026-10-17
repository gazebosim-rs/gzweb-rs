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

pub struct ReplayOptions {
    pub port: u16,
    pub client_log: Option<PathBuf>,
    pub stream_limit: Option<usize>,
    /// Exit after this many connections (0 = serve forever).
    pub connections: usize,
}

pub fn serve(corpus: &Corpus, opts: &ReplayOptions) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", opts.port)).map_err(|e| e.to_string())?;
    eprintln!("replay: serving '{}' on ws://127.0.0.1:{}", corpus.manifest.world, opts.port);
    if let Some(path) = &opts.client_log {
        std::fs::write(path, "").map_err(|e| e.to_string())?;
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
        if let Err(e) = handle(&mut ws, corpus, opts) {
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
) -> Result<(), String> {
    let mut log = match &opts.client_log {
        Some(p) => Some(
            std::fs::OpenOptions::new()
                .append(true)
                .open(p)
                .map_err(|e| e.to_string())?,
        ),
        None => None,
    };
    let m = &corpus.manifest;
    let send_idx = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>, idx: usize| -> Result<(), String> {
        let data = corpus.frame(idx).ok_or(format!("corpus has no frame {idx}"))?;
        ws.send(Message::Binary(data.to_vec().into())).map_err(|e| e.to_string())
    };

    loop {
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::Protocol(_)) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => {
                if let Some(f) = log.as_mut() {
                    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                    let _ = writeln!(f, "binary:{hex}");
                }
                continue;
            }
            Message::Close(_) => return Ok(()),
            _ => continue,
        };
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{text}");
        }

        let Some(fr) = Frame::parse(text.as_bytes()) else { continue };
        match fr.op {
            "auth" => ws.send(Message::Text("authorized".into())).map_err(|e| e.to_string())?,
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
                        let data = frame::build_binary("asset", &uri, "gz.msgs.StringMsg", &err.encode_to_vec());
                        ws.send(Message::Binary(data.into())).map_err(|e| e.to_string())?;
                    }
                }
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
