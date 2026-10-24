//! What the scene message leaves out: collisions, inertials and joints,
//! read from the SDFormat the world's `generate_world_sdf` service returns
//! (DECISIONS.md D-62). The viewport's overlays (spec §7.3: Collisions,
//! Inertia, Center of mass, Joints) draw them on the links the scene
//! message already gave us.
//!
//! The Gazebo websocket server forwards only the `gz.msgs.Scene` that
//! gz-sim's SceneBroadcaster builds (models, links, visuals, lights), so
//! the Gazebo GUI's VisualizationCapabilities, which reads the entity
//! component manager directly, has nothing to copy here. The world's SDF,
//! with included models expanded ([`generate_request`]), has all of it.
//!
//! Everything is keyed by the link's path as [`crate::scene`] builds it
//! (`model/link`, `outer/inner/link`), and every pose is in the link's own
//! frame, so a renderer attaches each overlay to the link entity it already
//! has and pose updates move them.
//!
//! Frames, as SDFormat 1.7+ defines them: a link's pose is relative to its
//! model, a collision's and an inertial's to their link, and a joint's to
//! its child link, unless `relative_to` names another link, a nested
//! model's link (`inner::link`) or `__model__`. Other frames (`<frame>`
//! elements) are not resolved: they count as the model frame.

use std::collections::HashMap;

use crate::scene::{GeometryDesc, Pose};
use crate::uri;

/// The request body for `/world/<w>/generate_world_sdf`: a
/// `gz.msgs.SdfGeneratorConfig` with `expand_include_tags` on, so models
/// included from Fuel or a model path come back in full. Encoded by hand:
/// it is six bytes, none of them a `,` or a NUL (which the websocket
/// framing can't carry).
pub fn generate_request() -> Vec<u8> {
    // global_entity_gen_config (2) { expand_include_tags (1) { data (2): true } }
    vec![0x12, 0x04, 0x0a, 0x02, 0x10, 0x01]
}

/// The service that returns the world's SDFormat.
pub fn generate_service(world: &str) -> String {
    format!("/world/{world}/generate_world_sdf")
}

/// A joint's type, as far as drawing it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointKind {
    Revolute,
    Continuous,
    Prismatic,
    Fixed,
    Ball,
    Universal,
    Revolute2,
    Screw,
    Gearbox,
    Other,
}

impl JointKind {
    fn parse(s: &str) -> JointKind {
        match s {
            "revolute" => JointKind::Revolute,
            "continuous" => JointKind::Continuous,
            "prismatic" => JointKind::Prismatic,
            "fixed" => JointKind::Fixed,
            "ball" => JointKind::Ball,
            "universal" => JointKind::Universal,
            "revolute2" => JointKind::Revolute2,
            "screw" => JointKind::Screw,
            "gearbox" => JointKind::Gearbox,
            _ => JointKind::Other,
        }
    }

    /// The type as SDFormat writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            JointKind::Revolute => "revolute",
            JointKind::Continuous => "continuous",
            JointKind::Prismatic => "prismatic",
            JointKind::Fixed => "fixed",
            JointKind::Ball => "ball",
            JointKind::Universal => "universal",
            JointKind::Revolute2 => "revolute2",
            JointKind::Screw => "screw",
            JointKind::Gearbox => "gearbox",
            JointKind::Other => "other",
        }
    }

    /// Turns about its axes (drawn with a ring), rather than sliding along
    /// them.
    pub fn rotates(self) -> bool {
        matches!(
            self,
            JointKind::Revolute
                | JointKind::Continuous
                | JointKind::Universal
                | JointKind::Revolute2
                | JointKind::Screw
                | JointKind::Gearbox
        )
    }
}

/// A collision shape, in its link's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Collision {
    pub name: String,
    pub pose: Pose,
    pub geometry: GeometryDesc,
    /// A mesh's `<scale>`; ones for everything else.
    pub scale: [f64; 3],
}

/// A link's mass properties: the centre of mass in the link's frame and
/// the inertia about it (`ixx, ixy, ixz, iyy, iyz, izz`) in the inertial
/// frame's axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inertial {
    pub pose: Pose,
    pub mass: f64,
    pub inertia: [f64; 6],
}

