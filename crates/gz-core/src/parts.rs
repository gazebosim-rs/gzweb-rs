//! Parts added to a model while it is edited (spec §6.6; DECISIONS.md
//! D-113): links, joints, visuals, collisions, sensors and plugins, as
//! SDFormat elements, and the [`Draft`] that holds them in order, with its
//! own undo. Done writes them into the model's SDFormat ([`write_parts`],
//! D-114) once [`check_model`] finds it whole.
//!
//! What a part shows before then comes from its own SDFormat: a part's
//! [`WorldExtras`] (its collisions, inertial, joint, sensor or plugin, as
//! the overlays, the tree and the inspectors read them) is its element
//! parsed, so what is drawn is what will be written.

mod write;

pub use write::{check_model, write_parts};

use crate::insert::{xml_escape, Shape};
use crate::scene::{GeometryDesc, MaterialDesc, Pose, SensorDesc, VisualDesc};
use crate::sdf::{JointKind, WorldExtras};
use crate::world_file::{number, sdf_pose};

/// How big (tall, and across) a new link's shape is, in metres.
pub const LINK_SIZE: f64 = 0.5;

/// How big a visual or collision added to a link is.
pub const PART_SIZE: f64 = 0.2;

/// A new link's mass, in kilograms.
pub const LINK_MASS: f64 = 1.0;

/// The shapes a link, visual or collision can have (a plane can't be
/// one).
pub const PART_SHAPES: [Shape; 5] = [
    Shape::Box,
    Shape::Sphere,
    Shape::Cylinder,
    Shape::Capsule,
    Shape::Cone,
];

/// The joints the Parts tab adds.
pub const JOINT_KINDS: [JointKind; 5] = [
    JointKind::Revolute,
    JointKind::Continuous,
    JointKind::Prismatic,
    JointKind::Fixed,
    JointKind::Ball,
];

/// What a joint is called in the Parts tab.
pub fn joint_label(kind: JointKind) -> &'static str {
    match kind {
        JointKind::Revolute => "Revolute",
        JointKind::Continuous => "Continuous",
        JointKind::Prismatic => "Prismatic",
        JointKind::Fixed => "Fixed",
        JointKind::Ball => "Ball",
        JointKind::Universal => "Universal",
        JointKind::Revolute2 => "Revolute 2",
        JointKind::Screw => "Screw",
        JointKind::Gearbox => "Gearbox",
        JointKind::Other => "Other",
    }
}

/// A sensor the Parts tab adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SensorKind {
    Camera,
    DepthCamera,
    GpuLidar,
    Imu,
    Altimeter,
    Magnetometer,
    AirPressure,
    NavSat,
}

impl SensorKind {
    pub const ALL: [SensorKind; 8] = [
        SensorKind::Camera,
        SensorKind::DepthCamera,
        SensorKind::GpuLidar,
        SensorKind::Imu,
        SensorKind::Altimeter,
        SensorKind::Magnetometer,
        SensorKind::AirPressure,
        SensorKind::NavSat,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SensorKind::Camera => "Camera",
            SensorKind::DepthCamera => "Depth camera",
            SensorKind::GpuLidar => "GPU lidar",
            SensorKind::Imu => "IMU",
            SensorKind::Altimeter => "Altimeter",
            SensorKind::Magnetometer => "Magnetometer",
            SensorKind::AirPressure => "Air pressure",
            SensorKind::NavSat => "NavSat",
        }
    }

    /// Its `type` as SDFormat writes it.
    pub fn ty(self) -> &'static str {
        match self {
            SensorKind::Camera => "camera",
            SensorKind::DepthCamera => "depth_camera",
            SensorKind::GpuLidar => "gpu_lidar",
            SensorKind::Imu => "imu",
            SensorKind::Altimeter => "altimeter",
            SensorKind::Magnetometer => "magnetometer",
            SensorKind::AirPressure => "air_pressure",
            SensorKind::NavSat => "navsat",
        }
    }

    /// How often it updates, in Hz.
    fn rate(self) -> f64 {
        match self {
            SensorKind::GpuLidar => 10.0,
            SensorKind::Imu => 100.0,
            SensorKind::NavSat => 1.0,
            SensorKind::Camera
            | SensorKind::DepthCamera
            | SensorKind::Altimeter
            | SensorKind::Magnetometer
            | SensorKind::AirPressure => 30.0,
        }
    }

    /// Its type's own element: a 320×240 camera with a 60° view, a
    /// 640-ray lidar out to 10 m; the others need none.
    fn body(self) -> &'static str {
        match self {
            SensorKind::Camera | SensorKind::DepthCamera => {
                "<camera><horizontal_fov>1.047</horizontal_fov><image><width>320</width>\
                 <height>240</height></image><clip><near>0.1</near><far>100</far></clip>\
                 </camera>"
            }
            SensorKind::GpuLidar => {
                "<lidar><scan><horizontal><samples>640</samples><resolution>1</resolution>\
                 <min_angle>-1.396263</min_angle><max_angle>1.396263</max_angle></horizontal>\
                 <vertical><samples>1</samples><resolution>1</resolution><min_angle>0\
                 </min_angle><max_angle>0</max_angle></vertical></scan><range><min>0.08</min>\
                 <max>10</max><resolution>0.01</resolution></range></lidar>"
            }
            _ => "",
        }
    }
}

