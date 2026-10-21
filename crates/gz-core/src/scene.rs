//! Renderer-independent description of a Gazebo scene.
//!
//! [`SceneDesc::from_msg`] applies gzweb's rules (`SDFParser.ts`, `Scene.ts`)
//! to a `gz.msgs.Scene`, producing a tree that a renderer only has to
//! instantiate. All poses are in Gazebo's Z-up frame and are *local* to the
//! parent node, exactly like gzweb's three.js `Object3D` tree.

use std::collections::HashMap;

use gz_proto::msgs;

use crate::uri;

/// Translation + rotation (quaternion `[x, y, z, w]`), Gazebo frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub pos: [f64; 3],
    pub quat: [f64; 4],
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        pos: [0.0; 3],
        quat: [0.0, 0.0, 0.0, 1.0],
    };

    /// gzweb `setPose`: position and orientation are copied when present.
    /// The quaternion is normalized (three.js keeps it raw).
    pub fn from_msg(pose: Option<&msgs::Pose>) -> Pose {
        let Some(p) = pose else { return Pose::IDENTITY };
        let pos = p
            .position
            .as_ref()
            .map(|v| [v.x, v.y, v.z])
            .unwrap_or([0.0; 3]);
        let quat = p
            .orientation
            .as_ref()
            .map(|q| normalize_quat([q.x, q.y, q.z, q.w]))
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        Pose { pos, quat }
    }
}

fn normalize_quat(q: [f64; 4]) -> [f64; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 0.0 {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Model,
    Link,
    Visual,
    Light,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Model => "model",
            NodeKind::Link => "link",
            NodeKind::Visual => "visual",
            NodeKind::Light => "light",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub id: u32,
    pub kind: NodeKind,
    /// Unique `/`-joined name path; see [`PathAllocator`].
    pub path: String,
    pub pose: Pose,
    pub scale: [f64; 3],
    pub visible: bool,
    pub visual: Option<VisualDesc>,
    pub light: Option<LightDesc>,
    /// A link's sensors (none elsewhere). Not nodes: nothing renders them
    /// and the snapshot walk (PARITY.md §3) doesn't include them; the UI's
    /// scene tree lists them.
    pub sensors: Vec<SensorDesc>,
    pub children: Vec<Node>,
}

/// A sensor on a link (`gz.msgs.Sensor`): what the scene tree shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorDesc {
    pub name: String,
    pub id: u32,
    /// Its type as Gazebo names it (`camera`, `gpu_lidar`, `imu`, …).
    pub ty: String,
    pub pose: Pose,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualDesc {
    /// Lower-cased `Geometry.Type` name from the message, e.g. `box`.
    pub proto_type: String,
    pub geometry: GeometryDesc,
    pub material: MaterialDesc,
    pub cast_shadows: bool,
    pub transparency: f64,
}

/// Geometry in the visual's local Gazebo frame.
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryDesc {
    Box {
        size: [f64; 3],
    },
    /// Axis along +Z.
    Cylinder {
        radius: f64,
        length: f64,
    },
    /// Axis along +Z.
    Capsule {
        radius: f64,
        length: f64,
    },
    /// Axis along +Z, apex at +Z.
    Cone {
        radius: f64,
        length: f64,
    },
    Ellipsoid {
        radii: [f64; 3],
    },
    Sphere {
        radius: f64,
    },
    /// Plane through the origin; `size` is along local X/Y before rotating
    /// +Z onto `normal` with the shortest-arc rotation (gzweb `createPlane`).
    Plane {
        normal: [f64; 3],
        size: [f64; 2],
    },
    Mesh {
        /// The URI from the message.
        uri: String,
        /// What to load (see [`uri::mesh_load_uri`]).
        load_uri: String,
        submesh: Option<String>,
        center_submesh: bool,
    },
    Heightmap(HeightmapDesc),
    /// Recognised but not rendered yet.
    Unsupported(String),
}

/// A terrain: heights from the message or from a grayscale image, over
/// `size[0]` × `size[1]` metres centred on `origin`, `size[2]` metres from
/// black to white. Row 0 of the heights (or the image's top row) is +Y.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightmapDesc {
    /// The image to load (see [`uri::mesh_load_uri`]); `None` when the
    /// message carries the heights.
    pub image_uri: Option<String>,
    /// Row-major, `width` × `height`, in metres (from the message).
    pub heights: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub size: [f64; 3],
    pub origin: [f64; 3],
    /// Vertices per image pixel along each side (SDF `<sampling>`, ≥ 1).
    pub sampling: u32,
    /// Bottom layer first.
    pub layers: Vec<HeightmapLayer>,
    /// Between consecutive layers: (`min_height`, `fade_dist`).
    pub blends: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeightmapLayer {
    /// Load URIs, like a mesh's.
    pub diffuse: Option<String>,
    pub normal: Option<String>,
    /// Metres covered by one repeat of the textures.
    pub size: f64,
}

/// A visual's material as Gazebo renders it (gz-sim
/// `SceneManager::LoadMaterial`, gz-rendering `Ogre2Material`), not as
/// gzweb does. Colours are the message's floats, which Gazebo's ogre2 PBS
/// shaders use as **linear** values; texture fields are load URIs (see
/// [`uri::mesh_load_uri`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDesc {
    /// RGBA. Gazebo's base colour; its alpha multiplies the opacity.
    pub diffuse: Option<[f32; 4]>,
    /// Kept for reference only: ogre2 never uses it (gzweb mixes
    /// `0.4 * ambient + 0.6 * diffuse`).
    pub ambient: Option<[f32; 4]>,
    /// Scales every specular reflection (Ogre's `kS`).
    pub specular: Option<[f32; 3]>,
    pub emissive: [f32; 3],
    /// Depth bias for coplanar polygons: higher draws on top.
    pub render_order: f32,
    pub double_sided: bool,
    /// The legacy `Material.normal_map` field.
    pub normal_map: Option<String>,
    /// `Some` when the message carries a PBR block.
    pub pbr: Option<PbrDesc>,
    /// Whether the message had a material at all.
    pub present: bool,
}

impl MaterialDesc {
    /// A visual without a material.
    pub fn absent() -> MaterialDesc {
        MaterialDesc {
            diffuse: None,
            ambient: None,
            specular: None,
            emissive: [0.0; 3],
            render_order: 0.0,
            double_sided: false,
            normal_map: None,
            pbr: None,
            present: false,
        }
    }

    /// The normal map: the PBR one, else the legacy field.
    pub fn normal_map(&self) -> Option<&str> {
        self.pbr
            .as_ref()
            .and_then(|p| p.normal_map.as_deref())
            .or(self.normal_map.as_deref())
    }
}

