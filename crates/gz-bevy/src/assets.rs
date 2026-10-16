//! Asset sources for Gazebo resources.
//!
//! gzweb loads a mesh/texture from its resolved URI over HTTP and, if that
//! fails, asks the Gazebo websocket server for it (`asset,,,<uri>`). We pick
//! the source up front instead:
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

use bevy::asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader};
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

fn http_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    match p.strip_prefix("http:/") {
        Some(rest) => format!("http://{rest}"),
        None => format!("https://{p}"),
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

/// Shared between [`WsAssetReader`] (on IO task threads) and the connection
/// system. Readers register a waiter and sleep; the connection system sends
/// the request and later wakes the waiters with the reply.
#[derive(Resource, Clone, Default)]
pub struct WsAssetStore(Arc<Mutex<StoreInner>>);

impl WsAssetStore {
    pub fn deliver(&self, uri: &str, result: AssetResult) {
        let mut inner = self.0.lock().unwrap();
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
        std::mem::take(&mut self.0.lock().unwrap().to_request)
    }

    /// Fail every waiting reader (e.g. the connection closed).
    pub fn fail_all(&self, reason: &str) {
        let mut inner = self.0.lock().unwrap();
        for (_, waiters) in inner.waiters.drain() {
            for tx in waiters {
                let _ = tx.try_send(Err(reason.to_string()));
            }
        }
        inner.to_request.clear();
    }

    async fn fetch(&self, uri: &str) -> AssetResult {
        let rx = {
            let mut inner = self.0.lock().unwrap();
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

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

// ─── HTTP(S) assets (Fuel and other web URLs) ───────────────────────────────

struct HttpAssetReader;

impl AssetReader for HttpAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let url = http_url(path);
        match fetch_bytes(&url).await {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(e) => {
                warn!("HTTP asset '{url}' failed: {e}");
                Err(AssetReaderError::NotFound(path.to_path_buf()))
            }
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    // ureq blocks, but asset readers run on the IO task pool.
    ureq::get(url)
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
pub struct GzAssetSourcesPlugin;

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
        .register_asset_source(HTTP_SOURCE, AssetSourceBuilder::new(|| Box::new(HttpAssetReader)))
        .insert_resource(store);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip() {
        assert_eq!(asset_path("/home/u/m/a.dae"), "wsasset://abs/home/u/m/a.dae");
        assert_eq!(ws_request_uri(Path::new("abs/home/u/m/a.dae")).unwrap(), "/home/u/m/a.dae");
        assert_eq!(asset_path("pkg/meshes/a.stl"), "wsasset://rel/pkg/meshes/a.stl");
        assert_eq!(ws_request_uri(Path::new("rel/pkg/meshes/a.stl")).unwrap(), "pkg/meshes/a.stl");
        assert_eq!(
            asset_path("https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"),
            "gzhttps://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"
        );
        assert_eq!(
            http_url(Path::new("fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb")),
            "https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"
        );
    }

    #[test]
    fn store_requests_once_and_wakes_all_waiters() {
        let store = WsAssetStore::default();
        let a = store.clone();
        let b = store.clone();
        let fa = bevy::tasks::futures_lite::future::block_on(async {
            let f1 = a.fetch("/x");
            let f2 = b.fetch("/x");
            let (r1, r2) = bevy::tasks::futures_lite::future::zip(
                f1,
                bevy::tasks::futures_lite::future::or(f2, async {
                    // Deliver once both waiters are registered.
                    assert_eq!(store.take_requests(), ["/x"]);
                    store.deliver("/x", Ok(vec![1, 2]));
                    std::future::pending::<AssetResult>().await
                }),
            )
            .await;
            (r1, r2)
        });
        assert_eq!(fa, (Ok(vec![1, 2]), Ok(vec![1, 2])));
    }
}
