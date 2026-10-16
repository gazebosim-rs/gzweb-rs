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
    pub children: Vec<Node>,
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
    Box { size: [f64; 3] },
    /// Axis along +Z.
    Cylinder { radius: f64, length: f64 },
    /// Axis along +Z.
    Capsule { radius: f64, length: f64 },
    /// Axis along +Z, apex at +Z.
    Cone { radius: f64, length: f64 },
    Ellipsoid { radii: [f64; 3] },
    Sphere { radius: f64 },
    /// Plane through the origin; `size` is along local X/Y before rotating
    /// +Z onto `normal` with the shortest-arc rotation (gzweb `createPlane`).
    Plane { normal: [f64; 3], size: [f64; 2] },
    Mesh {
        /// The URI from the message.
        uri: String,
        /// What to load (see [`uri::mesh_load_uri`]).
        load_uri: String,
        submesh: Option<String>,
        center_submesh: bool,
    },
    /// Recognised but not rendered yet.
    Unsupported(String),
}

/// Material in three.js terms, as gzweb `createMaterial` + `setMaterial`
/// would build it, plus the extra message fields a PBR renderer can use.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDesc {
    /// gzweb colour rule: `0.4 * ambient + 0.6 * diffuse` when both are set,
    /// else `diffuse`, else white.
    pub color: [f32; 3],
    pub diffuse_alpha: f32,
    pub emissive: [f32; 3],
    /// `Some` when the message carries a PBR block.
    pub pbr: Option<PbrDesc>,
    pub double_sided: bool,
    /// Whether the message had a material at all.
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PbrDesc {
    pub metalness: f64,
    pub roughness: f64,
    pub albedo_map: Option<String>,
    pub normal_map: Option<String>,
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
    /// `Some(uri)` when the scene has a sky; the URI is the `cubemap_uri`
    /// header value, if any.
    pub sky: Option<Option<String>>,
    pub grid: bool,
    pub shadows: bool,
    pub roots: Vec<Node>,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    /// gzweb `enableLights`: lights attached to links start visible.
    pub enable_lights: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self { enable_lights: true }
    }
}

impl SceneDesc {
    pub fn from_msg(scene: &msgs::Scene, opts: BuildOptions) -> SceneDesc {
        Self::from_msg_with(scene, opts, &mut PathAllocator::default())
    }

    /// Like [`SceneDesc::from_msg`], allocating paths from `paths` so later
    /// additions (`scene/info`) stay unique.
    pub fn from_msg_with(scene: &msgs::Scene, opts: BuildOptions, paths: &mut PathAllocator) -> SceneDesc {
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
            sky.header.as_ref().and_then(|h| {
                h.data
                    .iter()
                    .find(|d| d.key == "cubemap_uri")
                    .and_then(|d| d.value.first().cloned())
            })
        });

        SceneDesc {
            name: scene.name.clone(),
            ambient: scene.ambient.as_ref().map(|c| [c.r, c.g, c.b]),
            background: scene.background.as_ref().map(|c| [c.r, c.g, c.b, c.a]),
            sky,
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
        children,
    })
}

fn link_node(link: &msgs::Link, parent: &str, paths: &mut PathAllocator, opts: BuildOptions) -> Node {
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
        children,
    }
}

fn visual_node(visual: &msgs::Visual, parent: &str, paths: &mut PathAllocator) -> Option<Node> {
    if visual.delete_me {
        return None;
    }
    let path = paths.alloc(parent, &visual.name);
    let geometry = visual.geometry.as_ref();
    let geom = geometry.map(geometry_desc).unwrap_or(GeometryDesc::Unsupported("none".into()));

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
        children: Vec::new(),
    })
}

/// Dispatches on which geometry sub-message is present, in gzweb's order
/// (`SDFParser.createGeom`), not on `Geometry.type`.
fn geometry_desc(g: &msgs::Geometry) -> GeometryDesc {
    let v3 = |v: &Option<msgs::Vector3d>, d: [f64; 3]| v.as_ref().map(|v| [v.x, v.y, v.z]).unwrap_or(d);
    if let Some(b) = &g.r#box {
        GeometryDesc::Box { size: v3(&b.size, [1.0; 3]) }
    } else if let Some(c) = &g.cylinder {
        GeometryDesc::Cylinder { radius: c.radius, length: c.length }
    } else if let Some(c) = &g.capsule {
        GeometryDesc::Capsule { radius: c.radius, length: c.length }
    } else if let Some(c) = &g.cone {
        GeometryDesc::Cone { radius: c.radius, length: c.length }
    } else if let Some(e) = &g.ellipsoid {
        GeometryDesc::Ellipsoid { radii: v3(&e.radii, [1.0; 3]) }
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
    } else if g.heightmap.is_some() {
        GeometryDesc::Unsupported("heightmap".into())
    } else {
        GeometryDesc::Unsupported(
            msgs::geometry::Type::try_from(g.r#type)
                .map(|t| t.as_str_name().to_ascii_lowercase())
                .unwrap_or_else(|_| "unknown".into()),
        )
    }
}

