//! Asset sources for Gazebo resources.
//!
//! gzweb loads a mesh/texture from its resolved URI over HTTP and, if that
//! fails, asks the Gazebo websocket server for it (`asset,,,<uri>`). So does
//! `gzhttps://`: a failed fetch falls back to the server, which resolves
//! Fuel URLs to its local Fuel cache. Local paths have no HTTP location
//! (gzweb fetches them relative to the page, which only finds files a site
//! happens to serve), so they go straight to the server:
//!
//! | load URI ([`gz_core::uri::mesh_load_uri`]) | Bevy asset path               |
//! |--------------------------------------------|-------------------------------|
//! | `https://host/a/b.dae`                     | `gzhttps://host/a/b.dae`      |
//! | `/abs/path/b.dae`                          | `wsasset://abs/abs/path/b.dae`|
//! | `rel/path/b.dae`                           | `wsasset://rel/rel/path/b.dae`|
//!
//! Keeping the real directory structure in the Bevy path means relative
//! references inside a mesh (glTF buffers, OBJ `.mtl`, textures) resolve to
//! the right server-side file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader,
};
use bevy::prelude::*;

pub const WS_SOURCE: &str = "wsasset";
pub const HTTP_SOURCE: &str = "gzhttps";

/// Bevy asset path for a resolved load URI (see module docs).
pub fn asset_path(load_uri: &str) -> String {
    if let Some(rest) = load_uri.strip_prefix("https://") {
        format!("{HTTP_SOURCE}://{rest}")
    } else if let Some(rest) = load_uri.strip_prefix("http://") {
        // Plain HTTP is only expected for local servers; keep the scheme visible.
        format!("{HTTP_SOURCE}://http:/{rest}")
    } else if let Some(rest) = load_uri.strip_prefix('/') {
        format!("{WS_SOURCE}://abs/{rest}")
    } else {
        format!("{WS_SOURCE}://rel/{load_uri}")
    }
}

/// Inverse of [`asset_path`] for the websocket source: the URI to request.
fn ws_request_uri(path: &Path) -> Option<String> {
    let p = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = p.strip_prefix("abs/") {
        Some(format!("/{rest}"))
    } else {
        p.strip_prefix("rel/").map(str::to_string)
    }
}

/// Inverse of [`asset_path`] for the HTTP source. With a `mirror`, HTTPS
/// URLs become `<mirror>/<host>/<path>` (see `gz-parity mirror`).
fn http_url(path: &Path, mirror: Option<&str>) -> String {
    let p = path
        .to_string_lossy()
        .replace('\\', "/")
        .replace(' ', "%20");
    match (p.strip_prefix("http:/"), mirror) {
        (Some(rest), _) => format!("http://{rest}"),
        (None, Some(m)) => format!("{}/{p}", m.trim_end_matches('/')),
        (None, None) => format!("https://{p}"),
    }
}

// ─── Websocket asset rendezvous ─────────────────────────────────────────────

pub type AssetResult = Result<Vec<u8>, String>;

#[derive(Default)]
struct StoreInner {
    /// Responses that arrived before any reader asked for them.
    ready: HashMap<String, AssetResult>,
    /// Readers waiting for a response, keyed by requested URI.
    waiters: HashMap<String, Vec<async_channel::Sender<AssetResult>>>,
    /// URIs to request from the server, drained by the connection system.
    to_request: Vec<String>,
}

/// Shared between the websocket asset reader (on IO task threads) and the connection
/// system. Readers register a waiter and sleep; the connection system sends
/// the request and later wakes the waiters with the reply.
#[derive(Resource, Clone, Default)]
pub struct WsAssetStore(Arc<Mutex<StoreInner>>);