/// A part added to the model being edited. Links are named by their
/// scene path (`model/link`, `model/inner/link`); a pose is in the frame
/// of what the part goes on: the model for a link, the link for the rest
/// (a joint's in its child link's).
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Link {
        name: String,
        shape: Shape,
        pose: Pose,
    },
    Visual {
        link: String,
        name: String,
        shape: Shape,
        pose: Pose,
    },
    Collision {
        link: String,
        name: String,
        shape: Shape,
        pose: Pose,
    },
    Sensor {
        link: String,
        name: String,
        kind: SensorKind,
        pose: Pose,
    },
    Joint {
        name: String,
        kind: JointKind,
        parent: String,
        child: String,
    },
    Plugin {
        name: String,
        filename: String,
    },
    /// Joint `joint` of model `model` (a scene path: the edited model or
    /// one nested in it) takes `parent` as its parent link (its child
    /// stays `child`). Not an element: it changes its joint's.
    Reparent {
        joint: String,
        model: String,
        child: String,
        parent: String,
    },
}

/// `size` of a shape: its SDFormat geometry, how it draws, and its
/// inertia's diagonal for `mass` about its centre.
pub fn shape_geometry(shape: Shape, size: f64, mass: f64) -> (String, GeometryDesc, [f64; 3]) {
    let (r, l) = (size / 2.0, size);
    let n = number;
    match shape {
        Shape::Box | Shape::Plane => {
            let i = mass * size * size / 6.0;
            (
                format!("<box><size>{} {} {}</size></box>", n(l), n(l), n(l)),
                GeometryDesc::Box { size: [l; 3] },
                [i; 3],
            )
        }
        Shape::Sphere => {
            let i = 0.4 * mass * r * r;
            (
                format!("<sphere><radius>{}</radius></sphere>", n(r)),
                GeometryDesc::Sphere { radius: r },
                [i; 3],
            )
        }
        Shape::Cylinder => {
            let side = mass * (3.0 * r * r + l * l) / 12.0;
            (
                format!(
                    "<cylinder><radius>{}</radius><length>{}</length></cylinder>",
                    n(r),
                    n(l)
                ),
                GeometryDesc::Cylinder {
                    radius: r,
                    length: l,
                },
                [side, side, mass * r * r / 2.0],
            )
        }
        Shape::Capsule => {
            // As the Shapes tab's: the caps a fifth of its height each. The
            // mass splits by volume between the cylinder and the two
            // hemispheres, each about its own centre, carried out to the
            // capsule's.
            let (r, l) = (0.2 * size, 0.6 * size);
            let (cylinder, caps) = (l, 4.0 / 3.0 * r);
            let mc = mass * cylinder / (cylinder + caps);
            let mh = mass - mc;
            let side = mc * (l * l / 12.0 + r * r / 4.0)
                + mh * (0.4 * r * r + l * l / 4.0 + 3.0 * l * r / 8.0);
            (
                format!(
                    "<capsule><radius>{}</radius><length>{}</length></capsule>",
                    n(r),
                    n(l)
                ),
                GeometryDesc::Capsule {
                    radius: r,
                    length: l,
                },
                [side, side, mc * r * r / 2.0 + 0.4 * mh * r * r],
            )
        }
        Shape::Cone => {
            let side = 3.0 / 80.0 * mass * (4.0 * r * r + l * l);
            (
                format!(
                    "<cone><radius>{}</radius><length>{}</length></cone>",
                    n(r),
                    n(l)
                ),
                GeometryDesc::Cone {
                    radius: r,
                    length: l,
                },
                [side, side, 0.3 * mass * r * r],
            )
        }
    }
}