/// A joint, drawn on its child link: its frame in the child link's frame
/// and its axes in the joint frame (unit vectors; one, or two for
/// universal and revolute2 joints), with their limits and dynamics.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    pub name: String,
    pub kind: JointKind,
    pub pose: Pose,
    pub axes: Vec<[f64; 3]>,
    /// Per axis, its `<xyz>` as the SDFormat writes it and the frame it is
    /// expressed in (`None` for the joint's).
    pub written_axes: Vec<([f64; 3], Option<String>)>,
    /// The parent and child links as the SDFormat names them (`world`, or
    /// `inner::link` for a nested model's).
    pub parent: String,
    pub child: String,
    /// The path of the model that declares it (its child link may be a
    /// nested model's).
    pub model: String,
    /// Per axis, as `axes`.
    pub limits: Vec<AxisLimits>,
}

/// An axis's `<limit>` and `<dynamics>`, SDFormat's defaults when absent
/// (lower and upper ±1e16, effort and velocity −1: unlimited).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisLimits {
    pub lower: f64,
    pub upper: f64,
    pub effort: f64,
    pub velocity: f64,
    pub damping: f64,
    pub friction: f64,
}

impl Default for AxisLimits {
    fn default() -> Self {
        Self {
            lower: -1e16,
            upper: 1e16,
            effort: -1.0,
            velocity: -1.0,
            damping: 0.0,
            friction: 0.0,
        }
    }
}

/// A link's `<gravity>`, `<kinematic>` and `<enable_wind>` (SDFormat's
/// defaults: gravity on, the others off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkFlags {
    pub gravity: bool,
    pub kinematic: bool,
    pub wind: bool,
}

impl Default for LinkFlags {
    fn default() -> Self {
        Self {
            gravity: true,
            kinematic: false,
            wind: false,
        }
    }
}

/// A sensor's output settings (the scene message has none of them).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SensorExtras {
    pub name: String,
    pub ty: String,
    /// `<topic>` as written; Gazebo derives one when it is absent.
    pub topic: Option<String>,
    pub update_rate: f64,
    pub always_on: bool,
    pub visualize: bool,
}

/// Everything the overlays draw on one link, and what its inspector shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkExtras {
    pub collisions: Vec<Collision>,
    pub inertial: Option<Inertial>,
    /// The joints whose child this link is.
    pub joints: Vec<Joint>,
    pub flags: LinkFlags,
    pub sensors: Vec<SensorExtras>,
}

/// A model's `<static>`, `<self_collide>` and plugins.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelExtras {
    pub is_static: bool,
    pub self_collide: bool,
    /// Its `<plugin>`s' names (else file names).
    pub plugins: Vec<String>,
    /// The `<topic>` its JointStatePublisher sets, if any (else it
    /// publishes on the default topic).
    pub joint_state_topic: Option<String>,
    /// The topic its drive (DiffDrive, MecanumDrive, AckermannSteering,
    /// TrackedVehicle) takes `gz.msgs.Twist` commands on: its `<topic>`, or
    /// `/model/<name>/cmd_vel`.
    pub drive_topic: Option<String>,
    /// Its JointPositionControllers: each joint and the topic it takes
    /// `gz.msgs.Double` positions on (its plugin's `<topic>`,
    /// `/model/<name>/<sub_topic>`, or
    /// `/model/<name>/joint/<first joint>/<index>/cmd_pos`; a plugin's
    /// joints share it).
    pub joint_controllers: Vec<(String, String)>,
}

/// A plugin is one of these when its `name` ends with, or its `filename`
/// holds, the system's name.
fn plugin_is(p: Node, names: &[(&str, &str)]) -> bool {
    names.iter().any(|(class, file)| {
        p.attribute("name").is_some_and(|n| n.ends_with(class))
            || p.attribute("filename").is_some_and(|f| f.contains(file))
    })
}

/// A topic as gz-transport reads one: rooted.
fn rooted(topic: &str) -> String {
    if topic.starts_with('/') {
        topic.to_string()
    } else {
        format!("/{topic}")
    }
}

/// The drive plugins, as (class, file name part).
const DRIVES: &[(&str, &str)] = &[
    ("DiffDrive", "diff-drive"),
    ("MecanumDrive", "mecanum-drive"),
    ("AckermannSteering", "ackermann-steering"),
    ("TrackedVehicle", "tracked-vehicle"),
];

