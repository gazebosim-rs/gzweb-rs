//! Platform websocket drivers. They only move bytes; the protocol lives in
//! [`gz_core::transport::Transport`].

use crossbeam_channel::Receiver;
#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Sender;

/// What a driver reports to the connection system. A driver reports exactly
/// one `Closed` or `Rejected`, last.
#[derive(Debug)]
pub enum DriverEvent {
    Open,
    Message(Vec<u8>),
    Closed(Option<String>),
    /// The URL can't work; reconnecting won't help.
    Rejected(String),
}

/// Handle to a running websocket connection. Dropping it closes the socket.
pub struct Driver {
    pub events: Receiver<DriverEvent>,
    #[cfg(not(target_arch = "wasm32"))]
    outgoing: Sender<Vec<u8>>,
    #[cfg(target_arch = "wasm32")]
    socket: Option<web::Socket>,
}

impl Driver {
    pub(crate) fn bounded_events(&self) -> Vec<DriverEvent> {
        let mut events = Vec::new();
        for _ in 0..64 {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    events.push(DriverEvent::Closed(Some("bounded transport ended".into())));
                    break;
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if events.len() < 64
            && self.socket.as_ref().is_some_and(web::Socket::closed)
            && !events
                .iter()
                .any(|e| matches!(e, DriverEvent::Closed(_) | DriverEvent::Rejected(_)))
        {
            events.push(DriverEvent::Closed(Some(
                "bounded browser socket closed".into(),
            )));
        }
        events
    }

    /// Nonblocking bounded-backend send; failures are never accepted as controls.
    pub(crate) fn try_send(&self, frame: Vec<u8>) -> Result<(), ()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.outgoing.try_send(frame).map_err(|_| ())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.socket.as_ref().ok_or(())?.try_send(&frame)
        }
    }

    /// Sends a frame: as text when it is UTF-8 (like gzweb), else binary.
    pub fn send(&self, frame: Vec<u8>) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.outgoing.send(frame);
        #[cfg(target_arch = "wasm32")]
        if let Some(socket) = &self.socket {
            socket.send(&frame);
        }
    }
}

/// A frame as a websocket message: text when it is UTF-8, else binary.
#[cfg(not(target_arch = "wasm32"))]
fn message(frame: Vec<u8>) -> tungstenite::Message {
    match String::from_utf8(frame) {
        Ok(text) => tungstenite::Message::Text(text.into()),
        Err(e) => tungstenite::Message::Binary(e.into_bytes().into()),
    }
}

/// A driver with no socket: tests push [`DriverEvent`]s in and read the
/// frames the connection sent.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fake() -> (Driver, Sender<DriverEvent>, Receiver<Vec<u8>>) {
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