impl WsAssetStore {
    /// A panicking reader can't leave the maps inconsistent (every update is
    /// a single insert/remove), so a poisoned lock is safe to reuse.
    fn lock(&self) -> std::sync::MutexGuard<'_, StoreInner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn deliver(&self, uri: &str, result: AssetResult) {
        let mut inner = self.lock();
        match inner.waiters.remove(uri) {
            Some(waiters) => {
                for tx in waiters {
                    let _ = tx.try_send(result.clone());
                }
            }
            None => {
                inner.ready.insert(uri.to_string(), result);
            }
        }
    }

    /// URIs that readers are waiting for and that have not been requested yet.
    pub fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().to_request)
    }

    /// Fail every waiting reader (e.g. the connection closed).
    pub fn fail_all(&self, reason: &str) {
        let mut inner = self.lock();
        for (_, waiters) in inner.waiters.drain() {
            for tx in waiters {
                let _ = tx.try_send(Err(reason.to_string()));
            }
        }
        inner.to_request.clear();
    }

    /// Number of readers currently waiting, over all URIs.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.lock().waiters.values().map(Vec::len).sum()
    }

    pub(crate) async fn fetch(&self, uri: &str) -> AssetResult {
        let rx = {
            let mut inner = self.lock();
            if let Some(result) = inner.ready.remove(uri) {
                return result;
            }
            let (tx, rx) = async_channel::bounded(1);
            let waiters = inner.waiters.entry(uri.to_string()).or_default();
            let first = waiters.is_empty();
            waiters.push(tx);
            if first {
                inner.to_request.push(uri.to_string());
            }
            rx
        };
        rx.recv()
            .await
            .unwrap_or_else(|_| Err("asset store closed".to_string()))
    }
}

struct WsAssetReader {
    store: WsAssetStore,
}