/// A world's links' and models' extras by path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldExtras {
    pub links: HashMap<String, LinkExtras>,
    pub models: HashMap<String, ModelExtras>,
}

impl WorldExtras {
    /// Parses a `generate_world_sdf` reply (or any SDFormat world or model
    /// file). Models' top-level names are their paths, as in the scene
    /// message.
    pub fn parse(xml: &str) -> Result<WorldExtras, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        if root.tag_name().name() != "sdf" {
            return Err(format!("not SDFormat: <{}>", root.tag_name().name()));
        }
        let mut out = WorldExtras::default();
        for top in elements(root) {
            match top.tag_name().name() {
                "world" => {
                    for model in children(top, "model") {
                        read_model(model, "", &mut out);
                    }
                }
                "model" => read_model(top, "", &mut out),
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn link(&self, path: &str) -> Option<&LinkExtras> {
        self.links.get(path)
    }

    pub fn model(&self, path: &str) -> Option<&ModelExtras> {
        self.models.get(path)
    }

    /// The joint `name` that model `model` declares (not one of its nested
    /// models'; joints live on their child links, under the model's path).
    pub fn joint(&self, model: &str, name: &str) -> Option<&Joint> {
        let prefix = format!("{model}/");
        self.links
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .flat_map(|(_, l)| &l.joints)
            .find(|j| j.model == model && j.name == name)
    }

    /// The joints model `model` declares.
    pub fn joints_of<'a>(&'a self, model: &'a str) -> impl Iterator<Item = &'a Joint> + 'a {
        let prefix = format!("{model}/");
        self.links
            .iter()
            .filter(move |(path, _)| path.starts_with(&prefix))
            .flat_map(|(_, l)| &l.joints)
            .filter(move |j| j.model == model)
    }
}

type Node<'a, 'i> = roxmltree::Node<'a, 'i>;

fn elements<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(Node::is_element)
}

fn children<'a, 'i>(n: Node<'a, 'i>, tag: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    elements(n).filter(move |c| c.tag_name().name() == tag)
}

fn child<'a, 'i>(n: Node<'a, 'i>, tag: &'static str) -> Option<Node<'a, 'i>> {
    children(n, tag).next()
}

fn text<'a>(n: Node<'a, '_>, tag: &'static str) -> Option<&'a str> {
    child(n, tag).and_then(|c| c.text()).map(str::trim)
}

fn number(n: Node, tag: &'static str) -> Option<f64> {
    text(n, tag).and_then(|t| t.parse().ok())
}

