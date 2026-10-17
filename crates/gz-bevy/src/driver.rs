//! Platform websocket drivers. They only move bytes; the protocol lives in
//! [`gz_core::transport::Transport`].

use crossbeam_channel::Receiver;
#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Sender;

/// What a driver reports to the connection system.
#[derive(Debug)]
pub enum DriverEvent {
    Open,
    Message(Vec<u8>),
    Closed(Option<String>),
}

/// Handle to a running websocket connection.
pub struct Driver {
    pub events: Receiver<DriverEvent>,
    #[cfg(not(target_arch = "wasm32"))]
    outgoing: Sender<String>,
    #[cfg(target_arch = "wasm32")]
    socket: web_sys::WebSocket,
}

impl Driver {
    pub fn send(&self, frame: String) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.outgoing.send(frame);
        #[cfg(target_arch = "wasm32")]
        if let Err(e) = self.socket.send_with_str(&frame) {
            bevy::log::error!("websocket send failed: {e:?}");
        }
    }
}

/// A driver with no socket: tests push [`DriverEvent`]s in and read the
/// frames the connection sent.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fake() -> (Driver, Sender<DriverEvent>, Receiver<String>) {
    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let (out_tx, out_rx) = crossbeam_channel::unbounded();
    (
        Driver {
            events: event_rx,
            outgoing: out_tx,
        },
        event_tx,
        out_rx,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn connect(url: &str) -> Driver {
    use std::time::Duration;
    use tungstenite::{stream::MaybeTlsStream, Message};

    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let (out_tx, out_rx) = crossbeam_channel::unbounded::<String>();
    let url = url.to_string();

    std::thread::Builder::new()
        .name("gz-websocket".into())
        .spawn(move || {
            let mut socket = match tungstenite::connect(url.as_str()) {
                Ok((socket, _)) => socket,
                Err(e) => {
                    let _ = event_tx.send(DriverEvent::Closed(Some(e.to_string())));
                    return;
                }
            };
            // A short read timeout lets one thread service both directions.
            if let MaybeTlsStream::Plain(tcp) = socket.get_ref() {
                let _ = tcp.set_read_timeout(Some(Duration::from_millis(5)));
                let _ = tcp.set_nodelay(true);
            }
            let _ = event_tx.send(DriverEvent::Open);

            loop {
                while let Ok(frame) = out_rx.try_recv() {
                    if let Err(e) = socket.send(Message::Text(frame.into())) {
                        let _ = event_tx.send(DriverEvent::Closed(Some(e.to_string())));
                        return;
                    }
                }
                match socket.read() {
                    Ok(Message::Text(t)) => {
                        let _ = event_tx.send(DriverEvent::Message(t.as_bytes().to_vec()));
                    }
                    Ok(Message::Binary(b)) => {
                        let _ = event_tx.send(DriverEvent::Message(b.to_vec()));
                    }
                    Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => {
                        let _ = event_tx.send(DriverEvent::Closed(None));
                        return;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e) => {
                        let _ = event_tx.send(DriverEvent::Closed(Some(e.to_string())));
                        return;
                    }
                }
            }
        })
        .expect("failed to spawn websocket thread");

    Driver {
        events: event_rx,
        outgoing: out_tx,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn connect(url: &str) -> Driver {
    use wasm_bindgen::{closure::Closure, JsCast};
    use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let socket = match WebSocket::new(url) {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.send(DriverEvent::Closed(Some(format!("{e:?}"))));
            // A socket that never opens keeps the Driver shape uniform.
            let dummy = WebSocket::new("ws://0.0.0.0:0").expect("websocket constructor");
            return Driver {
                events: event_rx,
                socket: dummy,
            };
        }
    };
    socket.set_binary_type(BinaryType::Arraybuffer);

    let tx = event_tx.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        let _ = tx.send(DriverEvent::Open);
    });
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let tx = event_tx.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let data = e.data();
        if let Some(text) = data.as_string() {
            let _ = tx.send(DriverEvent::Message(text.into_bytes()));
        } else if let Ok(buf) = data.dyn_into::<js_sys::ArrayBuffer>() {
            let _ = tx.send(DriverEvent::Message(js_sys::Uint8Array::new(&buf).to_vec()));
        }
    });
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let tx = event_tx.clone();
    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |_e: ErrorEvent| {
        let _ = tx.send(DriverEvent::Closed(Some("websocket error".into())));
    });
    socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();

    let tx = event_tx;
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_e: CloseEvent| {
        let _ = tx.send(DriverEvent::Closed(None));
    });
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    Driver {
        events: event_rx,
        socket,
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Duration;
    use tungstenite::Message;

    fn next(d: &Driver) -> DriverEvent {
        d.events
            .recv_timeout(Duration::from_secs(10))
            .expect("driver event")
    }

    /// Runs `server` on one accepted websocket; returns the client URL.
    fn serve(
        server: impl FnOnce(tungstenite::WebSocket<std::net::TcpStream>) + Send + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server(tungstenite::accept(stream).unwrap());
        });
        url
    }

    #[test]
    fn relays_frames_both_ways_and_reports_a_clean_close() {
        let url = serve(|mut ws| {
            assert_eq!(ws.read().unwrap(), Message::Text("protos,,,".into()));
            ws.send(Message::Ping(vec![1].into())).unwrap();
            ws.send(Message::Text("hello".into())).unwrap();
            ws.send(Message::Binary(vec![1, 2, 3].into())).unwrap();
            ws.close(None).unwrap();
            // Drain until the client acknowledges the close.
            while ws.read().is_ok() {}
        });
        let d = connect(&url);
        assert!(matches!(next(&d), DriverEvent::Open));
        d.send("protos,,,".into());
        assert!(matches!(next(&d), DriverEvent::Message(m) if m == b"hello"));
        assert!(matches!(next(&d), DriverEvent::Message(m) if m == [1, 2, 3]));
        assert!(matches!(next(&d), DriverEvent::Closed(None)));
    }

    #[test]
    fn reports_a_dropped_connection_as_an_error() {
        let url = serve(drop);
        let d = connect(&url);
        assert!(matches!(next(&d), DriverEvent::Open));
        assert!(matches!(next(&d), DriverEvent::Closed(Some(_))));
    }

    #[test]
    fn reports_a_failed_connect() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let d = connect(&format!("ws://127.0.0.1:{port}"));
        assert!(matches!(next(&d), DriverEvent::Closed(Some(_))));
    }

    #[test]
    fn reports_a_failed_send() {
        // The server reads one frame, then closes with the rest unread, so
        // the kernel resets the connection while the client is still
        // writing (far more than the socket buffers hold).
        let url = serve(|mut ws| {
            let _ = ws.read();
        });
        let d = connect(&url);
        assert!(matches!(next(&d), DriverEvent::Open));
        for _ in 0..200 {
            d.send("x".repeat(1 << 16));
        }
        assert!(matches!(next(&d), DriverEvent::Closed(Some(_))));
    }
}
