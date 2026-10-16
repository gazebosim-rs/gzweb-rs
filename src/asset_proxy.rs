//! WebSocket-based asset proxy for loading `file://` assets in WASM.
//!
//! In the browser, the app cannot access the local filesystem. When a `file://`
//! URI is encountered on WASM, this module:
//!
//! 1. Converts it to a `wsasset://` path so Bevy's `AssetServer` routes it
//!    to our custom [`WsAssetReader`].
//! 2. The reader sends an `"asset"` request to the Gazebo WebSocket server.
//! 3. A Bevy system receives the response bytes and inserts them into the
//!    shared [`WsAssetResponseStore`].
//! 4. The reader picks up the bytes and returns them to Bevy's loader pipeline.
//!
//! This matches the existing Gazebo WebSocket protocol used by the JS frontend
//! (`Transport.ts` `getAsset` method):
//!
//! ```text
//! Client → Server:  "asset,,,file:///home/user/model/meshes/body.stl"
//! Server → Client:  binary frame "asset,<uri>,<type>,<payload>"
//! ```

use bevy::prelude::*;
use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

// ─── Shared response store ──────────────────────────────────────────────────

/// Outcome of an asset request: the file bytes, or the server's error code.
pub type AssetResult = Result<Vec<u8>, String>;

#[derive(Default)]
struct StoreInner {
    /// Responses that arrived before any reader asked for them.
    ready: HashMap<String, AssetResult>,
    /// Readers waiting for a response, keyed by `file://` URI.
    waiters: HashMap<String, Vec<async_channel::Sender<AssetResult>>>,
}

/// Thread-safe rendezvous between [`WsAssetReader`] and the websocket system.
///
/// Readers register a waiter and sleep on a channel; the websocket system
/// wakes them in [`WsAssetResponseStore::deliver`]. Both sides key by the
/// full `file://` URI.
#[derive(Resource, Clone, Default)]
pub struct WsAssetResponseStore(Arc<Mutex<StoreInner>>);

impl WsAssetResponseStore {
    /// Hand a response to every reader waiting on `uri`, or keep it until one asks.
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

    /// Wait for the response for `uri`.
    async fn wait_for(&self, uri: &str) -> AssetResult {
        let rx = {
            let mut inner = self.0.lock().unwrap();
            if let Some(result) = inner.ready.remove(uri) {
                return result;
            }
            let (tx, rx) = async_channel::bounded(1);
            inner.waiters.entry(uri.to_string()).or_default().push(tx);
            rx
        };
        rx.recv()
            .await
            .unwrap_or_else(|_| Err("asset store closed".to_string()))
    }
}

/// Bevy resource: set of `file://` URIs that need to be requested from
/// the WebSocket server. Each URI is added by `scene.rs` when it encounters
/// a WASM `file://` path. A system drains this and sends WebSocket requests.
#[derive(Resource, Default)]
pub struct PendingWsAssetRequests {
    /// URIs waiting to be sent. Key = file path, Value = true if request sent.
    pub requests: HashMap<String, bool>,
}

// ─── Custom AssetReader ─────────────────────────────────────────────────────

/// Reads asset bytes from the shared response store, which is populated
/// by WebSocket "asset" responses.
struct WsAssetReader {
    store: WsAssetResponseStore,
}

impl AssetReader for WsAssetReader {
    async fn read<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<impl Reader + 'a, AssetReaderError> {
        let key = format!("file:///{}", path.to_string_lossy());
        info!("WsAssetReader: waiting for bytes for '{}'", key);
        match self.store.wait_for(&key).await {
            Ok(bytes) => {
                info!("WsAssetReader: received {} bytes for '{}'", bytes.len(), key);
                Ok(VecReader::new(bytes))
            }
            Err(e) => {
                warn!("WsAssetReader: '{}' failed: {}", key, e);
                Err(AssetReaderError::NotFound(path.to_path_buf()))
            }
        }
    }

    async fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(
        &'a self,
        _path: &'a Path,
    ) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

// ─── System: send asset requests via WebSocket ──────────────────────────────

/// Sends pending asset requests to the Gazebo WebSocket server.
///
/// For each unrequested URI in [`PendingWsAssetRequests`], sends:
/// `"asset,,,<file://uri>"` matching the Gazebo WS protocol.
pub fn send_ws_asset_requests_system(
    mut pending: ResMut<PendingWsAssetRequests>,
    websocket: Option<NonSendMut<crate::websocket::GzWebSocket>>,
) {
    let Some(ws) = websocket else { return };

    for (uri, sent) in pending.requests.iter_mut() {
        if *sent {
            continue;
        }
        let msg = format!("asset,,,{}", uri);
        match ws.send_message(&msg) {
            Ok(()) => {
                info!("AssetProxy: requested '{}' via WebSocket", uri);
                *sent = true;
            }
            Err(e) => {
                warn!("AssetProxy: failed to request '{}': {}", uri, e);
            }
        }
    }
}

// ─── Plugin ─────────────────────────────────────────────────────────────────

/// Registers the `wsasset://` asset source and supporting resources/systems.
///
/// **Must be added before `DefaultPlugins`** (asset sources are built during
/// `AssetPlugin` init).
pub struct AssetProxyPlugin;

impl Plugin for AssetProxyPlugin {
    fn build(&self, app: &mut App) {
        // Create the shared response store
        let store = WsAssetResponseStore::default();
        let reader_store = store.clone();

        // Register the "wsasset" source with our custom reader
        app.register_asset_source(
            "wsasset",
            AssetSourceBuilder::new(move || {
                Box::new(WsAssetReader {
                    store: reader_store.clone(),
                })
            }),
        );

        // Insert resources and systems
        app.insert_resource(store)
            .init_resource::<PendingWsAssetRequests>()
            .add_systems(Update, send_ws_asset_requests_system);
    }
}