fn numbers(s: &str) -> Vec<f64> {
    s.split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn flag(n: Node, tag: &'static str, default: bool) -> bool {
    match text(n, tag) {
        Some(t) if t == "1" || t.eq_ignore_ascii_case("true") => true,
        Some(t) if t == "0" || t.eq_ignore_ascii_case("false") => false,
        _ => default,
    }
}

fn vec3(n: Node, tag: &'static str, default: [f64; 3]) -> [f64; 3] {
    match text(n, tag).map(numbers).as_deref() {
        Some([x, y, z]) => [*x, *y, *z],
        _ => default,
    }
}

/// A `<pose>`'s value and `relative_to`. Euler angles are radians unless
/// `degrees="true"`; `rotation_format="quat_xyzw"` gives a quaternion.
fn pose_of(n: Node) -> (Pose, Option<String>) {
    let Some(p) = child(n, "pose") else {
        return (Pose::IDENTITY, None);
    };
    let relative_to = p
        .attribute("relative_to")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let v = numbers(p.text().unwrap_or(""));
    let degrees = p.attribute("degrees") == Some("true");
    let quat_format = p.attribute("rotation_format") == Some("quat_xyzw");
    let pose = match (v.as_slice(), quat_format) {
        ([x, y, z, qx, qy, qz, qw], true) => Pose {
            pos: [*x, *y, *z],
            quat: normalize([*qx, *qy, *qz, *qw]),
        },
        ([x, y, z, r, pi, ya], false) => {
            let k = if degrees { 1f64.to_radians() } else { 1.0 };
            Pose {
                pos: [*x, *y, *z],
                quat: rpy(*r * k, *pi * k, *ya * k),
            }
        }
        _ => Pose::IDENTITY,
    };
    (pose, relative_to)
}

fn read_model(model: Node, parent: &str, out: &mut WorldExtras) {
    let name = model.attribute("name").unwrap_or("");
    let path = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    };
    let links: Vec<Node> = children(model, "link").collect();
    let in_model = |frame: &str| frame_in_model(model, frame, 0);
    out.models.insert(
        path.clone(),
        ModelExtras {
            is_static: flag(model, "static", false),
            self_collide: flag(model, "self_collide", false),
            plugins: children(model, "plugin")
                .filter_map(|p| p.attribute("name").or(p.attribute("filename")))
                .map(str::to_string)
                .collect(),
            joint_state_topic: children(model, "plugin")
                .filter(|p| {
                    p.attribute("name")
                        .is_some_and(|n| n.ends_with("JointStatePublisher"))
                        || p.attribute("filename")
                            .is_some_and(|f| f.contains("joint-state-publisher"))
                })
                .find_map(|p| text(p, "topic"))
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            drive_topic: children(model, "plugin")
                .find(|p| plugin_is(*p, DRIVES))
                .map(|p| match text(p, "topic").filter(|t| !t.is_empty()) {
                    Some(t) => rooted(t),
                    None => format!("/model/{name}/cmd_vel"),
                }),
            joint_controllers: children(model, "plugin")
                .filter(|p| {
                    plugin_is(
                        *p,
                        &[("JointPositionController", "joint-position-controller")],
                    )
                })
                // One topic per plugin, as gz-sim names it: `<topic>`, else
                // `/model/<model>/<sub_topic>`, else from the first joint;
                // it moves all the plugin's joints. One that takes
                // `gz.msgs.Actuators` (`use_actuator_msg`) isn't commanded.
                .filter(|p| !text(*p, "use_actuator_msg").is_some_and(|v| v.trim() == "true"))
                .flat_map(|p| {
                    let joints: Vec<&str> = children(p, "joint_name")
                        .filter_map(|j| j.text())
                        .map(str::trim)
                        .filter(|j| !j.is_empty())
                        .collect();
                    let index = text(p, "joint_index").unwrap_or("0");
                    let topic = text(p, "topic")
                        .filter(|t| !t.is_empty())
                        .map(rooted)
                        .or_else(|| {
                            text(p, "sub_topic")
                                .filter(|t| !t.is_empty())
                                .map(|t| format!("/model/{name}/{}", t.trim_start_matches('/')))
                        })
                        .or_else(|| {
                            let first = joints.first()?;
                            Some(format!("/model/{name}/joint/{first}/{index}/cmd_pos"))
                        });
                    joints
                        .iter()
                        .filter_map(|j| Some((j.to_string(), topic.clone()?)))
                        .collect::<Vec<_>>()
                })
                .collect(),
        },
    );
    // A pose that belongs to `owner` (a link), in that link's frame.
    let in_link =
        |owner: &str, (pose, relative_to): (Pose, Option<String>)| match relative_to.as_deref() {
            None => pose,
            Some(r) if r == owner => pose,
            Some(r) => mul(inverse(in_model(owner)), mul(in_model(r), pose)),
        };

    for link in &links {
        let lname = link.attribute("name").unwrap_or("");
        let extras = out.links.entry(format!("{path}/{lname}")).or_default();
        for c in children(*link, "collision") {
            if let Some(geometry) = child(c, "geometry").and_then(geometry) {
                extras.collisions.push(Collision {
                    name: c.attribute("name").unwrap_or("").to_string(),
                    pose: in_link(lname, pose_of(c)),
                    scale: mesh_scale(c),
                    geometry,
                });
            }
        }
        if let Some(i) = child(*link, "inertial") {
            extras.inertial = Some(Inertial {
                pose: in_link(lname, pose_of(i)),
                ..inertial(i)
            });
        }
        extras.flags = LinkFlags {
            gravity: flag(*link, "gravity", true),
            kinematic: flag(*link, "kinematic", false),
            wind: flag(*link, "enable_wind", false),
        };
        extras.sensors = children(*link, "sensor")
            .map(|s| SensorExtras {
                name: s.attribute("name").unwrap_or("").to_string(),
                ty: s.attribute("type").unwrap_or("").to_string(),
                // SDFormat's placeholder for "derive one" counts as none.
                topic: text(s, "topic")
                    .filter(|t| !t.is_empty() && *t != "__default__")
                    .map(str::to_string),
                update_rate: number(s, "update_rate").unwrap_or(0.0),
                always_on: flag(s, "always_on", false),
                visualize: flag(s, "visualize", false),
            })
            .collect();
    }

    for j in children(model, "joint") {
        let Some(child_name) = text(j, "child") else {
            continue;
        };
        // A nested model's link: `inner::link` is `path/inner/link`.
        let child_path = format!("{path}/{}", child_name.replace("::", "/"));
        let pose = in_link(child_name, pose_of(j));
        let written_axes: Vec<([f64; 3], Option<String>)> = ["axis", "axis2"]
            .into_iter()
            .filter_map(|tag| child(j, tag))
            .map(|a| {
                let xyz = child(a, "xyz");
                let v = xyz
                    .and_then(|x| x.text())
                    .map(numbers)
                    .and_then(|v| <[f64; 3]>::try_from(v).ok())
                    .unwrap_or([0.0, 0.0, 1.0]);
                let frame = xyz
                    .and_then(|x| x.attribute("expressed_in"))
                    .filter(|f| !f.is_empty())
                    .map(str::to_string);
                (v, frame)
            })
            .collect();
        let axes = written_axes
            .iter()
            .map(|(v, frame)| {
                // An axis expressed in the model frame turns into the
                // joint frame: undo the joint's rotation in the model.
                let v = if frame.as_deref() == Some("__model__") {
                    let joint_in_model = mul(in_model(child_name), pose);
                    rotate(inverse(joint_in_model).quat, *v)
                } else {
                    *v
                };
                unit(v)
            })
            .collect();
        let limits = ["axis", "axis2"]
            .into_iter()
            .filter_map(|tag| child(j, tag))
            .map(|a| {
                let d = AxisLimits::default();
                let limit = child(a, "limit");
                let dynamics = child(a, "dynamics");
                let of = |n: Option<Node>, tag, default| {
                    n.and_then(|n| number(n, tag)).unwrap_or(default)
                };
                AxisLimits {
                    lower: of(limit, "lower", d.lower),
                    upper: of(limit, "upper", d.upper),
                    effort: of(limit, "effort", d.effort),
                    velocity: of(limit, "velocity", d.velocity),
                    damping: of(dynamics, "damping", d.damping),
                    friction: of(dynamics, "friction", d.friction),
                }
            })
            .collect();
        out.links.entry(child_path).or_default().joints.push(Joint {
            name: j.attribute("name").unwrap_or("").to_string(),
            kind: JointKind::parse(j.attribute("type").unwrap_or("")),
            pose,
            axes,
            written_axes,
            parent: text(j, "parent").unwrap_or("").to_string(),
            child: child_name.to_string(),
            model: path.clone(),
            limits,
        });
    }

    for nested in children(model, "model") {
        read_model(nested, &path, out);
    }
}

