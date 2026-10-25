//! What the Assets panel inserts into a world (DECISIONS.md D-102): a
//! shape or a light as SDFormat, the way Gazebo's own Shapes and Lights
//! tools write them, or a model by URI (a Fuel model, `model://name`, a
//! file the server can read), as an `<include>`; and the `create`
//! request's message.

use crate::msgs;

/// A primitive the Shapes tab inserts: a one-link model with a 1 kg
/// inertial, a collision and a grey visual of the same geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Box,
    Sphere,
    Cylinder,
    Capsule,
    Cone,
    /// A 2 m square ground plane, static.
    Plane,
}

impl Shape {
    pub const ALL: [Shape; 6] = [
        Shape::Box,
        Shape::Sphere,
        Shape::Cylinder,
        Shape::Capsule,
        Shape::Cone,
        Shape::Plane,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Box => "Box",
            Shape::Sphere => "Sphere",
            Shape::Cylinder => "Cylinder",
            Shape::Capsule => "Capsule",
            Shape::Cone => "Cone",
            Shape::Plane => "Plane",
        }
    }

    /// Its model's name (the server renames a taken one).
    pub fn name(self) -> &'static str {
        match self {
            Shape::Box => "box",
            Shape::Sphere => "sphere",
            Shape::Cylinder => "cylinder",
            Shape::Capsule => "capsule",
            Shape::Cone => "cone",
            Shape::Plane => "plane",
        }
    }

    /// Its geometry, and its inertia's diagonal for 1 kg.
    fn geometry(self) -> (&'static str, [f64; 3]) {
        match self {
            Shape::Box => ("<box><size>1 1 1</size></box>", [0.1667, 0.1667, 0.1667]),
            Shape::Sphere => ("<sphere><radius>0.5</radius></sphere>", [0.1, 0.1, 0.1]),
            Shape::Cylinder => (
                "<cylinder><radius>0.5</radius><length>1</length></cylinder>",
                [0.1458, 0.1458, 0.125],
            ),
            Shape::Capsule => (
                "<capsule><radius>0.2</radius><length>0.6</length></capsule>",
                [0.0742, 0.0742, 0.0188],
            ),
            Shape::Cone => (
                "<cone><radius>0.5</radius><length>1</length></cone>",
                [0.075, 0.075, 0.075],
            ),
            Shape::Plane => (
                "<plane><normal>0 0 1</normal><size>2 2</size></plane>",
                [1.0, 1.0, 1.0],
            ),
        }
    }

    /// How far above the point it is placed on its origin goes, so it
    /// rests on the surface.
    pub fn lift(self) -> f64 {
        match self {
            // Each is a metre tall (the capsule 0.6 and two 0.2 caps).
            Shape::Box | Shape::Sphere | Shape::Cylinder | Shape::Capsule | Shape::Cone => 0.5,
            Shape::Plane => 0.0,
        }
    }

    /// Its model as SDFormat.
    pub fn sdf(self) -> String {
        let (geometry, [ixx, iyy, izz]) = self.geometry();
        let name = self.name();
        let fixed = if self == Shape::Plane {
            "<static>true</static>"
        } else {
            ""
        };
        format!(
            "<?xml version=\"1.0\"?><sdf version=\"1.9\"><model name=\"{name}\">{fixed}\
             <link name=\"{name}_link\"><inertial><mass>1</mass><inertia>\
             <ixx>{ixx}</ixx><ixy>0</ixy><ixz>0</ixz><iyy>{iyy}</iyy><iyz>0</iyz><izz>{izz}</izz>\
             </inertia></inertial><collision name=\"{name}_collision\"><geometry>{geometry}\
             </geometry></collision><visual name=\"{name}_visual\"><geometry>{geometry}\
             </geometry><material><ambient>0.3 0.3 0.3 1</ambient><diffuse>0.7 0.7 0.7 1\
             </diffuse><specular>1 1 1 1</specular></material></visual></link></model></sdf>"
        )
    }
}

/// A light the Lights tab inserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

impl LightKind {
    pub const ALL: [LightKind; 3] = [LightKind::Directional, LightKind::Point, LightKind::Spot];

