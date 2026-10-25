use super::*;

fn at(x: f64, y: f64, z: f64) -> Pose {
    Pose {
        pos: [x, y, z],
        quat: [0.0, 0.0, 0.0, 1.0],
    }
}

/// A new link is a 1 kg shape with a collision and a visual, at its pose;
/// what the overlays read of it is its SDFormat parsed.
#[test]
fn a_link_is_a_shape_with_mass() {
    let link = Part::Link {
        name: "link_1".into(),
        shape: Shape::Cylinder,
        pose: at(1.0, 0.0, 0.25),
    };
    let sdf = link.sdf("amr");
    assert!(
        sdf.starts_with("<link name=\"link_1\"><pose>1 0 0.25 0 0 0</pose>"),
        "{sdf}"
    );
    let mut extras = WorldExtras::default();
    link.add_to("amr", &mut extras);
    let got = extras.link("amr/link_1").expect("the link");
    let inertial = got.inertial.expect("an inertial");
    assert_eq!(inertial.mass, LINK_MASS);
    // ixx = m(3r² + l²)/12 for r 0.25, l 0.5.
    assert!((inertial.inertia[0] - (3.0 * 0.0625 + 0.25) / 12.0).abs() < 1e-6);
    assert_eq!(got.collisions.len(), 1);
    assert_eq!(
        got.collisions[0].geometry,
        GeometryDesc::Cylinder {
            radius: 0.25,
            length: 0.5
        }
    );
}

/// A collision and a sensor join their link's; a plugin its model's.
#[test]
fn collisions_sensors_and_plugins_join_what_they_go_on() {
    let mut extras = WorldExtras::default();
    Part::Collision {
        link: "amr/base".into(),
        name: "collision_1".into(),
        shape: Shape::Box,
        pose: at(0.0, 0.0, 0.1),
    }
    .add_to("amr", &mut extras);
    Part::Sensor {
        link: "amr/base".into(),
        name: "camera_1".into(),
        kind: SensorKind::Camera,
        pose: at(0.2, 0.0, 0.0),
    }
    .add_to("amr", &mut extras);
    Part::Plugin {
        name: "gz::sim::systems::JointStatePublisher".into(),
        filename: "gz-sim-joint-state-publisher-system".into(),
    }
    .add_to("amr", &mut extras);
    let base = extras.link("amr/base").unwrap();
    assert_eq!(base.collisions[0].name, "collision_1");
    assert_eq!(
        base.collisions[0].geometry,
        GeometryDesc::Box { size: [0.2; 3] }
    );
    assert_eq!(base.sensors[0].ty, "camera");
    assert_eq!(base.sensors[0].update_rate, 30.0);
    assert!(base.sensors[0].always_on && base.sensors[0].visualize);
    assert_eq!(
        extras.model("amr").unwrap().plugins,
        ["gz::sim::systems::JointStatePublisher"]
    );
}

/// A joint goes on its child link, naming its links as the model's
/// SDFormat does (a nested model's with `::`); a revolute one turns about
/// Z, a fixed one has no axis.
#[test]
fn a_joint_goes_on_its_child() {
    let mut extras = WorldExtras::default();
    let joint = Part::Joint {
        name: "joint_1".into(),
        kind: JointKind::Revolute,
        parent: "amr/base".into(),
        child: "amr/arm/upper".into(),
    };
    assert!(joint
        .sdf("amr")
        .contains("<parent>base</parent><child>arm::upper</child>"));
    joint.add_to("amr", &mut extras);
    let j = &extras.link("amr/arm/upper").unwrap().joints[0];
    assert_eq!(j.kind, JointKind::Revolute);
    assert_eq!(
        (j.parent.as_str(), j.child.as_str()),
        ("base", "arm::upper")
    );
    assert_eq!(j.model, "amr");
    assert_eq!(j.axes, [[0.0, 0.0, 1.0]]);
    assert!(!Part::Joint {
        name: "j".into(),
        kind: JointKind::Fixed,
        parent: "amr/a".into(),
        child: "amr/b".into(),
    }
    .sdf("amr")
    .contains("<axis>"));
}

