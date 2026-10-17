//! `gz-parity-snapshot/1`: the canonical scene snapshot that both gzweb and
//! gzweb-rs export for differential testing. See `.agent/PARITY.md` §3.

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "gz-parity-snapshot/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: String,
    pub source: String,
    pub world: String,
    pub scene: SceneInfo,
    pub nodes: Vec<SnapNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneInfo {
    pub ambient: Option<[f64; 3]>,
    pub clear_color: Option<[f64; 3]>,
    pub camera: Option<CameraInfo>,
    pub grid_visible: Option<bool>,
    pub sky: Option<SkyInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub fov_deg: f64,
    pub near: f64,
    pub far: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkyInfo {
    pub cubemap: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Xform {
    pub pos: [f64; 3],
    pub quat: [f64; 4],
    pub scale: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapNode {
    pub path: String,
    pub kind: String,
    /// `None` when the renderer has no object for this node (gzweb skips
    /// e.g. model-level visuals).
    pub local: Option<Xform>,
    pub world: Option<Xform>,
    pub visible: bool,
    #[serde(default)]
    pub geometry: Option<GeometryInfo>,
    #[serde(default)]
    pub material: Option<MaterialInfo>,
    #[serde(default)]
    pub light: Option<LightInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryInfo {
    #[serde(rename = "type")]
    pub ty: String,
    pub aabb_world: Option<Aabb>,
    pub mesh_count: u32,
    pub vertex_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialInfo {
    pub color: [f64; 3],
    pub opacity: f64,
    pub transparent: bool,
    pub emissive: [f64; 3],
    pub metalness: Option<f64>,
    pub roughness: Option<f64>,
    #[serde(default)]
    pub maps: Maps,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Maps {
    pub albedo: Option<String>,
    pub normal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightInfo {
    #[serde(rename = "type")]
    pub ty: String,
    pub color: [f64; 3],
    pub intensity: f64,
    /// three.js `distance`; `null` for directional lights.
    pub range: Option<f64>,
    pub angle: Option<f64>,
    pub penumbra: Option<f64>,
    pub decay: Option<f64>,
    pub cast_shadow: bool,
    pub direction_world: Option<[f64; 3]>,
}
