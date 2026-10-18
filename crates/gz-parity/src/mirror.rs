//! A caching HTTP mirror for web assets (Fuel meshes and textures), so a
//! replayed world loads them from a local cache instead of the live site.
//!
//! A client asks for `http://127.0.0.1:<port>/<host>/<path>` instead of
//! `https://<host>/<path>`. The mirror serves the file from `cache`, fetching
//! it from the upstream on a miss. With a lock file, every body is checked
//! against its recorded SHA-256 and new URLs are added to the lock.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub struct MirrorOptions {
    /// Where fetched files are kept, named by the SHA-256 of their URL.
    pub cache: PathBuf,
    /// JSON map of URL → SHA-256 of the body.
    pub lock: Option<PathBuf>,
    /// Fetch `<upstream>/<host>/<path>` instead of `https://<host>/<path>`
    /// (for tests).
    pub upstream: Option<String>,
    /// Never fetch; a cache miss is a 404.
    pub offline: bool,
    /// Longest an upstream fetch may take, body included
    /// ([`FETCH_TIMEOUT`] for the CLI).
    pub fetch_timeout: Duration,
}

type Lock = BTreeMap<String, String>;

/// For connecting to the upstream and for its response headers, so an
/// upstream that never answers shows up as a 404 well within a snapshot's
/// timeout.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The CLI's [`MirrorOptions::fetch_timeout`]: generous, for large meshes on
/// a slow link, but a body that stalls still fails.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Fuel, and its old domain, which serves the same files. The old domain is
/// tried only for files the lock pins, so what it returns is still checked.
const FUEL: &str = "fuel.gazebosim.org/";
const FUEL_OLD: &str = "fuel.ignitionrobotics.org/";

struct State {
    opts: MirrorOptions,
    lock: Mutex<Lock>,
}

/// Serves forever on `listener`, one thread per connection.
pub fn serve_on(listener: TcpListener, opts: MirrorOptions) -> Result<(), String> {
    std::fs::create_dir_all(&opts.cache).map_err(|e| format!("{}: {e}", opts.cache.display()))?;
    let lock = match &opts.lock {
        Some(path) if path.exists() => read_lock(path)?,
        _ => Lock::new(),
    };
    let port = listener.local_addr().map_err(crate::message)?.port();
    eprintln!("mirror: serving http://127.0.0.1:{port}/<host>/<path>");
    let state = Arc::new(State {
        opts,
        lock: Mutex::new(lock),
    });
    for stream in listener.incoming().flatten() {
        let state = Arc::clone(&state);
        std::thread::spawn(move || handle(stream, &state));
    }
    Ok(())
}

fn read_lock(path: &Path) -> Result<Lock, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn handle(stream: TcpStream, state: &State) {
    let mut reader = BufReader::new(&stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // Skip the headers.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let response = if method == "GET" {
        get(state, target)
    } else {
        Err((405, format!("{method} not supported")))
    };
    let mut out = &stream;
    let _ = match response {
        Ok(body) => respond(&mut out, 200, content_type(target), &body),
        Err((status, msg)) => {
            eprintln!("mirror: {status} {target}: {msg}");
            respond(&mut out, status, "text/plain", msg.as_bytes())
        }
    };
}

/// `/<host>/<path>` → the body, or an HTTP status and message.
fn get(state: &State, target: &str) -> Result<Vec<u8>, (u16, String)> {
    let rest = target.trim_start_matches('/');
    if !rest.contains('/') {
        return Err((400, "expected /<host>/<path>".into()));
    }
    // One key per file however the client encoded it.
    let url = format!("https://{}", percent_decode(rest));
    let file = state.opts.cache.join(hex(&Sha256::digest(url.as_bytes())));
    let body = match std::fs::read(&file) {
        Ok(body) => body,
        Err(_) if state.opts.offline => return Err((404, "not cached (offline)".into())),
        Err(_) => {
            let body = fetch(state, rest).or_else(|e| match old_fuel(state, rest, &url) {
                Some(alt) => {
                    fetch(state, &alt).map_err(|alt_e| (e.0, format!("{}; {}", e.1, alt_e.1)))
                }
                None => Err(e),
            })?;
            let tmp = file.with_extension("part");
            std::fs::write(&tmp, &body)
                .and_then(|()| std::fs::rename(&tmp, &file))
                .map_err(|e| (500, format!("cache write: {e}")))?;
            body
        }
    };
    check_lock(state, &url, &body)?;
    Ok(body)
}

/// `rest` on Fuel's old domain, when it is a Fuel file the lock pins.
fn old_fuel(state: &State, rest: &str, url: &str) -> Option<String> {
    let path = rest.strip_prefix(FUEL)?;
    let lock = state.lock.lock().unwrap_or_else(PoisonError::into_inner);
    lock.contains_key(url).then(|| format!("{FUEL_OLD}{path}"))
}

fn fetch(state: &State, rest: &str) -> Result<Vec<u8>, (u16, String)> {
    let url = match &state.opts.upstream {
        Some(base) => format!("{}/{rest}", base.trim_end_matches('/')),
        None => format!("https://{rest}"),
    };
    let not_found = |e: ureq::Error| (404, format!("{url}: {e}"));
    // An upstream that stalls, before the headers or in the body, fails the
    // request instead of hanging the client.
    ureq::get(&url)
        .config()
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .timeout_global(Some(state.opts.fetch_timeout))
        .build()
        .call()
        .map_err(not_found)?
        .into_body()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(not_found)
}

fn check_lock(state: &State, url: &str, body: &[u8]) -> Result<(), (u16, String)> {
    let Some(path) = &state.opts.lock else {
        return Ok(());
    };
    let sum = hex(&Sha256::digest(body));
    let mut lock = state.lock.lock().unwrap_or_else(PoisonError::into_inner);
    match lock.get(url) {
        Some(want) if *want == sum => Ok(()),
        Some(want) => Err((502, format!("sha256 {sum}, lock has {want}"))),
        None => {
            lock.insert(url.to_string(), sum);
            let json = serde_json::to_string_pretty(&*lock).map_err(|e| (500, e.to_string()))?;
            std::fs::write(path, json + "\n").map_err(|e| (500, format!("{}: {e}", path.display())))
        }
    }
}

fn respond(out: &mut impl Write, status: u16, ty: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        502 => "Bad Gateway",
        _ => "Error",
    };
    write!(
        out,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    out.write_all(body)
}

fn content_type(target: &str) -> &'static str {
    match gz_core::uri::extension(target).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "dae" => "model/vnd.collada+xml",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex_pair = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (b[i], hex_pair.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("/h/a.PNG"), "image/png");
        assert_eq!(content_type("/h/a.jpeg"), "image/jpeg");
        assert_eq!(content_type("/h/a.dae"), "model/vnd.collada+xml");
        assert_eq!(content_type("/h/a.gltf"), "model/gltf+json");
        assert_eq!(content_type("/h/a.glb"), "model/gltf-binary");
        assert_eq!(content_type("/h/a.stl"), "application/octet-stream");
    }

    #[test]
    fn status_lines() {
        for (status, reason) in [
            (200, "OK"),
            (400, "Bad Request"),
            (404, "Not Found"),
            (405, "Method Not Allowed"),
            (502, "Bad Gateway"),
            (500, "Error"),
        ] {
            let mut out = Vec::new();
            respond(&mut out, status, "text/plain", b"x").unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.starts_with(&format!("HTTP/1.1 {status} {reason}\r\n")));
            assert!(text.ends_with("\r\n\r\nx"));
        }
    }
}