    pub fn label(self) -> &'static str {
        match self {
            LightKind::Directional => "Directional",
            LightKind::Point => "Point",
            LightKind::Spot => "Spot",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LightKind::Directional => "directional_light",
            LightKind::Point => "point_light",
            LightKind::Spot => "spot_light",
        }
    }

    /// How far above the point it is placed it goes.
    pub fn lift(self) -> f64 {
        match self {
            LightKind::Directional => 10.0,
            LightKind::Point | LightKind::Spot => 2.0,
        }
    }

    /// Its light as SDFormat.
    pub fn sdf(self) -> String {
        let name = self.name();
        let body = match self {
            LightKind::Directional => {
                "<cast_shadows>true</cast_shadows><diffuse>0.8 0.8 0.8 1</diffuse>\
                 <specular>0.2 0.2 0.2 1</specular><attenuation><range>1000</range>\
                 <constant>0.9</constant><linear>0.01</linear><quadratic>0.001</quadratic>\
                 </attenuation><direction>-0.5 0.1 -0.9</direction>"
            }
            LightKind::Point => {
                "<cast_shadows>false</cast_shadows><diffuse>0.5 0.5 0.5 1</diffuse>\
                 <specular>0.5 0.5 0.5 1</specular><attenuation><range>4</range>\
                 <constant>0.2</constant><linear>0.5</linear><quadratic>0.01</quadratic>\
                 </attenuation>"
            }
            LightKind::Spot => {
                "<cast_shadows>true</cast_shadows><diffuse>0.5 0.5 0.5 1</diffuse>\
                 <specular>0.5 0.5 0.5 1</specular><attenuation><range>4</range>\
                 <constant>0.2</constant><linear>0.5</linear><quadratic>0.01</quadratic>\
                 </attenuation><direction>0 0 -1</direction><spot><inner_angle>0.1\
                 </inner_angle><outer_angle>0.5</outer_angle><falloff>0.8</falloff></spot>"
            }
        };
        let kind = match self {
            LightKind::Directional => "directional",
            LightKind::Point => "point",
            LightKind::Spot => "spot",
        };
        format!(
            "<?xml version=\"1.0\"?><sdf version=\"1.9\"><light type=\"{kind}\" \
             name=\"{name}\">{body}</light></sdf>"
        )
    }
}

/// Something to insert.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Insertable {
    Shape(Shape),
    Light(LightKind),
    /// A model by URI, named `name`: a Fuel model's URL, `model://…`, or
    /// a path on the server.
    Model {
        uri: String,
        name: String,
    },
}

impl Insertable {
    /// What its model or light is called (before the server renames a
    /// taken name).
    pub fn name(&self) -> String {
        match self {
            Insertable::Shape(s) => s.name().into(),
            Insertable::Light(l) => l.name().into(),
            Insertable::Model { name, .. } => name.clone(),
        }
    }

    /// What it's called on screen.
    pub fn label(&self) -> String {
        match self {
            Insertable::Shape(s) => s.label().into(),
            Insertable::Light(l) => format!("{} light", l.label()),
            Insertable::Model { name, .. } => name.clone(),
        }
    }

    /// How far above the point it is placed its origin goes.
    pub fn lift(&self) -> f64 {
        match self {
            Insertable::Shape(s) => s.lift(),
            Insertable::Light(l) => l.lift(),
            Insertable::Model { .. } => 0.0,
        }
    }

    /// Its SDFormat: the shape or light, or an `<include>` of the model.
    pub fn sdf(&self) -> String {
        match self {
            Insertable::Shape(s) => s.sdf(),
            Insertable::Light(l) => l.sdf(),
            Insertable::Model { uri, .. } => format!(
                "<?xml version=\"1.0\"?><sdf version=\"1.9\"><include><uri>{}</uri></include></sdf>",
                xml_escape(uri)
            ),
        }
    }