/// `Material.PBR.WorkflowType`. Gazebo only renders the metal workflow;
/// for the specular one it keeps the maps but not the scalars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbrWorkflow {
    /// `METAL`, and `NONE` (which gz-sim never sends).
    Metal,
    Specular,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PbrDesc {
    pub workflow: PbrWorkflow,
    pub metalness: f64,
    pub roughness: f64,
    pub albedo_map: Option<String>,
    pub normal_map: Option<String>,
    /// Red channel (Ogre samples `.x`).
    pub metalness_map: Option<String>,
    /// Red channel.
    pub roughness_map: Option<String>,
    pub emissive_map: Option<String>,
    pub ambient_occlusion_map: Option<String>,
    pub light_map: Option<String>,
    /// UV set the light map uses.
    pub light_map_uv: u32,
    /// A per-material cubemap; not rendered (see DESIGN.md "Materials").
    pub environment_map: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    Point,
    Spot,
    Directional,
}

impl LightType {
    pub fn as_str(self) -> &'static str {
        match self {
            LightType::Point => "point",
            LightType::Spot => "spot",
            LightType::Directional => "directional",
        }
    }
}

/// Light parameters with three.js semantics (gzweb `spawnLightFromProto` +
/// `Scene.createLight`).
#[derive(Debug, Clone, PartialEq)]
pub struct LightDesc {
    pub ty: LightType,
    pub color: [f32; 3],
    /// `1/(1+linear) * 1/(1+quadratic)` (Blender-style attenuation, E=D=r=1).
    pub intensity: f64,
    /// three.js `distance` (0 = infinite). Not used for directional lights.
    pub range: f64,
    /// Spot cone half-angle (`spot_outer_angle`).
    pub angle: Option<f64>,
    /// Spot full-intensity half-angle (`spot_inner_angle`).
    pub inner_angle: Option<f64>,
    pub penumbra: Option<f64>,
    pub decay: Option<f64>,
    pub cast_shadows: bool,
    /// Direction in the light's local frame (default `0,0,-1`).
    pub direction: Option<[f64; 3]>,
    /// The message's `intensity` field (not used by gzweb).
    pub msg_intensity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneDesc {
    pub name: String,
    pub ambient: Option<[f32; 3]>,
    pub background: Option<[f32; 4]>,
    /// `Some(uri)` when the scene has a sky; the URI is the cubemap, if any
    /// (the `cubemap_uri` header value gzweb reads, else the field).
    pub sky: Option<Option<String>>,
    pub fog: Option<FogDesc>,
    pub grid: bool,
    pub shadows: bool,
    pub roots: Vec<Node>,
}

impl SceneDesc {
    /// Whether any light in the scene (at scene level or on a link) is
    /// directional: something to light a sky.
    pub fn has_directional_light(&self) -> bool {
        fn any(nodes: &[Node]) -> bool {
            nodes.iter().any(|n| {
                n.light
                    .as_ref()
                    .is_some_and(|l| l.ty == LightType::Directional)
                    || any(&n.children)
            })
        }
        any(&self.roots)
    }
}

/// `gz.msgs.Fog`; `None` in [`SceneDesc::fog`] for type `NONE` or an empty
/// linear range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogDesc {
    pub color: [f32; 3],
    pub falloff: FogFalloff,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FogFalloff {
    Linear { start: f32, end: f32 },
    Exponential { density: f32 },
    ExponentialSquared { density: f32 },
}

fn fog_desc(fog: &msgs::Fog) -> Option<FogDesc> {
    use msgs::fog::FogType;
    let falloff = match FogType::try_from(fog.r#type).ok()? {
        FogType::None => return None,
        // An empty range would divide by zero in the renderer.
        FogType::Linear if fog.end <= fog.start => return None,
        FogType::Linear => FogFalloff::Linear {
            start: fog.start,
            end: fog.end,
        },
        FogType::Exponential => FogFalloff::Exponential {
            density: fog.density,
        },
        FogType::Exponential2 => FogFalloff::ExponentialSquared {
            density: fog.density,
        },
    };
    let color = fog.color.as_ref().map_or([1.0; 3], |c| [c.r, c.g, c.b]);
    Some(FogDesc { color, falloff })
}

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    /// gzweb `enableLights`: lights attached to links start visible.
    pub enable_lights: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            enable_lights: true,
        }
    }
}

impl SceneDesc {
    pub fn from_msg(scene: &msgs::Scene, opts: BuildOptions) -> SceneDesc {
        Self::from_msg_with(scene, opts, &mut PathAllocator::default())
    }

    /// Like [`SceneDesc::from_msg`], allocating paths from `paths` so later
    /// additions (`scene/info`) stay unique.
    pub fn from_msg_with(
        scene: &msgs::Scene,
        opts: BuildOptions,
        paths: &mut PathAllocator,
    ) -> SceneDesc {
        let mut roots = Vec::new();
        for model in &scene.model {
            if let Some(node) = model_node(model, "", paths, opts) {
                roots.push(node);
            }
        }
        for light in &scene.light {
            roots.push(light_node(light, "", paths, true));
        }

        let sky = scene.sky.as_ref().map(|sky| {
            sky.header
                .as_ref()
                .and_then(|h| {
                    h.data
                        .iter()
                        .find(|d| d.key == "cubemap_uri")
                        .and_then(|d| d.value.first().cloned())
                })
                .or_else(|| (!sky.cubemap_uri.is_empty()).then(|| sky.cubemap_uri.clone()))
        });

        SceneDesc {
            name: scene.name.clone(),
            ambient: scene.ambient.as_ref().map(|c| [c.r, c.g, c.b]),
            background: scene.background.as_ref().map(|c| [c.r, c.g, c.b, c.a]),
            sky,
            fog: scene.fog.as_ref().and_then(fog_desc),
            grid: scene.grid,
            shadows: scene.shadows,
            roots,
        }
    }

    /// Nodes for the models of a `scene/info` update that are not already
    /// known by name (gzweb only spawns models whose name is new).
    pub fn new_models(
        scene: &msgs::Scene,
        known: &dyn Fn(&str) -> bool,
        paths: &mut PathAllocator,
        opts: BuildOptions,
    ) -> Vec<Node> {
        scene
            .model
            .iter()
            .filter(|m| !known(&m.name))
            .filter_map(|m| model_node(m, "", paths, opts))
            .collect()
    }

    /// Depth-first iterator over every node.
    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        let mut stack: Vec<&Node> = self.roots.iter().rev().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }
}

/// Assigns unique name paths: names joined with `/`; a repeated sibling name
/// gets `#1`, `#2`, ... in order of appearance.
#[derive(Debug, Default, Clone)]
pub struct PathAllocator {
    seen: HashMap<String, usize>,
}

impl PathAllocator {
    pub fn alloc(&mut self, parent: &str, name: &str) -> String {
        let base = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        };
        let count = self.seen.entry(base.clone()).or_insert(0);
        let path = if *count == 0 {
            base
        } else {
            format!("{base}#{count}")
        };
        *count += 1;
        path
    }
}

