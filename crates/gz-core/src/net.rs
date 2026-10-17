//! Blocking websocket connect for native clients (feature `native`), with
//! `wss://` over rustls.

use std::fmt::Display;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls_pki_types::pem::PemObject;
use rustls_pki_types::CertificateDer;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, WebSocket};

pub type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Shared TLS client settings.
pub type TlsConfig = Arc<rustls::ClientConfig>;

fn msg(e: impl Display) -> String {
    e.to_string()
}

/// How long connecting and the websocket/TLS handshake may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Why [`connect`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The URL can't work; retrying won't help.
    InvalidUrl(String),
    /// Anything else (refused, timed out, TLS, handshake).
    Failed(String),
}

impl std::error::Error for ConnectError {}

impl Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::InvalidUrl(e) => write!(f, "invalid URL: {e}"),
            ConnectError::Failed(e) => f.write_str(e),
        }
    }
}

/// TLS settings trusting Mozilla's roots plus, optionally, the PEM
/// certificates in `extra_ca` (for a server with a private or self-signed
/// certificate).
pub fn tls_config(extra_ca: Option<&Path>) -> Result<TlsConfig, String> {
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(path) = extra_ca {
        let at = |e: String| format!("{}: {e}", path.display());
        let certs: Vec<_> = CertificateDer::pem_file_iter(path)
            .map_err(msg)
            .map_err(at)?
            .collect::<Result<_, _>>()
            .map_err(msg)
            .map_err(at)?;
        if certs.is_empty() {
            return Err(at("no certificates".into()));
        }
        for cert in certs {
            roots.add(cert).map_err(msg).map_err(at)?;
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(msg)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Connects to a `ws://` or `wss://` URL. `tls` defaults to
/// [`tls_config`]`(None)`. Connecting and the handshakes time out after
/// [`CONNECT_TIMEOUT`]; `read_timeout` applies afterwards, so a single
/// thread can poll the socket in both directions.
pub fn connect(
    url: &str,
    tls: Option<TlsConfig>,
    read_timeout: Option<Duration>,
) -> Result<Socket, ConnectError> {
    connect_within(url, tls, read_timeout, CONNECT_TIMEOUT)
}

/// [`connect`] with a custom connect/handshake timeout.
pub fn connect_within(
    url: &str,
    tls: Option<TlsConfig>,
    read_timeout: Option<Duration>,
    timeout: Duration,
) -> Result<Socket, ConnectError> {
    let invalid = |e: String| ConnectError::InvalidUrl(e);
    let failed = |e: String| ConnectError::Failed(e);
    let request = url.into_client_request().map_err(msg).map_err(invalid)?;
    let uri = request.uri();
    let secure = match uri.scheme_str() {
        Some("ws") => false,
        Some("wss") => true,
        other => return Err(invalid(format!("unsupported scheme {other:?}"))),
    };
    // `into_client_request` already refused a URL without a host.
    let host = uri
        .host()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });

    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(msg)
        .map_err(failed)?
        .collect();
    let mut last = format!("{host} has no addresses");
    let stream = addrs
        .iter()
        .find_map(|a| match TcpStream::connect_timeout(a, timeout) {
            Ok(s) => Some(s),
            Err(e) => {
                last = format!("{a}: {e}");
                None
            }
        })
        .ok_or_else(|| failed(last.clone()))?;
    stream.set_nodelay(true).map_err(msg).map_err(failed)?;
    // A silent peer must not block the handshake forever.
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(msg)
        .map_err(failed)?;
    // Same socket, kept to set the timeouts once the handshake is done.
    let raw = stream.try_clone().map_err(msg).map_err(failed)?;
    let connector = match (secure, tls) {
        (false, _) => Connector::Plain,
        (true, Some(tls)) => Connector::Rustls(tls),
        (true, None) => Connector::Rustls(tls_config(None).map_err(failed)?),
    };
    let (socket, _) = tungstenite::client_tls_with_config(request, stream, None, Some(connector))
        .map_err(msg)
        .map_err(failed)?;
    raw.set_read_timeout(read_timeout)
        .and_then(|()| raw.set_write_timeout(None))
        .map_err(msg)
        .map_err(failed)?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use tungstenite::Message;

    struct Pki {
        ca_pem: String,
        server: Arc<rustls::ServerConfig>,
    }

    /// A CA and a `localhost` certificate it signed.
    fn pki() -> Pki {
        use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["localhost".to_string()])
            .unwrap()
            .signed_by(&leaf_key, &ca, &ca_key)
            .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                rustls_pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
            )
            .unwrap();
        Pki {
            ca_pem: ca.pem(),
            server: Arc::new(server),
        }
    }

    /// Accepts one websocket (over TLS when `tls` is set) and echoes one
    /// text frame. Returns the port.
    fn echo_server(tls: Option<Arc<rustls::ServerConfig>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            match tls {
                Some(cfg) => {
                    let conn = rustls::ServerConnection::new(cfg).unwrap();
                    if let Ok(ws) = tungstenite::accept(rustls::StreamOwned::new(conn, tcp)) {
                        echo(ws);
                    }
                }
                None => echo(tungstenite::accept(tcp).unwrap()),
            }
        });
        port
    }

    fn echo<S: std::io::Read + Write>(mut ws: WebSocket<S>) {
        if let Ok(m) = ws.read() {
            let _ = ws.send(m);
        }
        let _ = ws.close(None);
        while ws.read().is_ok() {}
    }

    fn round_trip(mut ws: Socket) {
        ws.send(Message::Text("hi".into())).unwrap();
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    assert_eq!(t.as_str(), "hi");
                    return;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// A file removed when dropped.
    struct Temp(std::path::PathBuf);

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn temp(name: &str, text: &str) -> Temp {
        let path = std::env::temp_dir().join(format!("gz-core-net-{}-{name}", std::process::id()));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        Temp(path)
    }

    #[test]
    fn plain_websocket() {
        let port = echo_server(None);
        let ws = connect(
            &format!("ws://127.0.0.1:{port}"),
            None,
            Some(Duration::from_millis(5)),
        )
        .unwrap();
        round_trip(ws);
    }

    #[test]
    fn wss_with_a_private_ca() {
        let pki = pki();
        let ca = temp("ca.pem", &pki.ca_pem);
        let tls = tls_config(Some(&ca.0)).unwrap();
        let port = echo_server(Some(pki.server.clone()));
        round_trip(connect(&format!("wss://localhost:{port}"), Some(tls), None).unwrap());

        // Without the CA the certificate is rejected.
        let port = echo_server(Some(pki.server));
        let err = connect(&format!("wss://localhost:{port}"), None, None).unwrap_err();
        assert!(err.to_string().contains("UnknownIssuer"), "{err}");
    }

    #[test]
    fn bad_ca_files() {
        let missing = std::env::temp_dir().join("gz-core-net-missing.pem");
        assert!(tls_config(Some(&missing))
            .unwrap_err()
            .contains("missing.pem"));
        let empty = temp("empty.pem", "no certificates here\n");
        assert!(tls_config(Some(&empty.0))
            .unwrap_err()
            .ends_with("no certificates"));
        let broken = temp(
            "broken.pem",
            "-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n",
        );
        assert!(tls_config(Some(&broken.0))
            .unwrap_err()
            .contains("broken.pem"));
        let not_a_cert = temp(
            "garbage.pem",
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
        );
        assert!(tls_config(Some(&not_a_cert.0))
            .unwrap_err()
            .contains("garbage.pem"));
    }

    #[test]
    fn connect_errors() {
        let invalid =
            |url: &str| matches!(connect(url, None, None), Err(ConnectError::InvalidUrl(_)));
        assert!(invalid("not a url"));
        assert!(invalid("ws:///path"));
        assert!(invalid("http://127.0.0.1:1"));
        let failed = |url: &str| matches!(connect(url, None, None), Err(ConnectError::Failed(_)));
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(failed(&format!("ws://127.0.0.1:{port}")));
        assert!(failed("ws://no-such-host.invalid:1"));
        // A server that isn't speaking websocket.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || drop(listener.accept()));
        assert!(failed(&format!("ws://127.0.0.1:{port}")));
        assert_eq!(
            ConnectError::InvalidUrl("x".into()).to_string(),
            "invalid URL: x"
        );
    }

    #[test]
    fn a_silent_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accepts and never answers the handshake.
        std::thread::spawn(move || {
            let held = listener.accept();
            std::thread::sleep(Duration::from_secs(5));
            drop(held);
        });
        let started = std::time::Instant::now();
        let err = connect_within(
            &format!("ws://127.0.0.1:{port}"),
            None,
            None,
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(matches!(err, ConnectError::Failed(_)), "{err}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