    /// The `create` request's message: it at `at` (Gazebo's world
    /// coordinates) on a surface facing `normal`, moved off it by
    /// [`lift`](Self::lift) along the normal (up, on the ground), so it
    /// rests on it rather than in it; renamed by the server if its name is
    /// taken.
    pub fn factory(&self, at: [f64; 3], normal: [f64; 3]) -> msgs::EntityFactory {
        let len = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        let n = if len > 1e-9 {
            normal.map(|v| v / len)
        } else {
            [0.0, 0.0, 1.0]
        };
        let lift = self.lift();
        let [x, y, z] = [0, 1, 2].map(|i| at[i] + n[i] * lift);
        msgs::EntityFactory {
            name: self.name(),
            allow_renaming: true,
            pose: Some(msgs::Pose {
                position: Some(msgs::Vector3d {
                    x,
                    y,
                    z,
                    ..Default::default()
                }),
                orientation: Some(msgs::Quaternion {
                    w: 1.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            from: Some(msgs::entity_factory::From::Sdf(self.sdf())),
            ..Default::default()
        }
    }

    /// Whether a model called `seen` is this one, as the server named it
    /// (its name, or its name and a suffix the server added).
    pub fn is_named(&self, seen: &str) -> bool {
        Self::named(&self.name(), seen)
    }

    /// Whether `seen` is `name` as the server may rename it (`name`, or
    /// `name_N`).
    pub fn named(name: &str, seen: &str) -> bool {
        seen == name
            || seen
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    }
}

/// A model name from a title ("Euro pallet" → "euro_pallet"): letters,
/// digits and underscores.
pub fn model_name(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "model".into()
    } else {
        out
    }
}

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each shape is a one-link model named after it, with its geometry
    /// in both collision and visual; the plane is static; lights are top
    /// level; none has a comma (the stock server splits frames on them).
    #[test]
    fn shapes_and_lights_are_sdformat() {
        for s in Shape::ALL {
            let sdf = s.sdf();
            assert!(
                sdf.contains(&format!("<model name=\"{}\">", s.name())),
                "{sdf}"
            );
            let geometry = s.geometry().0;
            assert_eq!(sdf.matches(geometry).count(), 2, "{sdf}");
            assert_eq!(sdf.contains("<static>true</static>"), s == Shape::Plane);
            assert!(!sdf.contains(','), "{sdf}");
        }
        for l in LightKind::ALL {
            let sdf = l.sdf();
            assert!(sdf.contains(&format!("name=\"{}\"", l.name())), "{sdf}");
            assert!(!sdf.contains(','), "{sdf}");
        }
        assert!(LightKind::Spot.sdf().contains("<spot>"));
        assert!(LightKind::Directional
            .sdf()
            .contains("type=\"directional\""));
    }

    /// The factory places it at the point, lifted to rest on it, upright,
    /// renamable; a model is an include of its escaped URI.
    #[test]
    fn the_factory_places_and_names_it() {
        let f = Insertable::Shape(Shape::Box).factory([1.0, 2.0, 0.25], [0.0, 0.0, 1.0]);
        assert_eq!(f.name, "box");
        assert!(f.allow_renaming);
        let p = f.pose.unwrap();
        let at = p.position.unwrap();
        assert_eq!((at.x, at.y, at.z), (1.0, 2.0, 0.75));
        assert_eq!(p.orientation.unwrap().w, 1.0);
        // On a wall facing +X: off it by half its size, upright.
        let f = Insertable::Shape(Shape::Box).factory([2.0, 0.0, 1.0], [3.0, 0.0, 0.0]);
        let at = f.pose.unwrap().position.unwrap();
        assert_eq!((at.x, at.y, at.z), (2.5, 0.0, 1.0));
        let m = Insertable::Model {
            uri: "https://fuel.gazebosim.org/1.0/OpenRobotics/models/Euro%20pallet".into(),
            name: "euro_pallet".into(),
        };
        let Some(msgs::entity_factory::From::Sdf(sdf)) = m.factory([0.0; 3], [0.0; 3]).from else {
            panic!();
        };
        assert!(sdf.contains("<include><uri>https://fuel.gazebosim.org/1.0/OpenRobotics/models/Euro%20pallet</uri></include>"), "{sdf}");
        let odd = Insertable::Model {
            uri: "model://a&b".into(),
            name: "x".into(),
        };
        assert!(odd.sdf().contains("model://a&amp;b"));
        assert_eq!(
            Insertable::Light(LightKind::Point)
                .factory([0.0; 3], [0.0, 0.0, 1.0])
                .pose
                .unwrap()
                .position
                .unwrap()
                .z,
            2.0
        );
        assert_eq!(Insertable::Light(LightKind::Spot).label(), "Spot light");
    }

    /// The server's renamed copy is still it; another model isn't.
    #[test]
    fn a_renamed_model_is_recognised() {
        let b = Insertable::Shape(Shape::Box);
        assert!(b.is_named("box"));
        assert!(b.is_named("box_3"));
        assert!(!b.is_named("box_"));
        assert!(!b.is_named("boxes"));
        assert!(!b.is_named("box_a"));
        assert_eq!(model_name("Euro pallet"), "euro_pallet");
        assert_eq!(model_name("  KMP 1500 (v2) "), "kmp_1500_v2");
        assert_eq!(model_name("!!!"), "model");
    }
}