/// Every part's SDFormat parses, and every sensor's type is one Gazebo
/// reads.
#[test]
fn every_part_parses() {
    for kind in SensorKind::ALL {
        let mut extras = WorldExtras::default();
        let s = Part::Sensor {
            link: "m/l".into(),
            name: "s".into(),
            kind,
            pose: Pose::IDENTITY,
        };
        s.add_to("m", &mut extras);
        assert_eq!(
            extras.link("m/l").unwrap().sensors[0].ty,
            kind.ty(),
            "{kind:?}"
        );
        assert_eq!(s.sensor().unwrap().ty, kind.ty());
    }
    for shape in PART_SHAPES {
        let mut extras = WorldExtras::default();
        Part::Link {
            name: "l".into(),
            shape,
            pose: Pose::IDENTITY,
        }
        .add_to("m", &mut extras);
        let link = extras.link("m/l").unwrap();
        assert_eq!(link.collisions.len(), 1, "{shape:?}");
        assert!(link.inertial.unwrap().inertia.iter().all(|i| *i >= 0.0));
        let v = shape_visual(shape, PART_SIZE);
        assert_eq!(v.proto_type, v.geometry.kind_name());
    }
    for kind in JOINT_KINDS {
        let mut extras = WorldExtras::default();
        Part::Joint {
            name: "j".into(),
            kind,
            parent: "m/a".into(),
            child: "m/b".into(),
        }
        .add_to("m", &mut extras);
        assert_eq!(extras.link("m/b").unwrap().joints[0].kind, kind);
        assert!(!joint_label(kind).is_empty());
    }
}

/// A cone link's centre of mass sits a quarter of its height below its
/// centre; a capsule's inertia counts its caps as hemispheres.
#[test]
fn cones_and_capsules_have_their_own_mass() {
    let inertial = |shape| {
        let mut extras = WorldExtras::default();
        Part::Link {
            name: "l".into(),
            shape,
            pose: Pose::IDENTITY,
        }
        .add_to("m", &mut extras);
        extras.link("m/l").unwrap().inertial.unwrap()
    };
    assert_eq!(inertial(Shape::Cone).pose.pos, [0.0, 0.0, -LINK_SIZE / 4.0]);
    assert_eq!(inertial(Shape::Box).pose.pos, [0.0; 3]);
    // r 0.1, l 0.3: 0.692 kg of cylinder, 0.308 kg of caps.
    let capsule = inertial(Shape::Capsule).inertia;
    assert!((capsule[0] - 0.018_538).abs() < 1e-5, "{capsule:?}");
    assert!((capsule[5] - 0.004_692).abs() < 1e-5, "{capsule:?}");
}

/// A plugin's library and class are escaped, so any text makes SDFormat
/// that reads back as typed.
#[test]
fn a_plugin_escapes_what_was_typed() {
    let plugin = Part::Plugin {
        name: "a<b>&\"c\"".into(),
        filename: "lib&.so".into(),
    };
    let sdf = plugin.sdf("m");
    let doc = roxmltree::Document::parse(&sdf).expect("well-formed");
    let root = doc.root_element();
    assert_eq!(root.attribute("name"), Some("a<b>&\"c\""));
    assert_eq!(root.attribute("filename"), Some("lib&.so"));
}

/// A link is a joint's child once at most, and a joint can't close a loop.
#[test]
fn joints_make_a_tree() {
    let joints = vec![
        ("m/base".to_string(), "m/arm".to_string()),
        ("m/arm".to_string(), "m/hand".to_string()),
    ];
    assert_eq!(joint_problem("m/hand", "m/finger", &joints), None);
    assert_eq!(
        joint_problem("m/base", "m/base", &joints).as_deref(),
        Some("A joint needs two links")
    );
    assert_eq!(
        joint_problem("m/base", "m/hand", &joints).as_deref(),
        Some("hand has a parent joint already")
    );
    assert_eq!(
        joint_problem("m/hand", "m/base", &joints).as_deref(),
        Some("base is above hand already")
    );
}

/// Undo takes the last part back and redo puts it back; a new part
/// forgets what was undone. Names count up from 1.
#[test]
fn the_draft_undoes_and_redoes() {
    let mut d = Draft::default();
    let link = |n: &str| Part::Link {
        name: n.into(),
        shape: Shape::Box,
        pose: Pose::IDENTITY,
    };
    assert!(d.is_empty() && d.undo_label().is_none());
    d.add(link("link_1"));
    d.add(link("link_2"));
    assert_eq!(d.undo_label().as_deref(), Some("add link_2"));
    assert_eq!(d.undo().map(Part::name), Some("link_2"));
    assert_eq!(d.redo_label().as_deref(), Some("add link_2"));
    assert_eq!(d.redo().map(Part::name), Some("link_2"));
    d.undo();
    d.add(link("link_3"));
    assert_eq!(d.redo_label(), None);
    assert_eq!(d.parts().len(), 2);
    assert_eq!(
        Draft::free_name("link", |n| ["link_1", "link_2"].contains(&n)),
        "link_3"
    );
    assert_eq!(sdf_link_name("m", "m/inner/wheel"), "inner::wheel");
    assert_eq!(
        Part::Plugin {
            name: "p".into(),
            filename: "f".into()
        }
        .link(),
        None
    );
}

