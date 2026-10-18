//! The caching HTTP mirror, against a fake upstream.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gz_parity::mirror::{self, MirrorOptions};

/// Serves `/<anything>` with the path as the body, or 404 when the path
/// contains "missing"; a path with "stall" sends half its body and hangs.
/// Returns the base URL and a hit counter.
fn upstream() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            if path.contains("stall") {
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhalf");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            let (status, body) = if path.contains("missing") {
                ("404 Not Found", String::new())
            } else {
                ("200 OK", path)
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, hits)
}

fn start(opts: MirrorOptions) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || mirror::serve_on(listener, opts));
    base
}

fn get(url: &str) -> (u16, String) {
    let resp = ureq::get(url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.into_body().read_to_string().unwrap())
}

fn opts(cache: &Path, lock: Option<&Path>, upstream: Option<&str>, offline: bool) -> MirrorOptions {
    MirrorOptions {
        cache: cache.to_path_buf(),
        lock: lock.map(Path::to_path_buf),
        upstream: upstream.map(str::to_string),
        offline,
        fetch_timeout: Duration::from_millis(500),
    }
}

#[test]
fn fetches_once_then_serves_from_the_cache() {
    let tmp = common::scratch("mirror-cache");
    let lock = tmp.join("http.lock.json");
    let (up, hits) = upstream();
    let base = start(opts(&tmp.join("cache"), Some(&lock), Some(&up), false));

    let (status, body) = get(&format!("{base}/fuel.example/a%20b/c.png"));
    assert_eq!((status, body.as_str()), (200, "/fuel.example/a%20b/c.png"));
    // Same file, encoded differently: one cache entry, no second fetch.
    let (status, again) = get(&format!("{base}/fuel.example/a%20b/%63.png"));
    assert_eq!((status, again.as_str()), (200, body.as_str()));
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let locked: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
    assert_eq!(
        locked.keys().collect::<Vec<_>>(),
        ["https://fuel.example/a b/c.png"]
    );

    // A fresh mirror on the same cache and lock is fully offline.
    let offline = start(opts(&tmp.join("cache"), Some(&lock), None, true));
    assert_eq!(get(&format!("{offline}/fuel.example/a%20b/c.png")).0, 200);
    assert_eq!(get(&format!("{offline}/fuel.example/other.png")).0, 404);
}

#[test]
fn a_changed_file_fails_the_lock() {
    let tmp = common::scratch("mirror-lock");
    let lock = tmp.join("http.lock.json");
    std::fs::write(&lock, r#"{"https://fuel.example/x.dae": "0000"}"#).unwrap();
    let (up, _) = upstream();
    let base = start(opts(&tmp.join("cache"), Some(&lock), Some(&up), false));
    let (status, body) = get(&format!("{base}/fuel.example/x.dae"));
    assert_eq!(status, 502);
    assert!(body.contains("lock has 0000"), "{body}");
}

#[test]
fn a_stalled_body_is_a_404_not_a_hang() {
    let tmp = common::scratch("mirror-stall");
    let (up, _) = upstream();
    let base = start(opts(&tmp.join("cache"), None, Some(&up), false));
    let started = std::time::Instant::now();
    let (status, body) = get(&format!("{base}/fuel.example/stall.dae"));
    assert_eq!(status, 404, "{body}");
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn errors_become_http_statuses() {
    let tmp = common::scratch("mirror-errors");
    let (up, _) = upstream();
    let base = start(opts(&tmp.join("cache"), None, Some(&up), false));
    assert_eq!(get(&format!("{base}/fuel.example/missing.png")).0, 404);
    assert_eq!(get(&format!("{base}/nohost")).0, 400);
    let resp = ureq::post(&format!("{base}/fuel.example/a.png"))
        .config()
        .http_status_as_error(false)
        .build()
        .send_empty()
        .unwrap();
    assert_eq!(resp.status().as_u16(), 405);
    // Without a lock, a fetched file is simply served.
    assert_eq!(get(&format!("{base}/fuel.example/ok.stl")).0, 200);
}

#[test]
fn bad_setup_is_an_error() {
    let tmp = common::scratch("mirror-setup");
    let listener = || TcpListener::bind("127.0.0.1:0").unwrap();

    let lock = tmp.join("bad.lock.json");
    std::fs::write(&lock, "not json").unwrap();
    let err = mirror::serve_on(listener(), opts(&tmp, Some(&lock), None, true)).unwrap_err();
    assert!(err.contains("bad.lock.json"), "{err}");

    let missing = tmp.join("missing.lock.json");
    std::fs::create_dir_all(&missing).unwrap();
    let err = mirror::serve_on(listener(), opts(&tmp, Some(&missing), None, true)).unwrap_err();
    assert!(err.contains("missing.lock.json"), "{err}");

    let file = tmp.join("a-file");
    std::fs::write(&file, "").unwrap();
    let err = mirror::serve_on(listener(), opts(&file, None, None, true)).unwrap_err();
    assert!(err.contains("a-file"), "{err}");
}