/// Connects on a background thread. `tls` is used for `wss://` URLs.
#[cfg(not(target_arch = "wasm32"))]
pub fn connect(url: &str, tls: Option<gz_core::net::TlsConfig>) -> Driver {
    connect_with_limit(url, tls, None)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn connect_bounded(url: &str, tls: Option<gz_core::net::TlsConfig>) -> Driver {
    connect_with_limit(url, tls, Some(64 * 1024))
}

#[cfg(not(target_arch = "wasm32"))]
fn connect_with_limit(
    url: &str,
    tls: Option<gz_core::net::TlsConfig>,
    limit: Option<usize>,
) -> Driver {
    use std::time::Duration;
    use tungstenite::Message;

    let (event_tx, event_rx) = if limit.is_some() {
        crossbeam_channel::bounded(64)
    } else {
        crossbeam_channel::unbounded()
    };
    let (out_tx, out_rx) = if limit.is_some() {
        crossbeam_channel::bounded::<Vec<u8>>(8)
    } else {
        crossbeam_channel::unbounded::<Vec<u8>>()
    };
    let url = url.to_string();

    std::thread::Builder::new()
        .name("gz-websocket".into())
        .spawn(move || {
            use crossbeam_channel::TryRecvError;
            use gz_core::net::ConnectError;

            // A short read timeout lets one thread service both directions.
            let timeout = Some(Duration::from_millis(5));
            let mut socket = match gz_core::net::connect(&url, tls, timeout) {
                Ok(socket) => socket,
                Err(ConnectError::InvalidUrl(e)) => {
                    let _ = event_tx.try_send(DriverEvent::Rejected(format!("invalid URL: {e}")));
                    return;
                }
                Err(ConnectError::Failed(e)) => {
                    let _ = event_tx.try_send(DriverEvent::Closed(Some(e)));
                    return;
                }
            };
            if let Some(limit) = limit {
                socket.set_config(|config| {
                    config.max_message_size = Some(limit);
                    config.max_frame_size = Some(limit);
                });
            }
            let _ = event_tx.try_send(DriverEvent::Open);

            loop {
                loop {
                    match out_rx.try_recv() {
                        Ok(frame) => {
                            if let Err(e) = socket.send(message(frame)) {
                                let _ = event_tx.try_send(DriverEvent::Closed(Some(e.to_string())));
                                return;
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        // The Driver was dropped: close and stop.
                        Err(TryRecvError::Disconnected) => {
                            let _ = socket.close(None);
                            let _ = socket.flush();
                            return;
                        }
                    }
                }
                match socket.read() {
                    Ok(Message::Text(t)) => {
                        if limit.is_some_and(|n| t.len() > n)
                            || event_tx
                                .try_send(DriverEvent::Message(t.as_bytes().to_vec()))
                                .is_err()
                        {
                            return;
                        }
                    }
                    Ok(Message::Binary(b)) => {
                        if limit.is_some_and(|n| b.len() > n)
                            || event_tx.try_send(DriverEvent::Message(b.to_vec())).is_err()
                        {
                            return;
                        }
                    }
                    Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => {
                        let _ = event_tx.try_send(DriverEvent::Closed(None));
                        return;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e) => {
                        let _ = event_tx.try_send(DriverEvent::Closed(Some(e.to_string())));
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
    connect_web(url, false)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn connect_bounded(url: &str) -> Driver {
    connect_web(url, true)
}

#[cfg(target_arch = "wasm32")]
fn connect_web(url: &str, bounded: bool) -> Driver {
    let (event_tx, event_rx) = if bounded {
        crossbeam_channel::bounded(64)
    } else {
        crossbeam_channel::unbounded()
    };
    let socket = match web::Socket::open(url, event_tx.clone(), bounded) {
        Ok(socket) => Some(socket),
        Err(e) => {
            let _ = event_tx.try_send(DriverEvent::Rejected(e));
            None
        }
    };
    Driver {
        events: event_rx,
        socket,
    }
}

/// The browser `WebSocket` and the callbacks it holds; dropped with the
/// [`Driver`].
#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crossbeam_channel::Sender;
    use wasm_bindgen::{closure::Closure, JsCast};
    use web_sys::{BinaryType, CloseEvent, Event, MessageEvent, WebSocket};

    use super::DriverEvent;

    pub struct Socket {
        ws: WebSocket,
        _on_open: Closure<dyn FnMut()>,
        _on_message: Closure<dyn FnMut(MessageEvent)>,
        _on_error: Closure<dyn FnMut(Event)>,
        _on_close: Closure<dyn FnMut(CloseEvent)>,
    }

    impl Socket {
        pub fn open(url: &str, tx: Sender<DriverEvent>, bounded: bool) -> Result<Self, String> {
            let ws = WebSocket::new(url).map_err(|e| format!("invalid URL: {e:?}"))?;
            ws.set_binary_type(BinaryType::Arraybuffer);

            let open_tx = tx.clone();
            let on_open = Closure::<dyn FnMut()>::new(move || {
                let _ = open_tx.try_send(DriverEvent::Open);
            });
            let message_tx = tx.clone();
            let message_ws = ws.clone();
            let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                if bounded
                    && web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.document_element())
                        .and_then(|e| e.get_attribute("data-viewer-state"))
                        .as_deref()
                        == Some("failed")
                {
                    let _ = message_ws.close();
                    return;
                }
                let data = e.data();
                let bytes = if let Some(text) = data.as_string() {
                    text.into_bytes()
                } else if let Ok(buf) = data.dyn_into::<js_sys::ArrayBuffer>() {
                    if bounded && buf.byte_length() > 64 * 1024 {
                        let _ = message_ws.close_with_code(4002);
                        return;
                    }
                    js_sys::Uint8Array::new(&buf).to_vec()
                } else {
                    if bounded {
                        let _ = message_ws.close_with_code(4002);
                    }
                    return;
                };
                if (bounded && bytes.len() > 64 * 1024)
                    || message_tx.try_send(DriverEvent::Message(bytes)).is_err()
                {
                    let _ = message_ws.close_with_code(4002);
                }
            });
            // An error is always followed by a close: report both as one.
            let failed = Rc::new(RefCell::new(None::<String>));
            let error_flag = failed.clone();
            let on_error = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                *error_flag.borrow_mut() = Some("websocket error".into());
            });
            let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_e: CloseEvent| {
                let _ = tx.try_send(DriverEvent::Closed(failed.borrow_mut().take()));
            });
            ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
            Ok(Self {
                ws,
                _on_open: on_open,
                _on_message: on_message,
                _on_error: on_error,
                _on_close: on_close,
            })
        }

        pub fn closed(&self) -> bool {
            self.ws.ready_state() >= WebSocket::CLOSING
        }
        pub fn try_send(&self, frame: &[u8]) -> Result<(), ()> {
            if self.ws.ready_state() != WebSocket::OPEN || self.ws.buffered_amount() > 4096 {
                return Err(());
            }
            let text = std::str::from_utf8(frame).map_err(|_| ())?;
            self.ws.send_with_str(text).map_err(|_| ())
        }

        pub fn send(&self, frame: &[u8]) {
            let sent = match std::str::from_utf8(frame) {
                Ok(text) => self.ws.send_with_str(text),
                Err(_) => self.ws.send_with_u8_array(frame),
            };
            if let Err(e) = sent {
                bevy::log::error!("websocket send failed: {e:?}");
            }
        }
    }

    impl Drop for Socket {
        fn drop(&mut self) {
            // Detach the callbacks before they are freed.
            self.ws.set_onopen(None);
            self.ws.set_onmessage(None);
            self.ws.set_onerror(None);
            self.ws.set_onclose(None);
            let _ = self.ws.close();
        }
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
            assert_eq!(
                ws.read().unwrap(),
                Message::Binary(b"req,/s,T,\xff".to_vec().into())
            );
            ws.send(Message::Ping(vec![1].into())).unwrap();
            ws.send(Message::Text("hello".into())).unwrap();
            ws.send(Message::Binary(vec![1, 2, 3].into())).unwrap();
            ws.close(None).unwrap();
            // Drain until the client acknowledges the close.
            while ws.read().is_ok() {}
        });
        let d = connect(&url, None);
        assert!(matches!(next(&d), DriverEvent::Open));
        d.send(b"protos,,,".to_vec());
        d.send(b"req,/s,T,\xff".to_vec());
        assert!(matches!(next(&d), DriverEvent::Message(m) if m == b"hello"));
        assert!(matches!(next(&d), DriverEvent::Message(m) if m == [1, 2, 3]));
        assert!(matches!(next(&d), DriverEvent::Closed(None)));
    }

    #[test]
    fn reports_a_dropped_connection_as_an_error() {
        let url = serve(drop);
        let d = connect(&url, None);
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
        let d = connect(&format!("ws://127.0.0.1:{port}"), None);
        assert!(matches!(next(&d), DriverEvent::Closed(Some(_))));
    }

    #[test]
    fn dropping_the_driver_closes_the_socket() {
        let (closed_tx, closed_rx) = crossbeam_channel::bounded(1);
        let url = serve(move |mut ws| {
            let closed = loop {
                match ws.read() {
                    Ok(Message::Close(_)) | Err(_) => break true,
                    Ok(_) => {}
                }
            };
            let _ = closed_tx.send(closed);
        });
        let d = connect(&url, None);
        assert!(matches!(next(&d), DriverEvent::Open));
        drop(d);
        assert_eq!(closed_rx.recv_timeout(Duration::from_secs(10)), Ok(true));
    }

    #[test]
    fn rejects_an_invalid_url() {
        let d = connect("http://example.com", None);
        assert!(matches!(next(&d), DriverEvent::Rejected(e) if e.starts_with("invalid URL")));
    }

    #[test]
    fn reports_a_failed_send() {
        // The server reads one frame, then closes with the rest unread, so
        // the kernel resets the connection while the client is still
        // writing (far more than the socket buffers hold).
        let url = serve(|mut ws| {
            let _ = ws.read();
        });
        let d = connect(&url, None);
        assert!(matches!(next(&d), DriverEvent::Open));
        for _ in 0..200 {
            d.send(vec![b'x'; 1 << 16]);
        }
        assert!(matches!(next(&d), DriverEvent::Closed(Some(_))));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod bounded_poll_tests {
    use super::*;
    #[test]
    fn event_budget_is_per_poll_even_if_the_producer_continues() {
        let (driver, tx, _out) = fake();
        for _ in 0..130 {
            assert!(tx.send(DriverEvent::Message(vec![1])).is_ok());
        }
        assert_eq!(driver.bounded_events().len(), 64);
        assert_eq!(driver.bounded_events().len(), 64);
        assert_eq!(driver.bounded_events().len(), 2);
    }
}