impl AssetReader for WsAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let not_found = || AssetReaderError::NotFound(path.to_path_buf());
        let uri = ws_request_uri(path).ok_or_else(not_found)?;
        match self.store.fetch(&uri).await {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(e) => {
                warn!("websocket asset '{uri}' failed: {e}");
                Err(not_found())
            }
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

// ─── HTTP(S) assets (Fuel and other web URLs) ───────────────────────────────

struct HttpAssetReader {
    mirror: Option<String>,
    /// Where a failed fetch is retried.
    store: WsAssetStore,
}

impl AssetReader for HttpAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let url = http_url(path, self.mirror.as_deref());
        let http_error = match fetch_bytes(&url).await {
            Ok(bytes) => return Ok(VecReader::new(bytes)),
            Err(e) => e,
        };
        // The server knows the real URL, not the mirror's.
        let uri = http_url(path, None);
        match self.store.fetch(&uri).await {
            Ok(bytes) => {
                info!("HTTP asset '{url}' failed ({http_error}); got it from the server");
                Ok(VecReader::new(bytes))
            }
            Err(e) => {
                warn!("asset '{uri}' failed: HTTP: {http_error}; server: {e}");
                Err(AssetReaderError::NotFound(path.to_path_buf()))
            }
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use std::time::Duration;
    // ureq blocks, but asset readers run on the IO task pool. A server that
    // never answers fails the load instead of leaving it pending forever;
    // the body itself may take as long as it needs.
    ureq::get(url)
        .config()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .call()
        .map_err(|e| e.to_string())?
        .into_body()
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or("no window")?;
    let resp = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a Response")?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Registers the `wsasset://` and `gzhttps://` sources. **Must be added
/// before `DefaultPlugins`**, because `AssetPlugin` builds the sources.
#[derive(Default)]
pub struct GzAssetSourcesPlugin {
    /// Fetch HTTPS assets through this `gz-parity mirror` base URL.
    pub http_mirror: Option<String>,
}

impl Plugin for GzAssetSourcesPlugin {
    fn build(&self, app: &mut App) {
        let store = WsAssetStore::default();
        let reader_store = store.clone();
        app.register_asset_source(
            WS_SOURCE,
            AssetSourceBuilder::new(move || {
                Box::new(WsAssetReader {
                    store: reader_store.clone(),
                })
            }),
        )
        .register_asset_source(
            HTTP_SOURCE,
            AssetSourceBuilder::new({
                let mirror = self.http_mirror.clone();
                let store = store.clone();
                move || {
                    Box::new(HttpAssetReader {
                        mirror: mirror.clone(),
                        store: store.clone(),
                    })
                }
            }),
        )
        .insert_resource(store);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::tasks::block_on;

    #[test]
    fn paths_round_trip() {
        assert_eq!(
            asset_path("/home/u/m/a.dae"),
            "wsasset://abs/home/u/m/a.dae"
        );
        assert_eq!(
            ws_request_uri(Path::new("abs/home/u/m/a.dae")).unwrap(),
            "/home/u/m/a.dae"
        );
        assert_eq!(
            asset_path("pkg/meshes/a.stl"),
            "wsasset://rel/pkg/meshes/a.stl"
        );
        assert_eq!(
            ws_request_uri(Path::new("rel/pkg/meshes/a.stl")).unwrap(),
            "pkg/meshes/a.stl"
        );
        assert_eq!(
            asset_path("https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"),
            "gzhttps://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"
        );
        assert_eq!(
            http_url(
                Path::new("fuel.gazebosim.org/1.0/o/models/m m/1/files/a.glb"),
                None
            ),
            "https://fuel.gazebosim.org/1.0/o/models/m%20m/1/files/a.glb"
        );
        assert_eq!(
            http_url(
                Path::new("fuel.gazebosim.org/1.0/a.glb"),
                Some("http://127.0.0.1:9101/")
            ),
            "http://127.0.0.1:9101/fuel.gazebosim.org/1.0/a.glb"
        );
    }

    #[test]
    fn plain_http_keeps_its_scheme() {
        assert_eq!(
            asset_path("http://127.0.0.1:8/a.obj"),
            "gzhttps://http:/127.0.0.1:8/a.obj"
        );
        assert_eq!(
            http_url(Path::new("http:/127.0.0.1:8/a.obj"), Some("http://m")),
            "http://127.0.0.1:8/a.obj"
        );
        assert_eq!(ws_request_uri(Path::new("elsewhere/a.obj")), None);
    }

    fn spawn_fetch(
        store: &WsAssetStore,
        uri: &'static str,
    ) -> std::thread::JoinHandle<AssetResult> {
        let store = store.clone();
        std::thread::spawn(move || block_on(store.fetch(uri)))
    }

    fn wait_for_waiters(store: &WsAssetStore, n: usize) {
        while store.waiting() < n {
            std::thread::yield_now();
        }
    }

    #[test]
    fn store_requests_once_and_wakes_all_waiters() {
        let store = WsAssetStore::default();
        let a = spawn_fetch(&store, "/x");
        let b = spawn_fetch(&store, "/x");
        wait_for_waiters(&store, 2);
        assert_eq!(store.take_requests(), ["/x"]);
        assert!(store.take_requests().is_empty());
        store.deliver("/x", Ok(vec![1, 2]));
        assert_eq!(a.join().unwrap(), Ok(vec![1, 2]));
        assert_eq!(b.join().unwrap(), Ok(vec![1, 2]));
    }

    #[test]
    fn a_reply_before_the_request_is_kept_for_the_reader() {
        let store = WsAssetStore::default();
        store.deliver("/early", Ok(vec![9]));
        assert_eq!(block_on(store.fetch("/early")), Ok(vec![9]));
        // Consumed: nothing left over, and nothing to request.
        assert_eq!(store.lock().ready.len(), 0);
        assert!(store.take_requests().is_empty());
    }

    #[test]
    fn fail_all_wakes_waiters_and_drops_pending_requests() {
        let store = WsAssetStore::default();
        let a = spawn_fetch(&store, "/a");
        wait_for_waiters(&store, 1);
        store.fail_all("gone");
        assert_eq!(a.join().unwrap(), Err("gone".into()));
        assert!(store.take_requests().is_empty());
    }

    #[test]
    fn a_dropped_waiter_channel_is_an_error() {
        let store = WsAssetStore::default();
        let a = spawn_fetch(&store, "/a");
        wait_for_waiters(&store, 1);
        // Dropping the senders without replying closes the channel.
        store.lock().waiters.clear();
        assert_eq!(a.join().unwrap(), Err("asset store closed".into()));
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let store = WsAssetStore::default();
        let s = store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = s.lock();
            panic!("poison the store");
        })
        .join();
        assert!(store.0.is_poisoned());
        store.deliver("/p", Ok(vec![1]));
        assert_eq!(block_on(store.fetch("/p")), Ok(vec![1]));
    }

    async fn read_all(r: &impl AssetReader, path: &str) -> Result<Vec<u8>, AssetReaderError> {
        let mut reader = r.read(Path::new(path)).await?;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        Ok(buf)
    }

    #[test]
    fn ws_reader_serves_replies_and_maps_failures_to_not_found() {
        let store = WsAssetStore::default();
        let reader = WsAssetReader {
            store: store.clone(),
        };
        store.deliver("/m/a.stl", Ok(vec![4, 5]));
        store.deliver("pkg/b.stl", Err("asset_not_found".into()));
        block_on(async {
            assert_eq!(read_all(&reader, "abs/m/a.stl").await.unwrap(), [4, 5]);
            assert!(matches!(
                read_all(&reader, "rel/pkg/b.stl").await,
                Err(AssetReaderError::NotFound(_))
            ));
            assert!(matches!(
                read_all(&reader, "neither/c.stl").await,
                Err(AssetReaderError::NotFound(_))
            ));
            assert!(reader.read_meta(Path::new("abs/m/a.stl")).await.is_err());
            assert!(reader.read_directory(Path::new("abs/m")).await.is_err());
            assert!(!reader.is_directory(Path::new("abs/m")).await.unwrap());
        });
    }

    /// Answers one request per connection: 200 with `body` for `/ok`, else 404.
    fn http_server(body: &'static [u8]) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let ok = String::from_utf8_lossy(&buf[..n]).starts_with("GET /ok ");
                let head = if ok { "200 OK" } else { "404 Not Found" };
                let body = if ok { body } else { b"" };
                let _ = write!(
                    s,
                    "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            }
        });
        format!("http:/{addr}")
    }

    #[test]
    fn http_reader_fetches_then_falls_back_to_the_server() {
        let base = http_server(b"mesh");
        let store = WsAssetStore::default();
        let reader = HttpAssetReader {
            mirror: None,
            store: store.clone(),
        };
        let missing = format!("{base}/missing");
        let gone = format!("{base}/gone");
        store.deliver(&http_url(Path::new(&missing), None), Ok(b"cached".to_vec()));
        store.deliver(
            &http_url(Path::new(&gone), None),
            Err("asset_not_found".into()),
        );
        block_on(async {
            assert_eq!(
                read_all(&reader, &format!("{base}/ok")).await.unwrap(),
                b"mesh"
            );
            assert_eq!(read_all(&reader, &missing).await.unwrap(), b"cached");
            assert!(matches!(
                read_all(&reader, &gone).await,
                Err(AssetReaderError::NotFound(_))
            ));
            assert!(reader.read_meta(Path::new("x")).await.is_err());
            assert!(reader.read_directory(Path::new("x")).await.is_err());
            assert!(!reader.is_directory(Path::new("x")).await.unwrap());
        });
    }

    #[test]
    fn plugin_registers_both_sources_and_the_store() {
        let mut app = App::new();
        app.add_plugins((
            GzAssetSourcesPlugin::default(),
            AssetPlugin::default(),
            bevy::time::TimePlugin,
            TaskPoolPlugin::default(),
        ));
        assert!(app.world().contains_resource::<WsAssetStore>());
        let server = app.world().resource::<AssetServer>();
        assert!(server.get_source(WS_SOURCE).is_ok());
        assert!(server.get_source(HTTP_SOURCE).is_ok());
    }
}