fn model_node(
    model: &msgs::Model,
    parent: &str,
    paths: &mut PathAllocator,
    opts: BuildOptions,
) -> Option<Node> {
    if model.deleted {
        return None;
    }
    let path = paths.alloc(parent, &model.name);
    let mut children = Vec::new();

    // Order matches the snapshot node walk (PARITY.md §3): links, the model's
    // own visuals, then nested models. gzweb ignores model-level visuals.
    for link in &model.link {
        children.push(link_node(link, &path, paths, opts));
    }
    for visual in &model.visual {
        if let Some(v) = visual_node(visual, &path, paths) {
            children.push(v);
        }
    }
    for nested in &model.model {
        if let Some(n) = model_node(nested, &path, paths, opts) {
            children.push(n);
        }
    }

    Some(Node {
        name: model.name.clone(),
        id: model.id,
        kind: NodeKind::Model,
        path,
        pose: Pose::from_msg(model.pose.as_ref()),
        scale: [1.0; 3],
        visible: true,
        visual: None,
        light: None,
        sensors: Vec::new(),
        children,
    })
}

fn link_node(
    link: &msgs::Link,
    parent: &str,
    paths: &mut PathAllocator,
    opts: BuildOptions,
) -> Node {
    let path = paths.alloc(parent, &link.name);
    let mut children = Vec::new();
    for visual in &link.visual {
        if let Some(v) = visual_node(visual, &path, paths) {
            children.push(v);
        }
    }
    for light in &link.light {
        children.push(light_node(light, &path, paths, opts.enable_lights));
    }
    Node {
        name: link.name.clone(),
        id: link.id,
        kind: NodeKind::Link,
        path,
        pose: Pose::from_msg(link.pose.as_ref()),
        scale: [1.0; 3],
        visible: true,
        visual: None,
        light: None,
        sensors: link
            .sensor
            .iter()
            .map(|s| SensorDesc {
                name: s.name.clone(),
                id: s.id,
                ty: s.r#type.clone(),
                pose: Pose::from_msg(s.pose.as_ref()),
            })
            .collect(),
        children,
    }
}