/// How far above its centre a `size` shape's centre of mass is, when it
/// isn't at it: a cone's is a quarter of its height below.
pub fn shape_centroid(shape: Shape, size: f64) -> Option<f64> {
    (shape == Shape::Cone).then_some(-size / 4.0)
}

/// The grey the Shapes tab's models are.
const GREY: &str = "<material><ambient>0.3 0.3 0.3 1</ambient><diffuse>0.7 0.7 0.7 1</diffuse>\
                    <specular>1 1 1 1</specular></material>";

/// How a part's shape draws: the Shapes tab's grey.
pub fn shape_visual(shape: Shape, size: f64) -> VisualDesc {
    let (_, geometry, _) = shape_geometry(shape, size, 1.0);
    VisualDesc {
        proto_type: geometry.kind_name().to_string(),
        geometry,
        material: MaterialDesc {
            diffuse: Some([0.7, 0.7, 0.7, 1.0]),
            ambient: Some([0.3, 0.3, 0.3, 1.0]),
            specular: Some([1.0, 1.0, 1.0]),
            present: true,
            ..MaterialDesc::absent()
        },
        cast_shadows: true,
        transparency: 0.0,
    }
}

/// A link's name in its model's SDFormat, from its scene path: `base`
/// for `model/base`, `inner::wheel` for `model/inner/wheel`.
pub fn sdf_link_name(model: &str, path: &str) -> String {
    path.strip_prefix(model)
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(path)
        .replace('/', "::")
}