/// A frame's pose in `model`'s frame: `__model__`, one of its links, or a
/// nested model's link (`inner::link`, `a::b::link`), following
/// `relative_to` through the others (at most a few deep; a loop or an
/// unknown frame stops at the model frame).
fn frame_in_model(model: Node, frame: &str, depth: u32) -> Pose {
    if depth >= 16 || frame == "__model__" {
        return Pose::IDENTITY;
    }
    let named = |tag| children(model, tag).find(|n| n.attribute("name") == Some(frame));
    let (element, inner) = match frame.split_once("::") {
        Some((nested, rest)) => {
            let Some(m) = children(model, "model").find(|n| n.attribute("name") == Some(nested))
            else {
                return Pose::IDENTITY;
            };
            (m, Some(rest))
        }
        None => match named("link") {
            Some(l) => (l, None),
            None => return Pose::IDENTITY,
        },
    };
    let (pose, relative_to) = pose_of(element);
    let pose = match relative_to.as_deref() {
        Some(r) if r != "__model__" => mul(frame_in_model(model, r, depth + 1), pose),
        _ => pose,
    };
    match inner {
        Some(rest) => mul(pose, frame_in_model(element, rest, depth + 1)),
        None => pose,
    }
}

fn mesh_scale(collision: Node) -> [f64; 3] {
    child(collision, "geometry")
        .and_then(|g| child(g, "mesh"))
        .map_or([1.0; 3], |m| vec3(m, "scale", [1.0; 3]))
}