fn visual_node(visual: &msgs::Visual, parent: &str, paths: &mut PathAllocator) -> Option<Node> {
    if visual.delete_me {
        return None;
    }
    let path = paths.alloc(parent, &visual.name);
    let geometry = visual.geometry.as_ref();
    let geom = geometry
        .map(geometry_desc)
        .unwrap_or(GeometryDesc::Unsupported("none".into()));

    // gzweb applies `<mesh><scale>` to the visual node and ignores
    // `Visual.scale`.
    let scale = geometry
        .and_then(|g| g.mesh.as_ref())
        .and_then(|m| m.scale.as_ref())
        .map(|s| [s.x, s.y, s.z])
        .unwrap_or([1.0; 3]);

    let proto_type = geometry
        .and_then(|g| msgs::geometry::Type::try_from(g.r#type).ok())
        .map(|t| t.as_str_name().to_ascii_lowercase())
        .unwrap_or_else(|| "none".to_string());

    Some(Node {
        name: visual.name.clone(),
        id: visual.id,
        kind: NodeKind::Visual,
        path,
        pose: Pose::from_msg(visual.pose.as_ref()),
        scale,
        visible: true,
        visual: Some(VisualDesc {
            proto_type,
            geometry: geom,
            material: material_desc(visual.material.as_ref()),
            cast_shadows: visual.cast_shadows,
            transparency: visual.transparency,
        }),
        light: None,
        sensors: Vec::new(),
        children: Vec::new(),
    })
}

/// Dispatches on which geometry sub-message is present, in gzweb's order
/// (`SDFParser.createGeom`), not on `Geometry.type`.
fn geometry_desc(g: &msgs::Geometry) -> GeometryDesc {
    let v3 =
        |v: &Option<msgs::Vector3d>, d: [f64; 3]| v.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or(d);
    if let Some(b) = &g.r#box {
        GeometryDesc::Box {
            size: v3(&b.size, [1.0; 3]),
        }
    } else if let Some(c) = &g.cylinder {
        GeometryDesc::Cylinder {
            radius: c.radius,
            length: c.length,
        }
    } else if let Some(c) = &g.capsule {
        GeometryDesc::Capsule {
            radius: c.radius,
            length: c.length,
        }
    } else if let Some(c) = &g.cone {
        GeometryDesc::Cone {
            radius: c.radius,
            length: c.length,
        }
    } else if let Some(e) = &g.ellipsoid {
        GeometryDesc::Ellipsoid {
            radii: v3(&e.radii, [1.0; 3]),
        }
    } else if let Some(s) = &g.sphere {
        GeometryDesc::Sphere { radius: s.radius }
    } else if let Some(p) = &g.plane {
        let normal = p
            .normal
            .as_ref()
            .map(|n| [n.x, n.y, n.z])
            .unwrap_or([0.0, 0.0, 1.0]);
        let size = p.size.as_ref().map(|s| [s.x, s.y]).unwrap_or([1.0, 1.0]);
        GeometryDesc::Plane { normal, size }
    } else if let Some(m) = &g.mesh {
        GeometryDesc::Mesh {
            uri: m.filename.clone(),
            load_uri: uri::mesh_load_uri(&m.filename),
            submesh: (!m.submesh.is_empty()).then(|| m.submesh.clone()),
            center_submesh: m.center_submesh,
        }
    } else if let Some(h) = &g.heightmap {
        GeometryDesc::Heightmap(heightmap_desc(h))
    } else {
        GeometryDesc::Unsupported(
            msgs::geometry::Type::try_from(g.r#type)
                .map(|t| t.as_str_name().to_ascii_lowercase())
                .unwrap_or_else(|_| "unknown".into()),
        )
    }
}

fn heightmap_desc(h: &msgs::HeightmapGeom) -> HeightmapDesc {
    let v3 =
        |v: &Option<msgs::Vector3d>, d: [f64; 3]| v.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or(d);
    let load = |u: &str| (!u.is_empty()).then(|| uri::mesh_load_uri(u));
    let (width, height) = (h.width.max(0) as u32, h.height.max(0) as u32);
    let has_heights =
        width > 0 && height > 0 && h.heights.len() == (width as usize) * (height as usize);
    HeightmapDesc {
        image_uri: if has_heights { None } else { load(&h.filename) },
        heights: if has_heights {
            h.heights.clone()
        } else {
            Vec::new()
        },
        width: if has_heights { width } else { 0 },
        height: if has_heights { height } else { 0 },
        size: v3(&h.size, [1.0; 3]),
        origin: v3(&h.origin, [0.0; 3]),
        sampling: h.sampling.max(1),
        layers: h
            .texture
            .iter()
            .map(|t| HeightmapLayer {
                diffuse: load(&t.diffuse),
                normal: load(&t.normal),
                size: if t.size > 0.0 { t.size } else { 1.0 },
            })
            .collect(),
        blends: h
            .blend
            .iter()
            .map(|b| [b.min_height, b.fade_dist])
            .collect(),
    }
}

fn material_desc(mat: Option<&msgs::Material>) -> MaterialDesc {
    let Some(mat) = mat else {
        return MaterialDesc::absent();
    };
    let rgba = |c: &msgs::Color| [c.r, c.g, c.b, c.a];
    let rgb = |c: &msgs::Color| [c.r, c.g, c.b];
    let load = |u: &str| (!u.is_empty()).then(|| uri::mesh_load_uri(u));
    MaterialDesc {
        diffuse: mat.diffuse.as_ref().map(rgba),
        ambient: mat.ambient.as_ref().map(rgba),
        specular: mat.specular.as_ref().map(rgb),
        emissive: mat.emissive.as_ref().map(rgb).unwrap_or([0.0; 3]),
        render_order: mat.render_order as f32,
        double_sided: mat.double_sided,
        normal_map: load(&mat.normal_map),
        pbr: mat.pbr.as_ref().map(|p| PbrDesc {
            workflow: match msgs::material::pbr::WorkflowType::try_from(p.r#type) {
                Ok(msgs::material::pbr::WorkflowType::Specular) => PbrWorkflow::Specular,
                _ => PbrWorkflow::Metal,
            },
            metalness: p.metalness,
            roughness: p.roughness,
            albedo_map: load(&p.albedo_map),
            normal_map: load(&p.normal_map),
            metalness_map: load(&p.metalness_map),
            roughness_map: load(&p.roughness_map),
            emissive_map: load(&p.emissive_map),
            ambient_occlusion_map: load(&p.ambient_occlusion_map),
            light_map: load(&p.light_map),
            light_map_uv: p.light_map_texcoord_set,
            environment_map: load(&p.environment_map),
        }),
        present: true,
    }
}

fn light_node(light: &msgs::Light, parent: &str, paths: &mut PathAllocator, visible: bool) -> Node {
    let path = paths.alloc(parent, &light.name);
    Node {
        name: light.name.clone(),
        id: light.id,
        kind: NodeKind::Light,
        path,
        pose: Pose::from_msg(light.pose.as_ref()),
        scale: [1.0; 3],
        visible: visible && !light.is_light_off,
        visual: None,
        light: Some(light_desc(light)),
        sensors: Vec::new(),
        children: Vec::new(),
    }
}

/// gzweb `spawnLightFromProto` + `Scene.createLight`.
pub fn light_desc(light: &msgs::Light) -> LightDesc {
    let l = light.attenuation_linear as f64;
    let q = light.attenuation_quadratic as f64;
    let intensity = (1.0 / (1.0 + l)) * (1.0 / (1.0 + q));

    let ty = match msgs::light::LightType::try_from(light.r#type) {
        Ok(msgs::light::LightType::Spot) => LightType::Spot,
        Ok(msgs::light::LightType::Directional) => LightType::Directional,
        _ => LightType::Point,
    };
    let color = light
        .diffuse
        .as_ref()
        .map(|c| [c.r, c.g, c.b])
        .unwrap_or([1.0; 3]);
    let direction = match ty {
        LightType::Point => None,
        _ => Some(
            light
                .direction
                .as_ref()
                .map(|d| [d.x, d.y, d.z])
                .unwrap_or([0.0, 0.0, -1.0]),
        ),
    };

    let (angle, penumbra, decay, inner_angle) = match ty {
        LightType::Spot => {
            let inner = light.spot_inner_angle as f64;
            let outer = light.spot_outer_angle as f64;
            // gzweb clamps this to >= 1 (and produces NaN for 0/0); we keep
            // three.js' valid [0, 1] range. See PORT_PLAN.md §2.5.
            let mean = (inner + outer) / 2.0;
            let penumbra = if mean > 0.0 {
                ((outer - inner) / mean).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (
                Some(outer),
                Some(penumbra),
                Some(light.spot_falloff as f64),
                Some(inner.min(outer)),
            )
        }
        // three.js PointLight's default decay.
        LightType::Point => (None, None, Some(1.0), None),
        LightType::Directional => (None, None, None, None),
    };

    LightDesc {
        ty,
        color,
        intensity,
        range: match ty {
            LightType::Directional => 0.0,
            _ => light.range as f64,
        },
        angle,
        inner_angle,
        penumbra,
        decay,
        cast_shadows: light.cast_shadows,
        direction,
        msg_intensity: light.intensity as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v3(x: f64, y: f64, z: f64) -> Option<msgs::Vector3d> {
        Some(msgs::Vector3d {
            x,
            y,
            z,
            ..Default::default()
        })
    }

    fn color(r: f32, g: f32, b: f32) -> Option<msgs::Color> {
        Some(msgs::Color {
            r,
            g,
            b,
            a: 1.0,
            ..Default::default()
        })
    }

    fn box_visual(name: &str) -> msgs::Visual {
        msgs::Visual {
            name: name.into(),
            geometry: Some(msgs::Geometry {
                r#type: msgs::geometry::Type::Box as i32,
                r#box: Some(msgs::BoxGeom {
                    size: v3(1.0, 2.0, 3.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn tree_and_paths() {
        let scene = msgs::Scene {
            model: vec![msgs::Model {
                name: "m".into(),
                pose: Some(msgs::Pose {
                    position: v3(1.0, 2.0, 3.0),
                    ..Default::default()
                }),
                link: vec![msgs::Link {
                    name: "link".into(),
                    visual: vec![box_visual("v"), box_visual("v")],
                    sensor: vec![msgs::Sensor {
                        name: "cam".into(),
                        id: 9,
                        r#type: "camera".into(),
                        pose: Some(msgs::Pose {
                            position: v3(0.0, 0.0, 0.5),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                model: vec![msgs::Model {
                    name: "inner".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            light: vec![msgs::Light {
                name: "sun".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let desc = SceneDesc::from_msg(&scene, BuildOptions::default());
        let paths: Vec<_> = desc.iter().map(|n| (n.path.as_str(), n.kind)).collect();
        assert_eq!(
            paths,
            [
                ("m", NodeKind::Model),
                ("m/link", NodeKind::Link),
                ("m/link/v", NodeKind::Visual),
                ("m/link/v#1", NodeKind::Visual),
                ("m/inner", NodeKind::Model),
                ("sun", NodeKind::Light),
            ]
        );
        assert_eq!(desc.roots[0].pose.pos, [1.0, 2.0, 3.0]);
        // Sensors stay on their link, not in the node walk.
        let link = &desc.roots[0].children[0];
        assert_eq!(
            link.sensors,
            [SensorDesc {
                name: "cam".into(),
                id: 9,
                ty: "camera".into(),
                pose: Pose {
                    pos: [0.0, 0.0, 0.5],
                    ..Pose::IDENTITY
                },
            }]
        );
        assert!(desc.roots[0].sensors.is_empty());
        assert_eq!(
            desc.roots[0].children[0].children[0]
                .visual
                .as_ref()
                .unwrap()
                .geometry,
            GeometryDesc::Box {
                size: [1.0, 2.0, 3.0]
            }
        );
    }

    #[test]
    fn materials_keep_gazebo_colours() {
        // Gazebo renders the diffuse colour; ambient is only kept.
        let m = msgs::Material {
            ambient: color(1.0, 0.0, 0.0),
            diffuse: color(0.0, 1.0, 0.0),
            specular: color(0.5, 0.5, 0.5),
            render_order: 2.0,
            ..Default::default()
        };
        let d = material_desc(Some(&m));
        assert_eq!(d.diffuse, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(d.ambient, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(d.specular, Some([0.5; 3]));
        assert_eq!(d.render_order, 2.0);
        assert_eq!(material_desc(None), MaterialDesc::absent());
    }

    #[test]
    fn light_rules() {
        let l = msgs::Light {
            r#type: msgs::light::LightType::Spot as i32,
            attenuation_linear: 1.0,
            attenuation_quadratic: 3.0,
            spot_inner_angle: 0.2,
            spot_outer_angle: 0.6,
            spot_falloff: 2.0,
            range: 15.0,
            ..Default::default()
        };
        let d = light_desc(&l);
        assert_eq!(d.ty, LightType::Spot);
        assert!((d.intensity - 0.125).abs() < 1e-12);
        assert_eq!(d.angle, Some(0.6000000238418579));
        assert_eq!(d.inner_angle, Some(0.20000000298023224));
        assert_eq!(d.direction, Some([0.0, 0.0, -1.0]));
        assert_eq!(d.range, 15.0);
        // (0.6-0.2)/0.4 = 1.0
        assert!((d.penumbra.unwrap() - 1.0).abs() < 1e-6);

        let dir = light_desc(&msgs::Light {
            r#type: msgs::light::LightType::Directional as i32,
            direction: v3(0.5, 0.1, -0.9),
            ..Default::default()
        });
        assert_eq!(dir.direction, Some([0.5, 0.1, -0.9]));
        assert_eq!(dir.intensity, 1.0);
    }

    #[test]
    fn geometry_dispatch_follows_gzweb_order() {
        // A box sub-message wins even if `type` says sphere.
        let g = msgs::Geometry {
            r#type: msgs::geometry::Type::Sphere as i32,
            r#box: Some(Default::default()),
            sphere: Some(msgs::SphereGeom {
                radius: 2.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(geometry_desc(&g), GeometryDesc::Box { size: [1.0; 3] });

        let plane = msgs::Geometry {
            plane: Some(Default::default()),
            ..Default::default()
        };
        assert_eq!(
            geometry_desc(&plane),
            GeometryDesc::Plane {
                normal: [0.0, 0.0, 1.0],
                size: [1.0, 1.0]
            }
        );
    }

    #[test]
    fn heightmaps_from_an_image_or_the_message() {
        let tex = |d: &str, n: &str, size| msgs::heightmap_geom::Texture {
            diffuse: d.into(),
            normal: n.into(),
            size,
        };
        let image = msgs::HeightmapGeom {
            filename: "/////root/.gz/fuel/fuel.gazebosim.org/o/models/h/1/materials/textures/h.png"
                .into(),
            size: v3(129.0, 129.0, 10.0),
            origin: v3(1.0, 2.0, 3.0),
            texture: vec![
                tex("model://h/d.png", "", 2.0),
                tex("", "file:///n.png", 0.0),
            ],
            blend: vec![msgs::heightmap_geom::Blend {
                min_height: 2.0,
                fade_dist: 5.0,
            }],
            sampling: 0,
            // Inconsistent heights are ignored.
            heights: vec![1.0],
            width: 2,
            height: 2,
            ..Default::default()
        };
        let d = heightmap_desc(&image);
        assert_eq!(
            d.image_uri.as_deref(),
            Some("https://fuel.gazebosim.org/1.0/o/models/h/1/files/materials/textures/h.png")
        );
        assert!(d.heights.is_empty() && d.width == 0);
        assert_eq!(
            (d.size, d.origin, d.sampling),
            ([129.0, 129.0, 10.0], [1.0, 2.0, 3.0], 1)
        );
        assert_eq!(
            d.layers,
            vec![
                HeightmapLayer {
                    diffuse: Some("h/d.png".into()),
                    normal: None,
                    size: 2.0
                },
                HeightmapLayer {
                    diffuse: None,
                    normal: Some("/n.png".into()),
                    size: 1.0
                },
            ]
        );
        assert_eq!(d.blends, vec![[2.0, 5.0]]);

        let inline = heightmap_desc(&msgs::HeightmapGeom {
            heights: vec![0.0, 1.0, 2.0, 3.0],
            width: 2,
            height: 2,
            sampling: 2,
            ..image
        });
        assert_eq!(inline.image_uri, None);
        assert_eq!((inline.width, inline.height, inline.sampling), (2, 2, 2));
        assert_eq!(inline.heights, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn mesh_scale_goes_on_the_visual() {
        let v = msgs::Visual {
            name: "v".into(),
            scale: v3(9.0, 9.0, 9.0),
            geometry: Some(msgs::Geometry {
                mesh: Some(msgs::MeshGeom {
                    filename: "model://m/meshes/a.dae".into(),
                    scale: v3(2.0, 3.0, 4.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let n = visual_node(&v, "", &mut PathAllocator::default()).unwrap();
        assert_eq!(n.scale, [2.0, 3.0, 4.0]);
        let geometry = n.visual.unwrap().geometry;
        assert!(
            matches!(&geometry, GeometryDesc::Mesh { load_uri, .. } if load_uri == "m/meshes/a.dae"),
            "{geometry:?}"
        );
    }

    fn quat(x: f64, y: f64, z: f64, w: f64) -> Option<msgs::Quaternion> {
        Some(msgs::Quaternion {
            x,
            y,
            z,
            w,
            ..Default::default()
        })
    }

    #[test]
    fn pose_from_msg() {
        assert_eq!(Pose::from_msg(None), Pose::IDENTITY);
        // Missing position / orientation default like gzweb's setPose.
        assert_eq!(Pose::from_msg(Some(&msgs::Pose::default())), Pose::IDENTITY);
        let p = Pose::from_msg(Some(&msgs::Pose {
            position: v3(1.0, 2.0, 3.0),
            orientation: quat(0.0, 0.0, 2.0, 2.0),
            ..Default::default()
        }));
        assert_eq!(p.pos, [1.0, 2.0, 3.0]);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        for (got, want) in p.quat.iter().zip([0.0, 0.0, h, h]) {
            assert!((got - want).abs() < 1e-12, "{:?}", p.quat);
        }
        // A zero quaternion (proto default when only some fields are set)
        // becomes the identity rather than NaN.
        let z = Pose::from_msg(Some(&msgs::Pose {
            orientation: quat(0.0, 0.0, 0.0, 0.0),
            ..Default::default()
        }));
        assert_eq!(z.quat, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn kind_and_light_type_names() {
        let kinds = [
            NodeKind::Model,
            NodeKind::Link,
            NodeKind::Visual,
            NodeKind::Light,
        ];
        assert_eq!(
            kinds.map(NodeKind::as_str),
            ["model", "link", "visual", "light"]
        );
        let types = [LightType::Point, LightType::Spot, LightType::Directional];
        assert_eq!(
            types.map(LightType::as_str),
            ["point", "spot", "directional"]
        );
    }

    #[test]
    fn path_allocator_suffixes_per_parent() {
        let mut p = PathAllocator::default();
        assert_eq!(p.alloc("", "a"), "a");
        assert_eq!(p.alloc("", "a"), "a#1");
        assert_eq!(p.alloc("", "a"), "a#2");
        // Same name under different parents is not a repeat.
        assert_eq!(p.alloc("a", "x"), "a/x");
        assert_eq!(p.alloc("a#1", "x"), "a#1/x");
        assert_eq!(p.alloc("a", "x"), "a/x#1");
    }

    fn header(key: &str, value: &str) -> Option<msgs::Header> {
        Some(msgs::Header {
            data: vec![msgs::header::Map {
                key: key.into(),
                value: vec![value.into()],
            }],
            ..Default::default()
        })
    }

    #[test]
    fn scene_level_fields() {
        let scene = msgs::Scene {
            name: "w".into(),
            ambient: color(0.1, 0.2, 0.3),
            background: Some(msgs::Color {
                r: 0.4,
                g: 0.5,
                b: 0.6,
                a: 0.0,
                ..Default::default()
            }),
            grid: true,
            shadows: true,
            sky: Some(msgs::Sky {
                header: header("cubemap_uri", "file:///sky.dds"),
                ..Default::default()
            }),
            ..Default::default()
        };
        let d = SceneDesc::from_msg(&scene, BuildOptions::default());
        assert_eq!(d.name, "w");
        assert_eq!(d.ambient, Some([0.1, 0.2, 0.3]));
        assert_eq!(d.background, Some([0.4, 0.5, 0.6, 0.0]));
        assert!(d.grid && d.shadows);
        assert_eq!(d.sky, Some(Some("file:///sky.dds".into())));
        assert!(d.roots.is_empty());

        // Harmonic's own field, when there is no header.
        let field = msgs::Scene {
            sky: Some(msgs::Sky {
                cubemap_uri: "model://sky/sky.dds".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let d = SceneDesc::from_msg(&field, BuildOptions::default());
        assert_eq!(d.sky, Some(Some("model://sky/sky.dds".into())));

        // A sky without a cubemap header, or with other keys, has no URI.
        for sky in [
            msgs::Sky::default(),
            msgs::Sky {
                header: header("other", "x"),
                ..Default::default()
            },
        ] {
            let scene = msgs::Scene {
                sky: Some(sky),
                ..Default::default()
            };
            let d = SceneDesc::from_msg(&scene, BuildOptions::default());
            assert_eq!(d.sky, Some(None));
            assert_eq!(d.ambient, None);
            assert_eq!(d.background, None);
        }
    }

    #[test]
    fn fog_types() {
        use msgs::fog::FogType;
        let fog = |ty: FogType| {
            let scene = msgs::Scene {
                fog: Some(msgs::Fog {
                    r#type: ty as i32,
                    color: color(0.5, 0.6, 0.7),
                    density: 0.1,
                    start: 2.0,
                    end: 20.0,
                    ..Default::default()
                }),
                ..Default::default()
            };
            SceneDesc::from_msg(&scene, BuildOptions::default()).fog
        };
        assert_eq!(fog(FogType::None), None);
        let linear = fog(FogType::Linear).unwrap();
        assert_eq!(linear.color, [0.5, 0.6, 0.7]);
        assert_eq!(
            linear.falloff,
            FogFalloff::Linear {
                start: 2.0,
                end: 20.0
            }
        );
        assert_eq!(
            fog(FogType::Exponential).unwrap().falloff,
            FogFalloff::Exponential { density: 0.1 }
        );
        assert_eq!(
            fog(FogType::Exponential2).unwrap().falloff,
            FogFalloff::ExponentialSquared { density: 0.1 }
        );
        // An empty linear range is no fog.
        let empty = msgs::Fog {
            r#type: FogType::Linear as i32,
            start: 5.0,
            end: 5.0,
            ..Default::default()
        };
        assert_eq!(fog_desc(&empty), None);
        // No colour: white; an unknown type: no fog.
        let bare = msgs::Fog {
            r#type: FogType::Linear as i32,
            end: 1.0,
            ..Default::default()
        };
        assert_eq!(fog_desc(&bare).unwrap().color, [1.0; 3]);
        let unknown = msgs::Fog {
            r#type: 42,
            ..Default::default()
        };
        assert_eq!(fog_desc(&unknown), None);
    }

    #[test]
    fn deleted_entities_and_model_level_visuals() {
        let scene = msgs::Scene {
            model: vec![
                msgs::Model {
                    name: "gone".into(),
                    deleted: true,
                    ..Default::default()
                },
                msgs::Model {
                    name: "m".into(),
                    id: 7,
                    visual: vec![box_visual("mv")],
                    link: vec![msgs::Link {
                        name: "l".into(),
                        id: 8,
                        visual: vec![
                            box_visual("kept"),
                            msgs::Visual {
                                delete_me: true,
                                ..box_visual("dropped")
                            },
                        ],
                        ..Default::default()
                    }],
                    model: vec![msgs::Model {
                        name: "nested_gone".into(),
                        deleted: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let d = SceneDesc::from_msg(&scene, BuildOptions::default());
        let paths: Vec<_> = d.iter().map(|n| n.path.as_str()).collect();
        // Links first, then the model's own visuals (PARITY.md node order).
        assert_eq!(paths, ["m", "m/l", "m/l/kept", "m/mv"]);
        assert_eq!(d.roots[0].id, 7);
        assert_eq!(d.roots[0].children[0].id, 8);
    }

    fn typed(ty: msgs::geometry::Type, g: msgs::Geometry) -> msgs::Visual {
        msgs::Visual {
            name: "v".into(),
            geometry: Some(msgs::Geometry {
                r#type: ty as i32,
                ..g
            }),
            ..Default::default()
        }
    }

    #[test]
    fn every_geometry_kind() {
        use msgs::geometry::Type;
        let cases: Vec<(msgs::Visual, &str, GeometryDesc)> = vec![
            (
                typed(
                    Type::Cylinder,
                    msgs::Geometry {
                        cylinder: Some(msgs::CylinderGeom {
                            radius: 0.5,
                            length: 2.0,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "cylinder",
                GeometryDesc::Cylinder {
                    radius: 0.5,
                    length: 2.0,
                },
            ),
            (
                typed(
                    Type::Capsule,
                    msgs::Geometry {
                        capsule: Some(msgs::CapsuleGeom {
                            radius: 0.25,
                            length: 1.0,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "capsule",
                GeometryDesc::Capsule {
                    radius: 0.25,
                    length: 1.0,
                },
            ),
            (
                typed(
                    Type::Cone,
                    msgs::Geometry {
                        cone: Some(msgs::ConeGeom {
                            radius: 1.0,
                            length: 3.0,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "cone",
                GeometryDesc::Cone {
                    radius: 1.0,
                    length: 3.0,
                },
            ),
            (
                typed(
                    Type::Ellipsoid,
                    msgs::Geometry {
                        ellipsoid: Some(msgs::EllipsoidGeom {
                            radii: v3(1.0, 2.0, 3.0),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "ellipsoid",
                GeometryDesc::Ellipsoid {
                    radii: [1.0, 2.0, 3.0],
                },
            ),
            (
                typed(
                    Type::Ellipsoid,
                    msgs::Geometry {
                        ellipsoid: Some(msgs::EllipsoidGeom::default()),
                        ..Default::default()
                    },
                ),
                "ellipsoid",
                GeometryDesc::Ellipsoid { radii: [1.0; 3] },
            ),
            (
                typed(
                    Type::Sphere,
                    msgs::Geometry {
                        sphere: Some(msgs::SphereGeom {
                            radius: 0.75,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "sphere",
                GeometryDesc::Sphere { radius: 0.75 },
            ),
            (
                typed(
                    Type::Plane,
                    msgs::Geometry {
                        plane: Some(msgs::PlaneGeom {
                            normal: v3(0.0, 1.0, 0.0),
                            size: Some(msgs::Vector2d {
                                x: 4.0,
                                y: 2.0,
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "plane",
                GeometryDesc::Plane {
                    normal: [0.0, 1.0, 0.0],
                    size: [4.0, 2.0],
                },
            ),
            (
                typed(
                    Type::Mesh,
                    msgs::Geometry {
                        mesh: Some(msgs::MeshGeom {
                            filename: "https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb"
                                .into(),
                            submesh: "body".into(),
                            center_submesh: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                "mesh",
                GeometryDesc::Mesh {
                    uri: "https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb".into(),
                    load_uri: "https://fuel.gazebosim.org/1.0/o/models/m/1/files/a.glb".into(),
                    submesh: Some("body".into()),
                    center_submesh: true,
                },
            ),
            (
                typed(
                    Type::Heightmap,
                    msgs::Geometry {
                        heightmap: Some(msgs::HeightmapGeom::default()),
                        ..Default::default()
                    },
                ),
                "heightmap",
                GeometryDesc::Heightmap(HeightmapDesc {
                    image_uri: None,
                    heights: vec![],
                    width: 0,
                    height: 0,
                    size: [1.0; 3],
                    origin: [0.0; 3],
                    sampling: 1,
                    layers: vec![],
                    blends: vec![],
                }),
            ),
            // No sub-message: named after `type`, as gzweb creates nothing.
            (
                typed(Type::Polyline, msgs::Geometry::default()),
                "polyline",
                GeometryDesc::Unsupported("polyline".into()),
            ),
            (
                msgs::Visual {
                    geometry: Some(msgs::Geometry {
                        r#type: 999,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                "none",
                GeometryDesc::Unsupported("unknown".into()),
            ),
            (
                msgs::Visual::default(),
                "none",
                GeometryDesc::Unsupported("none".into()),
            ),
        ];
        for (visual, proto_type, want) in cases {
            let n = visual_node(&visual, "p", &mut PathAllocator::default()).unwrap();
            let v = n.visual.unwrap();
            assert_eq!(v.geometry, want);
            assert_eq!(v.proto_type, proto_type, "{want:?}");
            // Only <mesh><scale> scales the node.
            assert_eq!(n.scale, [1.0; 3]);
        }
    }

    #[test]
    fn material_variants() {
        let ambient_only = msgs::Material {
            ambient: color(1.0, 0.0, 0.0),
            normal_map: "model://m/materials/textures/n.png".into(),
            ..Default::default()
        };
        let d = material_desc(Some(&ambient_only));
        assert_eq!(d.diffuse, None);
        assert!(d.present);
        assert!(!material_desc(None).present);
        // The legacy normal map, as a load URI.
        assert_eq!(d.normal_map(), Some("m/materials/textures/n.png"));

        let full = msgs::Material {
            diffuse: Some(msgs::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.25,
                ..Default::default()
            }),
            emissive: color(0.0, 0.5, 0.0),
            double_sided: true,
            normal_map: "/legacy.png".into(),
            pbr: Some(msgs::material::Pbr {
                r#type: msgs::material::pbr::WorkflowType::Metal as i32,
                metalness: 0.8,
                roughness: 0.3,
                albedo_map: "file:///t/albedo.png".into(),
                normal_map: "https://fuel.gazebosim.org/1.0/o/models/m/1/files/n.png".into(),
                metalness_map:
                    "/root/.gz/fuel/fuel.gazebosim.org/o/models/m/1/materials/textures/m.png".into(),
                roughness_map: "model://m/r.png".into(),
                emissive_map: "e.png".into(),
                ambient_occlusion_map: "ao.png".into(),
                light_map: "l.png".into(),
                light_map_texcoord_set: 1,
                environment_map: "env.dds".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let d = material_desc(Some(&full));
        assert_eq!(d.diffuse, Some([1.0, 1.0, 1.0, 0.25]));
        assert_eq!(d.emissive, [0.0, 0.5, 0.0]);
        assert!(d.double_sided);
        let fuel = "https://fuel.gazebosim.org/1.0/o/models/m/1/files";
        assert_eq!(
            d.pbr,
            Some(PbrDesc {
                workflow: PbrWorkflow::Metal,
                metalness: 0.8,
                roughness: 0.3,
                albedo_map: Some("/t/albedo.png".into()),
                normal_map: Some(format!("{fuel}/n.png")),
                metalness_map: Some(format!("{fuel}/materials/textures/m.png")),
                roughness_map: Some("m/r.png".into()),
                emissive_map: Some("e.png".into()),
                ambient_occlusion_map: Some("ao.png".into()),
                light_map: Some("l.png".into()),
                light_map_uv: 1,
                environment_map: Some("env.dds".into()),
            })
        );
        // The PBR normal map wins over the legacy one.
        assert_eq!(d.normal_map(), Some(format!("{fuel}/n.png").as_str()));

        // The specular workflow; NONE counts as metal.
        for (ty, want) in [
            (
                msgs::material::pbr::WorkflowType::Specular,
                PbrWorkflow::Specular,
            ),
            (msgs::material::pbr::WorkflowType::None, PbrWorkflow::Metal),
        ] {
            let m = msgs::Material {
                pbr: Some(msgs::material::Pbr {
                    r#type: ty as i32,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert_eq!(material_desc(Some(&m)).pbr.unwrap().workflow, want);
        }
    }

    #[test]
    fn visual_flags_carry_through() {
        let v = msgs::Visual {
            cast_shadows: true,
            transparency: 0.4,
            id: 12,
            pose: Some(msgs::Pose {
                position: v3(0.0, 0.0, 1.0),
                ..Default::default()
            }),
            ..box_visual("v")
        };
        let n = visual_node(&v, "m/l", &mut PathAllocator::default()).unwrap();
        assert_eq!(n.path, "m/l/v");
        assert_eq!(n.id, 12);
        assert_eq!(n.pose.pos, [0.0, 0.0, 1.0]);
        let vd = n.visual.unwrap();
        assert!(vd.cast_shadows);
        assert_eq!(vd.transparency, 0.4);
    }

    #[test]
    fn point_and_unknown_lights() {
        let point = light_desc(&msgs::Light {
            r#type: msgs::light::LightType::Point as i32,
            diffuse: color(0.5, 0.6, 0.7),
            range: 10.0,
            cast_shadows: true,
            intensity: 3.0,
            ..Default::default()
        });
        assert_eq!(point.ty, LightType::Point);
        assert_eq!(point.color, [0.5, 0.6, 0.7]);
        assert_eq!(point.direction, None);
        assert_eq!((point.angle, point.penumbra), (None, None));
        // three.js PointLight's default decay.
        assert_eq!(point.decay, Some(1.0));
        assert_eq!(point.range, 10.0);
        assert!(point.cast_shadows);
        assert_eq!(point.msg_intensity, 3.0);
        // No attenuation: full intensity (E = D = r = 1).
        assert_eq!(point.intensity, 1.0);

        // gzweb logs "Unknown light type" and adds no light; we fall back to
        // a point light (PORT_PLAN.md §2.5 candidate).
        let unknown = light_desc(&msgs::Light {
            r#type: 7,
            ..Default::default()
        });
        assert_eq!(unknown.ty, LightType::Point);
        assert_eq!(unknown.color, [1.0; 3]);

        let directional = light_desc(&msgs::Light {
            r#type: msgs::light::LightType::Directional as i32,
            range: 99.0,
            ..Default::default()
        });
        assert_eq!(directional.range, 0.0);
        assert_eq!(directional.decay, None);
        assert_eq!(directional.direction, Some([0.0, 0.0, -1.0]));
    }

    #[test]
    fn finds_directional_lights_anywhere() {
        let light = |ty: msgs::light::LightType| msgs::Light {
            r#type: ty as i32,
            ..Default::default()
        };
        let none = msgs::Scene {
            light: vec![light(msgs::light::LightType::Point)],
            ..Default::default()
        };
        assert!(!SceneDesc::from_msg(&none, BuildOptions::default()).has_directional_light());
        // A sun on a link counts too.
        let on_link = msgs::Scene {
            model: vec![msgs::Model {
                name: "m".into(),
                link: vec![msgs::Link {
                    name: "l".into(),
                    light: vec![light(msgs::light::LightType::Directional)],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(SceneDesc::from_msg(&on_link, BuildOptions::default()).has_directional_light());
    }

    #[test]
    fn spot_penumbra_edge_cases() {
        let spot = |inner: f32, outer: f32| {
            light_desc(&msgs::Light {
                r#type: msgs::light::LightType::Spot as i32,
                spot_inner_angle: inner,
                spot_outer_angle: outer,
                ..Default::default()
            })
            .penumbra
        };
        // gzweb divides by zero here (NaN); we use 0.
        assert_eq!(spot(0.0, 0.0), Some(0.0));
        // Equal angles: a hard edge.
        assert_eq!(spot(0.5, 0.5), Some(0.0));
        // Inner > outer would be negative; clamp to 0.
        assert_eq!(spot(0.8, 0.4), Some(0.0));
        // Very wide penumbra clamps to three's maximum of 1.
        assert_eq!(spot(0.0, 1.0), Some(1.0));
    }

    #[test]
    fn light_visibility_follows_enable_lights_and_is_light_off() {
        let link_light = |off: bool| msgs::Light {
            name: if off { "off".into() } else { "on".into() },
            is_light_off: off,
            ..Default::default()
        };
        let scene = msgs::Scene {
            model: vec![msgs::Model {
                name: "m".into(),
                link: vec![msgs::Link {
                    name: "l".into(),
                    light: vec![link_light(false), link_light(true)],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            light: vec![link_light(false), link_light(true)],
            ..Default::default()
        };
        let vis = |enable_lights: bool| -> Vec<(String, bool)> {
            SceneDesc::from_msg(&scene, BuildOptions { enable_lights })
                .iter()
                .filter(|n| n.kind == NodeKind::Light)
                .map(|n| (n.path.clone(), n.visible))
                .collect()
        };
        let s = |p: &str, v: bool| (p.to_string(), v);
        assert_eq!(
            vis(true),
            [
                s("m/l/on", true),
                s("m/l/off", false),
                s("on", true),
                s("off", false)
            ]
        );
        // enableLights only affects lights attached to links.
        assert_eq!(
            vis(false),
            [
                s("m/l/on", false),
                s("m/l/off", false),
                s("on", true),
                s("off", false)
            ]
        );
        assert!(BuildOptions::default().enable_lights);
    }

    #[test]
    fn new_models_skip_known_names_and_keep_paths_unique() {
        let mut paths = PathAllocator::default();
        let initial = msgs::Scene {
            model: vec![msgs::Model {
                name: "a".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let desc = SceneDesc::from_msg_with(&initial, BuildOptions::default(), &mut paths);
        assert_eq!(desc.roots[0].path, "a");

        let update = msgs::Scene {
            model: vec![
                msgs::Model {
                    name: "a".into(),
                    ..Default::default()
                },
                msgs::Model {
                    name: "b".into(),
                    ..Default::default()
                },
                msgs::Model {
                    name: "c".into(),
                    deleted: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let known = |n: &str| n == "a";
        let nodes = SceneDesc::new_models(&update, &known, &mut paths, BuildOptions::default());
        assert_eq!(
            nodes.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        // Not known by name, but the path is taken: the allocator still
        // disambiguates.
        let again =
            SceneDesc::new_models(&update, &|n| n == "b", &mut paths, BuildOptions::default());
        assert_eq!(
            again.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(),
            ["a#1"]
        );
    }
}