/// What becomes of the last part of a path: its name.
fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Part {
    pub fn name(&self) -> &str {
        match self {
            Part::Link { name, .. }
            | Part::Visual { name, .. }
            | Part::Collision { name, .. }
            | Part::Sensor { name, .. }
            | Part::Joint { name, .. }
            | Part::Plugin { name, .. }
            | Part::Reparent { joint: name, .. } => name,
        }
    }

    /// What adding it does, as undo names it: "add", "reparent".
    pub fn verb(&self) -> &'static str {
        match self {
            Part::Reparent { .. } => "reparent",
            _ => "add",
        }
    }

    /// What it is, as undo names it: "link", "visual", …
    pub fn noun(&self) -> &'static str {
        match self {
            Part::Link { .. } => "link",
            Part::Visual { .. } => "visual",
            Part::Collision { .. } => "collision",
            Part::Sensor { .. } => "sensor",
            Part::Joint { .. } | Part::Reparent { .. } => "joint",
            Part::Plugin { .. } => "plugin",
        }
    }

    /// The link it goes on (a joint's child), by scene path; none for a
    /// link or a plugin.
    pub fn link(&self) -> Option<&str> {
        match self {
            Part::Visual { link, .. }
            | Part::Collision { link, .. }
            | Part::Sensor { link, .. } => Some(link),
            Part::Joint { child, .. } | Part::Reparent { child, .. } => Some(child),
            Part::Link { .. } | Part::Plugin { .. } => None,
        }
    }

    /// Its SDFormat element, in model `model` (a scene path): a link, a
    /// visual, collision or sensor for its link, a joint or a plugin for
    /// the model; none for a reparent (it changes its joint's `<parent>`).
    pub fn sdf(&self, model: &str) -> String {
        match self {
            Part::Link { name, shape, pose } => {
                let (geometry, _, [ixx, iyy, izz]) = shape_geometry(*shape, LINK_SIZE, LINK_MASS);
                let n = number;
                let centroid = shape_centroid(*shape, LINK_SIZE)
                    .map(|z| format!("<pose>0 0 {} 0 0 0</pose>", n(z)))
                    .unwrap_or_default();
                format!(
                    "<link name=\"{name}\"><pose>{}</pose><inertial>{centroid}<mass>{}</mass>\
                     <inertia><ixx>{}</ixx><ixy>0</ixy><ixz>0</ixz><iyy>{}</iyy><iyz>0</iyz>\
                     <izz>{}</izz></inertia></inertial><collision name=\"collision\"><geometry>{geometry}\
                     </geometry></collision><visual name=\"visual\"><geometry>{geometry}\
                     </geometry>{GREY}</visual></link>",
                    sdf_pose(pose),
                    n(LINK_MASS),
                    n(ixx),
                    n(iyy),
                    n(izz),
                )
            }
            Part::Visual {
                name, shape, pose, ..
            } => {
                let (geometry, ..) = shape_geometry(*shape, PART_SIZE, 1.0);
                format!(
                    "<visual name=\"{name}\"><pose>{}</pose><geometry>{geometry}</geometry>\
                     {GREY}</visual>",
                    sdf_pose(pose)
                )
            }
            Part::Collision {
                name, shape, pose, ..
            } => {
                let (geometry, ..) = shape_geometry(*shape, PART_SIZE, 1.0);
                format!(
                    "<collision name=\"{name}\"><pose>{}</pose><geometry>{geometry}</geometry>\
                     </collision>",
                    sdf_pose(pose)
                )
            }
            Part::Sensor {
                name, kind, pose, ..
            } => format!(
                "<sensor name=\"{name}\" type=\"{}\"><pose>{}</pose><always_on>true</always_on>\
                 <update_rate>{}</update_rate><visualize>true</visualize>{}</sensor>",
                kind.ty(),
                sdf_pose(pose),
                number(kind.rate()),
                kind.body()
            ),
            Part::Joint {
                name,
                kind,
                parent,
                child,
            } => {
                let axis = match kind {
                    JointKind::Fixed | JointKind::Ball => "",
                    _ => "<axis><xyz>0 0 1</xyz></axis>",
                };
                format!(
                    "<joint name=\"{name}\" type=\"{}\"><parent>{}</parent><child>{}</child>\
                     {axis}</joint>",
                    kind.as_str(),
                    sdf_link_name(model, parent),
                    sdf_link_name(model, child)
                )
            }
            Part::Plugin { name, filename } => {
                let (filename, name) = (xml_escape(filename), xml_escape(name));
                format!("<plugin filename=\"{filename}\" name=\"{name}\"/>")
            }
            Part::Reparent { .. } => String::new(),
        }
    }

    /// Adds what the overlays, the tree and the inspectors read of it to
    /// `extras`, for model `model`: its own SDFormat, parsed.
    pub fn add_to(&self, model: &str, extras: &mut WorldExtras) {
        let wrap =
            |inner: String| format!("<sdf version=\"1.9\"><model name=\"m\">{inner}</model></sdf>");
        let parsed = |inner: String| WorldExtras::parse(&wrap(inner)).ok();
        match self {
            Part::Link { name, .. } => {
                if let Some(mut e) = parsed(self.sdf(model)) {
                    if let Some(link) = e.links.remove(&format!("m/{name}")) {
                        extras.links.insert(format!("{model}/{name}"), link);
                    }
                }
            }
            Part::Visual { .. } => {}
            Part::Collision { link, .. } | Part::Sensor { link, .. } => {
                let inner = format!("<link name=\"l\">{}</link>", self.sdf(model));
                let Some(mut e) = parsed(inner) else { return };
                let Some(got) = e.links.remove("m/l") else {
                    return;
                };
                let to = extras.links.entry(link.clone()).or_default();
                to.collisions.extend(got.collisions);
                to.sensors.extend(got.sensors);
            }
            Part::Joint {
                kind, name, child, ..
            } => {
                let inner = format!(
                    "<link name=\"p\"/><link name=\"c\"/>{}",
                    Part::Joint {
                        name: name.clone(),
                        kind: *kind,
                        parent: "m/p".into(),
                        child: "m/c".into(),
                    }
                    .sdf("m")
                );
                let Some(mut e) = parsed(inner) else { return };
                let Some(got) = e.links.remove("m/c") else {
                    return;
                };
                let to = extras.links.entry(child.clone()).or_default();
                for mut j in got.joints {
                    if let Part::Joint { parent, child, .. } = self {
                        j.parent = sdf_link_name(model, parent);
                        j.child = sdf_link_name(model, child);
                    }
                    j.model = model.to_string();
                    to.joints.push(j);
                }
            }
            Part::Plugin { name, .. } => {
                extras
                    .models
                    .entry(model.to_string())
                    .or_default()
                    .plugins
                    .push(name.clone());
            }
            Part::Reparent {
                joint,
                model,
                child,
                parent,
            } => {
                let joints = extras.links.get_mut(child).map(|l| &mut l.joints);
                let found = joints
                    .into_iter()
                    .flatten()
                    .find(|j| j.name == *joint && j.model == *model);
                if let Some(j) = found {
                    j.parent = sdf_link_name(model, parent);
                }
            }
        }
    }

    /// The sensor as the scene tree lists it, for a sensor.
    pub fn sensor(&self) -> Option<SensorDesc> {
        match self {
            Part::Sensor {
                name, kind, pose, ..
            } => Some(SensorDesc {
                name: name.clone(),
                id: 0,
                ty: kind.ty().to_string(),
                pose: *pose,
            }),
            _ => None,
        }
    }
}