fn material_desc(mat: Option<&msgs::Material>) -> MaterialDesc {
    let Some(mat) = mat else {
        // gzweb leaves its shared white Phong material in place.
        return MaterialDesc {
            color: [1.0; 3],
            diffuse_alpha: 1.0,
            emissive: [0.0; 3],
            pbr: None,
            double_sided: false,
            present: false,
        };
    };
    let rgb = |c: &msgs::Color| [c.r, c.g, c.b];
    let color = match (&mat.ambient, &mat.diffuse) {
        (Some(a), Some(d)) => [
            a.r * 0.4 + d.r * 0.6,
            a.g * 0.4 + d.g * 0.6,
            a.b * 0.4 + d.b * 0.6,
        ],
        (None, Some(d)) => rgb(d),
        _ => [1.0; 3],
    };
    let non_empty = |s: &String| (!s.is_empty()).then(|| s.clone());
    MaterialDesc {
        color,
        diffuse_alpha: mat.diffuse.as_ref().map(|d| d.a).unwrap_or(1.0),
        emissive: mat.emissive.as_ref().map(rgb).unwrap_or([0.0; 3]),
        pbr: mat.pbr.as_ref().map(|p| PbrDesc {
            metalness: p.metalness,
            roughness: p.roughness,
            albedo_map: non_empty(&p.albedo_map),
            normal_map: non_empty(&p.normal_map),
        }),
        double_sided: mat.double_sided,
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

    let (angle, penumbra, decay) = match ty {
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
            (Some(outer), Some(penumbra), Some(light.spot_falloff as f64))
        }
        // three.js PointLight's default decay.
        LightType::Point => (None, None, Some(1.0)),
        LightType::Directional => (None, None, None),
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
        Some(msgs::Vector3d { x, y, z, ..Default::default() })
    }

    fn color(r: f32, g: f32, b: f32) -> Option<msgs::Color> {
        Some(msgs::Color { r, g, b, a: 1.0, ..Default::default() })
    }

    fn box_visual(name: &str) -> msgs::Visual {
        msgs::Visual {
            name: name.into(),
            geometry: Some(msgs::Geometry {
                r#type: msgs::geometry::Type::Box as i32,
                r#box: Some(msgs::BoxGeom { size: v3(1.0, 2.0, 3.0), ..Default::default() }),
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
                pose: Some(msgs::Pose { position: v3(1.0, 2.0, 3.0), ..Default::default() }),
                link: vec![msgs::Link {
                    name: "link".into(),
                    visual: vec![box_visual("v"), box_visual("v")],
                    ..Default::default()
                }],
                model: vec![msgs::Model { name: "inner".into(), ..Default::default() }],
                ..Default::default()
            }],
            light: vec![msgs::Light { name: "sun".into(), ..Default::default() }],
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
        assert_eq!(
            desc.roots[0].children[0].children[0].visual.as_ref().unwrap().geometry,
            GeometryDesc::Box { size: [1.0, 2.0, 3.0] }
        );
    }

    #[test]
    fn material_colour_rule() {
        let m = msgs::Material {
            ambient: color(1.0, 0.0, 0.0),
            diffuse: color(0.0, 1.0, 0.0),
            ..Default::default()
        };
        let d = material_desc(Some(&m));
        assert!((d.color[0] - 0.4).abs() < 1e-6 && (d.color[1] - 0.6).abs() < 1e-6);
        assert_eq!(material_desc(None).color, [1.0; 3]);
        let only_diffuse = msgs::Material { diffuse: color(0.2, 0.3, 0.4), ..Default::default() };
        assert_eq!(material_desc(Some(&only_diffuse)).color, [0.2, 0.3, 0.4]);
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
            sphere: Some(msgs::SphereGeom { radius: 2.0, ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(geometry_desc(&g), GeometryDesc::Box { size: [1.0; 3] });

        let plane = msgs::Geometry { plane: Some(Default::default()), ..Default::default() };
        assert_eq!(
            geometry_desc(&plane),
            GeometryDesc::Plane { normal: [0.0, 0.0, 1.0], size: [1.0, 1.0] }
        );
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
        match &n.visual.unwrap().geometry {
            GeometryDesc::Mesh { load_uri, .. } => assert_eq!(load_uri, "m/meshes/a.dae"),
            g => panic!("{g:?}"),
        }
    }
}
