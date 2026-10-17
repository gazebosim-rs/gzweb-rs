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
                    Ok(Message::Close(_)) => {
                        let _ = event_tx.send(DriverEvent::Closed(None));
                        return;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(tungstenite::Error::ConnectionClosed) => {
                        let _ = event_tx.send(DriverEvent::Closed(None));
                        return;
                    }
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