/// Why a joint from `parent` to `child` can't be added, given the joints
/// the model has (each `(parent, child)`, by scene path), if it can't: a
/// link is the child of one joint at most, and a joint can't close a
/// loop.
pub fn joint_problem(parent: &str, child: &str, joints: &[(String, String)]) -> Option<String> {
    if parent == child {
        return Some("A joint needs two links".into());
    }
    if joints.iter().any(|(_, c)| c == child) {
        return Some(format!("{} has a parent joint already", leaf(child)));
    }
    // Up from the parent: reaching the child would close a loop.
    let mut at = parent.to_string();
    for _ in 0..=joints.len() {
        if at == child {
            return Some(format!("{} is above {} already", leaf(child), leaf(parent)));
        }
        match joints.iter().find(|(_, c)| *c == at) {
            Some((p, _)) => at = p.clone(),
            None => break,
        }
    }
    None
}

/// The parts added so far, in order, and those undone (redo puts them
/// back; a new part forgets them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    parts: Vec<Part>,
    undone: Vec<Part>,
}

impl Draft {
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn add(&mut self, part: Part) {
        self.parts.push(part);
        self.undone.clear();
    }

    /// Takes the last part back, returning it.
    pub fn undo(&mut self) -> Option<&Part> {
        let part = self.parts.pop()?;
        self.undone.push(part);
        self.undone.last()
    }

    /// Puts the last part undone back, returning it.
    pub fn redo(&mut self) -> Option<&Part> {
        let part = self.undone.pop()?;
        self.parts.push(part);
        self.parts.last()
    }

    /// What undo would take back: "add link_2".
    pub fn undo_label(&self) -> Option<String> {
        self.parts
            .last()
            .map(|p| format!("{} {}", p.verb(), p.name()))
    }

    /// What redo would put back.
    pub fn redo_label(&self) -> Option<String> {
        self.undone
            .last()
            .map(|p| format!("{} {}", p.verb(), p.name()))
    }

    /// `base_N` with the lowest N from 1 that `taken` doesn't hold.
    pub fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
        (1..)
            .map(|n| format!("{base}_{n}"))
            .find(|name| !taken(name))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