/// The model as `model_sdf` gives it: base and, in a static nested
/// model, a wheel (empty).
const AMR: &str = "<?xml version=\"1.0\"?><sdf version=\"1.9\"><model name=\"amr\">\
                   <link name=\"base\"><inertial><mass>2</mass></inertial></link>\
                   <model name=\"inner\"><static>true</static><link name=\"wheel\"/></model>\
                   </model></sdf>";

/// The draft's parts go where they belong: a link, joint and plugin last in
/// the model, a visual or collision last in its link (a new link's in its
/// own element; an empty one opened out), a nested model's link found by
/// its path. The result is SDFormat the parts read back from as drawn.
#[test]
fn parts_are_written_into_their_model() {
    let parts = vec![
        Part::Link {
            name: "link_1".into(),
            shape: Shape::Box,
            pose: at(1.0, 0.0, 0.25),
        },
        Part::Visual {
            link: "amr/link_1".into(),
            name: "visual_1".into(),
            shape: Shape::Sphere,
            pose: at(0.0, 0.0, 0.3),
        },
        Part::Collision {
            link: "amr/inner/wheel".into(),
            name: "collision_1".into(),
            shape: Shape::Box,
            pose: Pose::IDENTITY,
        },
        Part::Sensor {
            link: "amr/base".into(),
            name: "imu_1".into(),
            kind: SensorKind::Imu,
            pose: Pose::IDENTITY,
        },
        Part::Joint {
            name: "joint_1".into(),
            kind: JointKind::Revolute,
            parent: "amr/base".into(),
            child: "amr/link_1".into(),
        },
        Part::Plugin {
            name: "gz::sim::systems::JointStatePublisher".into(),
            filename: "gz-sim-joint-state-publisher-system".into(),
        },
    ];
    let out = write_parts(AMR, "amr", &parts).unwrap();
    let x = WorldExtras::parse(&out).unwrap();
    let link = x.link("amr/link_1").expect("the new link");
    assert!(link.inertial.is_some());
    let base = x.link("amr/base").unwrap();
    assert_eq!(base.sensors[0].name, "imu_1");
    assert_eq!(
        x.link("amr/inner/wheel").unwrap().collisions[0].name,
        "collision_1"
    );
    assert_eq!(link.joints[0].parent, "base");
    assert_eq!(
        x.model("amr").unwrap().plugins,
        ["gz::sim::systems::JointStatePublisher"]
    );
    let doc = roxmltree::Document::parse(&out).unwrap();
    let visual = doc
        .descendants()
        .find(|n| n.has_tag_name("visual") && n.attribute("name") == Some("visual_1"))
        .unwrap();
    assert_eq!(visual.parent().unwrap().attribute("name"), Some("link_1"));
    assert!(out.starts_with(
        "<?xml version=\"1.0\"?><sdf version=\"1.9\"><model name=\"amr\"><link name=\"base\">"
    ));
    assert_eq!(check_model(&out), Ok(()), "{out}");
    // A part on a link the model hasn't.
    let lost = Part::Visual {
        link: "amr/gone".into(),
        name: "v".into(),
        shape: Shape::Box,
        pose: Pose::IDENTITY,
    };
    assert_eq!(
        write_parts(AMR, "amr", &[lost]),
        Err("no link gone in the model".into())
    );
}

/// Done's check: a link without inertia (unless its model is static), a
/// joint without both ends, or with an end the model hasn't, says why.
#[test]
fn a_model_is_checked_before_it_is_written() {
    let model =
        |inner: &str| format!("<sdf version=\"1.9\"><model name=\"m\">{inner}</model></sdf>");
    let link = "<link name=\"a\"><inertial/></link>";
    assert_eq!(check_model(&model(link)), Ok(()));
    assert_eq!(
        check_model(&model("<link name=\"a\"/>")),
        Err("a has no inertia (a link needs it unless its model is static)".into())
    );
    assert_eq!(
        check_model(&model("<static>true</static><link name=\"a\"/>")),
        Ok(())
    );
    assert_eq!(
        check_model(&model("<model name=\"i\"><link name=\"w\"/></model>")),
        Err("i::w has no inertia (a link needs it unless its model is static)".into())
    );
    let joint = |p: &str, c: &str| {
        model(&format!(
            "{link}<model name=\"i\"><static>1</static><link name=\"w\"/></model>\
             <frame name=\"f\"/><joint name=\"j\" type=\"fixed\"><parent>{p}</parent>\
             <child>{c}</child></joint>"
        ))
    };
    for (p, c) in [
        ("world", "a"),
        ("a", "i::w"),
        ("f", "i"),
        ("__model__", "a"),
    ] {
        assert_eq!(check_model(&joint(p, c)), Ok(()), "{p} {c}");
    }
    assert_eq!(
        check_model(&joint("a", "b")),
        Err("j's child b isn't in the model".into())
    );
    assert_eq!(
        check_model(&joint("", "a")),
        Err("j has no parent link".into())
    );
}