fn geometry(g: Node) -> Option<GeometryDesc> {
    let shape = elements(g).next()?;
    let num = |tag, d| number(shape, tag).unwrap_or(d);
    Some(match shape.tag_name().name() {
        "box" => GeometryDesc::Box {
            size: vec3(shape, "size", [1.0; 3]),
        },
        "sphere" => GeometryDesc::Sphere {
            radius: num("radius", 1.0),
        },
        "cylinder" => GeometryDesc::Cylinder {
            radius: num("radius", 1.0),
            length: num("length", 1.0),
        },
        "capsule" => GeometryDesc::Capsule {
            radius: num("radius", 0.5),
            length: num("length", 1.0),
        },
        "cone" => GeometryDesc::Cone {
            radius: num("radius", 1.0),
            length: num("length", 1.0),
        },
        "ellipsoid" => GeometryDesc::Ellipsoid {
            radii: vec3(shape, "radii", [1.0; 3]),
        },
        "plane" => GeometryDesc::Plane {
            normal: vec3(shape, "normal", [0.0, 0.0, 1.0]),
            size: match text(shape, "size").map(numbers).as_deref() {
                Some([x, y]) => [*x, *y],
                _ => [1.0, 1.0],
            },
        },
        "mesh" => {
            let u = text(shape, "uri")?.to_string();
            let sub = child(shape, "submesh");
            GeometryDesc::Mesh {
                load_uri: uri::mesh_load_uri(&u),
                uri: u,
                submesh: sub.and_then(|s| text(s, "name")).map(str::to_string),
                center_submesh: sub.and_then(|s| text(s, "center")) == Some("true"),
            }
        }
        other => GeometryDesc::Unsupported(other.to_string()),
    })
}

fn inertial(i: Node) -> Inertial {
    let m = child(i, "inertia");
    let get = |tag, d| m.and_then(|m| number(m, tag)).unwrap_or(d);
    Inertial {
        pose: pose_of(i).0,
        mass: number(i, "mass").unwrap_or(1.0),
        inertia: [
            get("ixx", 1.0),
            get("ixy", 0.0),
            get("ixz", 0.0),
            get("iyy", 1.0),
            get("iyz", 0.0),
            get("izz", 1.0),
        ],
    }
}

// Pose maths in f64 (quaternions `[x, y, z, w]`).

fn normalize(q: [f64; 4]) -> [f64; 4] {
    let n = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if n > 0.0 {
        q.map(|c| c / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = v.iter().map(|c| c * c).sum::<f64>().sqrt();
    if n > 0.0 {
        v.map(|c| c / n)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Gazebo's roll-pitch-yaw: about fixed X, then Y, then Z.
fn rpy(r: f64, p: f64, y: f64) -> [f64; 4] {
    let (sr, cr) = (r / 2.0).sin_cos();
    let (sp, cp) = (p / 2.0).sin_cos();
    let (sy, cy) = (y / 2.0).sin_cos();
    [
        sr * cp * cy - cr * sp * sy,
        cr * sp * cy + sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let p = qmul(
        qmul(q, [v[0], v[1], v[2], 0.0]),
        [-q[0], -q[1], -q[2], q[3]],
    );
    [p[0], p[1], p[2]]
}

/// `a` then `b` in `a`'s frame: `b` is relative to `a`.
fn mul(a: Pose, b: Pose) -> Pose {
    let t = rotate(a.quat, b.pos);
    Pose {
        pos: [a.pos[0] + t[0], a.pos[1] + t[1], a.pos[2] + t[2]],
        quat: normalize(qmul(a.quat, b.quat)),
    }
}

fn inverse(p: Pose) -> Pose {
    let q = [-p.quat[0], -p.quat[1], -p.quat[2], p.quat[3]];
    let t = rotate(q, p.pos);
    Pose {
        pos: [-t[0], -t[1], -t[2]],
        quat: q,
    }
}

#[cfg(test)]
mod tests;
