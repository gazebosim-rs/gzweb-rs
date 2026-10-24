use super::*;

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

const WORLD: &str = r#"<?xml version="1.0"?>
<sdf version="1.10">
  <world name="w">
    <model name="robot">
      <pose>1 0 0 0 0 0</pose>
      <link name="base">
        <collision name="body">
          <pose>0 0 0.1 0 0 0</pose>
          <geometry><box><size>1 0.5 0.2</size></box></geometry>
        </collision>
        <collision name="mesh">
          <geometry>
            <mesh>
              <uri>model://robot/meshes/body.dae</uri>
              <scale>2 2 2</scale>
              <submesh><name>hull</name><center>true</center></submesh>
            </mesh>
          </geometry>
        </collision>
        <collision name="odd"><geometry><polyline/></geometry></collision>
        <collision name="empty"><geometry/></collision>
        <inertial>
          <pose>0.1 0 0 0 0 0</pose>
          <mass>2.5</mass>
          <inertia><ixx>0.1</ixx><iyy>0.2</iyy><izz>0.3</izz><ixy>0.01</ixy></inertia>
        </inertial>
      </link>
      <link name="wheel">
        <pose>0 0.3 0 1.5707963267948966 0 0</pose>
        <collision name="c">
          <geometry><cylinder><radius>0.1</radius><length>0.05</length></cylinder></geometry>
        </collision>
      </link>
      <link name="arm">
        <pose relative_to="wheel">0 0 1 0 0 0</pose>
      </link>
      <joint name="wheel_joint" type="revolute">
        <parent>base</parent>
        <child>wheel</child>
        <axis><xyz>0 0 1</xyz></axis>
      </joint>
      <joint name="arm_joint" type="prismatic">
        <pose relative_to="__model__">0 0.3 2 0 0 0</pose>
        <parent>base</parent>
        <child>arm</child>
        <axis><xyz expressed_in="__model__">0 0 2</xyz></axis>
      </joint>
      <joint name="fixed" type="fixed">
        <parent>world</parent>
        <child>base</child>
      </joint>
      <joint name="no_child" type="fixed"><parent>base</parent></joint>
      <model name="gripper">
        <link name="finger">
          <collision name="tip"><geometry><sphere><radius>0.02</radius></sphere></geometry></collision>
        </link>
      </model>
      <joint name="grip" type="universal">
        <parent>base</parent>
        <child>gripper::finger</child>
        <axis><xyz>1 0 0</xyz></axis>
        <axis2><xyz>0 1 0</xyz></axis2>
      </joint>
    </model>
  </world>
</sdf>"#;

#[test]
fn collisions_inertials_and_joints_land_on_their_links() {
    let w = WorldExtras::parse(WORLD).unwrap();
    let base = w.link("robot/base").unwrap();
    assert_eq!(
        base.collisions.len(),
        3,
        "polyline kept, empty geometry not"
    );
    assert_eq!(base.collisions[0].name, "body");
    assert_eq!(
        base.collisions[0].geometry,
        GeometryDesc::Box {
            size: [1.0, 0.5, 0.2]
        }
    );
    assert!(near(base.collisions[0].pose.pos, [0.0, 0.0, 0.1]));
    assert_eq!(
        base.collisions[1].geometry,
        GeometryDesc::Mesh {
            uri: "model://robot/meshes/body.dae".into(),
            load_uri: "robot/meshes/body.dae".into(),
            submesh: Some("hull".into()),
            center_submesh: true,
        }
    );
    assert_eq!(base.collisions[1].scale, [2.0; 3]);
    assert_eq!(base.collisions[0].scale, [1.0; 3]);
    assert_eq!(
        base.collisions[2].geometry,
        GeometryDesc::Unsupported("polyline".into())
    );
    let i = base.inertial.unwrap();
    assert_eq!(i.mass, 2.5);
    assert_eq!(i.inertia, [0.1, 0.01, 0.0, 0.2, 0.0, 0.3]);
    assert!(near(i.pose.pos, [0.1, 0.0, 0.0]));
    // The fixed joint to the world is drawn on its child.
    assert_eq!(base.joints.len(), 1);
    assert_eq!(base.joints[0].kind, JointKind::Fixed);
    assert!(base.joints[0].axes.is_empty());

    let wheel = w.link("robot/wheel").unwrap();
    assert_eq!(wheel.joints[0].name, "wheel_joint");
    assert_eq!(wheel.joints[0].kind, JointKind::Revolute);
    assert_eq!(wheel.joints[0].pose, Pose::IDENTITY);
    assert!(near(wheel.joints[0].axes[0], [0.0, 0.0, 1.0]));
    assert!(wheel.inertial.is_none());

    let finger = w.link("robot/gripper/finger").unwrap();
    assert_eq!(finger.collisions.len(), 1);
    assert_eq!(finger.joints[0].kind, JointKind::Universal);
    assert_eq!(finger.joints[0].axes.len(), 2);
}

#[test]
fn joint_poses_and_axes_resolve_into_the_child_frame() {
    let w = WorldExtras::parse(WORLD).unwrap();
    // `arm` sits 1 m along the wheel's z, which is rolled 90° about x: in
    // the model frame that is (0, 0.3 - 1, 0). The joint is at (0, 0.3, 2)
    // in the model, so (0, 1, 2) from the arm, in the arm's (rolled) axes:
    // x stays, model y is the arm's -z... rotated back by the roll.
    let arm = &w.link("robot/arm").unwrap().joints[0];
    assert_eq!(arm.kind, JointKind::Prismatic);
    let roll = rpy(std::f64::consts::FRAC_PI_2, 0.0, 0.0);
    let arm_in_model = Pose {
        pos: [0.0, 0.3 - 1.0, 0.0],
        quat: roll,
    };
    let want = mul(
        inverse(arm_in_model),
        Pose {
            pos: [0.0, 0.3, 2.0],
            quat: [0.0, 0.0, 0.0, 1.0],
        },
    );
    assert!(near(arm.pose.pos, want.pos), "{:?}", arm.pose);
    // Model z, in the joint frame (rolled with the arm): +y... normalised.
    let axis = rotate(inverse(mul(arm_in_model, arm.pose)).quat, [0.0, 0.0, 1.0]);
    assert!(near(arm.axes[0], axis), "{:?}", arm.axes);
    // As written, for the inspector.
    assert_eq!(
        arm.written_axes,
        [([0.0, 0.0, 2.0], Some("__model__".to_string()))]
    );
    assert!((arm.axes[0].iter().map(|c| c * c).sum::<f64>() - 1.0).abs() < 1e-9);
}

#[test]
fn poses_read_degrees_and_quaternions() {
    let xml = r#"<sdf version="1.9"><model name="m">
        <link name="a">
          <collision name="d"><pose degrees="true">1 2 3 0 0 90</pose>
            <geometry><sphere><radius>1</radius></sphere></geometry></collision>
          <collision name="q"><pose rotation_format="quat_xyzw">0 0 0 0 0 2 0</pose>
            <geometry><capsule><radius>0.1</radius><length>1</length></capsule></geometry></collision>
          <collision name="bad"><pose>1 2</pose>
            <geometry><cone/></geometry></collision>
          <collision name="shapes"><geometry><ellipsoid><radii>1 2 3</radii></ellipsoid></geometry></collision>
          <collision name="plane"><geometry><plane><normal>0 0 1</normal><size>10 20</size></plane></geometry></collision>
          <collision name="plane2"><geometry><plane/></geometry></collision>
          <collision name="nouri"><geometry><mesh/></geometry></collision>
        </link></model></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    let a = w.link("m/a").unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let q = a.collisions[0].pose.quat;
    assert!(near([q[0], q[1], q[2]], [0.0, 0.0, s]) && (q[3] - s).abs() < 1e-9);
    assert_eq!(a.collisions[1].pose.quat, [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(a.collisions[2].pose, Pose::IDENTITY);
    assert_eq!(
        a.collisions[2].geometry,
        GeometryDesc::Cone {
            radius: 1.0,
            length: 1.0
        }
    );
    assert_eq!(
        a.collisions[3].geometry,
        GeometryDesc::Ellipsoid {
            radii: [1.0, 2.0, 3.0]
        }
    );
    assert_eq!(
        a.collisions[4].geometry,
        GeometryDesc::Plane {
            normal: [0.0, 0.0, 1.0],
            size: [10.0, 20.0]
        }
    );
    assert_eq!(
        a.collisions[5].geometry,
        GeometryDesc::Plane {
            normal: [0.0, 0.0, 1.0],
            size: [1.0, 1.0]
        }
    );
    // A mesh without a URI has nothing to draw.
    assert_eq!(a.collisions.len(), 6);
}

#[test]
fn defaults_errors_and_the_request() {
    assert!(WorldExtras::parse("<not xml").is_err());
    assert!(WorldExtras::parse("<world/>").is_err());
    let w = WorldExtras::parse(
        r#"<sdf><world name="w"><model name="m"><link name="l"><inertial/></link>
           <joint name="j" type="weird"><child>l</child><axis/></joint></model></world></sdf>"#,
    )
    .unwrap();
    let l = w.link("m/l").unwrap();
    assert_eq!(
        l.inertial.unwrap(),
        Inertial {
            pose: Pose::IDENTITY,
            mass: 1.0,
            inertia: [1.0, 0.0, 0.0, 1.0, 0.0, 1.0],
        }
    );
    assert_eq!(l.joints[0].kind, JointKind::Other);
    assert_eq!(l.joints[0].axes, [[0.0, 0.0, 1.0]]);
    assert!(w.link("m/none").is_none());

    let req = generate_request();
    assert!(!req.contains(&b',') && !req.contains(&0));
    let msg = <crate::msgs::SdfGeneratorConfig as prost::Message>::decode(req.as_slice()).unwrap();
    assert_eq!(
        msg.global_entity_gen_config
            .and_then(|c| c.expand_include_tags)
            .map(|b| b.data),
        Some(true)
    );
    assert_eq!(generate_service("w"), "/world/w/generate_world_sdf");
}

#[test]
fn joint_kinds() {
    for (s, k, rot) in [
        ("revolute", JointKind::Revolute, true),
        ("continuous", JointKind::Continuous, true),
        ("prismatic", JointKind::Prismatic, false),
        ("fixed", JointKind::Fixed, false),
        ("ball", JointKind::Ball, false),
        ("universal", JointKind::Universal, true),
        ("revolute2", JointKind::Revolute2, true),
        ("screw", JointKind::Screw, true),
        ("gearbox", JointKind::Gearbox, true),
        ("x", JointKind::Other, false),
    ] {
        assert_eq!(JointKind::parse(s), k);
        assert_eq!(k.rotates(), rot, "{s}");
    }
}

#[test]
fn link_frames_stop_at_loops_and_unknown_frames() {
    let xml = r#"<sdf><model name="m">
        <link name="a"><pose relative_to="b">1 0 0 0 0 0</pose></link>
        <link name="b"><pose relative_to="a">0 1 0 0 0 0</pose></link>
        <link name="c"><pose relative_to="nowhere">0 0 1 0 0 0</pose></link>
        <joint name="j" type="fixed"><pose relative_to="c">0 0 0 0 0 0</pose>
          <parent>a</parent><child>a</child></joint>
        <joint name="k" type="fixed"><pose relative_to="__model__">0 0 0 0 0 0</pose>
          <parent>a</parent><child>c</child></joint>
        </model></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    // Finite despite the a ↔ b loop.
    let j = &w.link("m/a").unwrap().joints[0];
    assert!(j.pose.pos.iter().all(|c| c.is_finite()));
    // `c` is relative to an unknown frame: the model's. The model origin
    // is 1 m below it.
    let k = &w.link("m/c").unwrap().joints[0];
    assert!(near(k.pose.pos, [0.0, 0.0, -1.0]));
}

/// Collisions and inertials resolve `relative_to` into their link's frame,
/// as joints do.
#[test]
fn collision_and_inertial_poses_follow_relative_to() {
    let xml = r#"<sdf version="1.10"><model name="m">
        <link name="a"><pose>0 2 0 0 0 0</pose></link>
        <link name="l"><pose>1 0 0 0 0 0</pose>
          <collision name="c"><pose relative_to="__model__">0 0 0 0 0 0</pose>
            <geometry><box><size>1 1 1</size></box></geometry></collision>
          <collision name="own"><pose relative_to="l">0 0 3 0 0 0</pose>
            <geometry><box><size>1 1 1</size></box></geometry></collision>
          <inertial><pose relative_to="a">0 0 0 0 0 0</pose><mass>1</mass></inertial>
        </link></model></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    let l = w.link("m/l").unwrap();
    assert!(near(l.collisions[0].pose.pos, [-1.0, 0.0, 0.0]));
    assert!(near(l.collisions[1].pose.pos, [0.0, 0.0, 3.0]));
    assert!(near(l.inertial.unwrap().pose.pos, [-1.0, 2.0, 0.0]));
}

/// A joint on a nested model's link (`inner::link`) finds that link's
/// pose through the nested model.
#[test]
fn nested_links_resolve_through_their_model() {
    let xml = r#"<sdf version="1.10"><model name="m">
        <link name="base"/>
        <model name="inner"><pose>0 0 1 0 0 1.5707963267948966</pose>
          <link name="tip"><pose>1 0 0 0 0 0</pose></link>
        </model>
        <joint name="j" type="fixed"><pose relative_to="__model__">0 0 0 0 0 0</pose>
          <parent>base</parent><child>inner::tip</child></joint>
        </model></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    // The tip is at (0, 1, 1) in the model, turned 90° about z; the model
    // origin is (0, 0, -1) below it and, in its turned axes, 1 m along -x.
    let j = &w.link("m/inner/tip").unwrap().joints[0];
    assert!(near(j.pose.pos, [-1.0, 0.0, -1.0]), "{:?}", j.pose.pos);
}

#[test]
fn models_links_sensors_and_joints_carry_what_the_inspectors_show() {
    let xml = r#"<sdf version="1.9"><world name="w">
      <model name="cart">
        <static>true</static>
        <self_collide>1</self_collide>
        <plugin filename="gz-sim-diff-drive-system" name="gz::sim::systems::DiffDrive"/>
        <plugin filename="libcustom.so"/>
        <plugin filename="gz-sim-joint-state-publisher-system"
                name="gz::sim::systems::JointStatePublisher">
          <topic>/cart/joints</topic>
        </plugin>
        <link name="base">
          <gravity>false</gravity>
          <kinematic>true</kinematic>
          <sensor name="lidar" type="gpu_lidar">
            <topic>/scan</topic>
            <update_rate>10</update_rate>
            <always_on>true</always_on>
            <visualize>1</visualize>
          </sensor>
          <sensor name="imu" type="imu"/>
        </link>
        <link name="wheel"/>
        <joint name="hinge" type="revolute">
          <parent>base</parent>
          <child>wheel</child>
          <axis>
            <xyz>0 1 0</xyz>
            <limit><lower>-1.5</lower><upper>1.5</upper><effort>20</effort></limit>
            <dynamics><damping>0.2</damping></dynamics>
          </axis>
        </joint>
      </model>
      <model name="plain"><link name="l"/></model>
    </world></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    let cart = w.model("cart").unwrap();
    assert!(cart.is_static && cart.self_collide);
    assert_eq!(
        cart.plugins,
        [
            "gz::sim::systems::DiffDrive",
            "libcustom.so",
            "gz::sim::systems::JointStatePublisher"
        ]
    );
    assert_eq!(cart.joint_state_topic.as_deref(), Some("/cart/joints"));
    let plain = w.model("plain").unwrap();
    assert!(!plain.is_static && !plain.self_collide && plain.plugins.is_empty());
    assert_eq!(plain.joint_state_topic, None);
    let base = w.link("cart/base").unwrap();
    assert_eq!(
        base.flags,
        LinkFlags {
            gravity: false,
            kinematic: true,
            wind: false
        }
    );
    assert_eq!(w.link("plain/l").unwrap().flags, LinkFlags::default());
    assert_eq!(
        base.sensors[0],
        SensorExtras {
            name: "lidar".into(),
            ty: "gpu_lidar".into(),
            topic: Some("/scan".into()),
            update_rate: 10.0,
            always_on: true,
            visualize: true,
        }
    );
    assert_eq!(base.sensors[1].topic, None);
    let hinge = w.joint("cart", "hinge").unwrap();
    assert_eq!(
        (hinge.parent.as_str(), hinge.child.as_str()),
        ("base", "wheel")
    );
    assert_eq!(
        hinge.limits,
        [AxisLimits {
            lower: -1.5,
            upper: 1.5,
            effort: 20.0,
            damping: 0.2,
            ..AxisLimits::default()
        }]
    );
    assert_eq!(hinge.written_axes, [([0.0, 1.0, 0.0], None)]);
    assert!(w.joint("plain", "hinge").is_none());
}

#[test]
fn joints_belong_to_the_model_that_declares_them() {
    // `outer` declares j (prismatic) and k, whose child is the nested
    // model's link; `inner` declares its own j (revolute).
    let xml = r#"<sdf version="1.9"><world name="w">
      <model name="outer">
        <link name="base"/>
        <model name="inner">
          <link name="l"/><link name="m"/>
          <joint name="j" type="revolute"><parent>l</parent><child>m</child></joint>
        </model>
        <link name="arm"/>
        <joint name="j" type="prismatic"><parent>base</parent><child>arm</child></joint>
        <joint name="k" type="fixed"><parent>base</parent><child>inner::l</child></joint>
      </model>
      <model name="outer2"><link name="x"/></model>
    </world></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    for _ in 0..20 {
        assert_eq!(w.joint("outer", "j").unwrap().kind, JointKind::Prismatic);
        assert_eq!(
            w.joint("outer/inner", "j").unwrap().kind,
            JointKind::Revolute
        );
    }
    let k = w.joint("outer", "k").unwrap();
    assert_eq!((k.model.as_str(), k.child.as_str()), ("outer", "inner::l"));
    assert!(w.joint("outer/inner", "k").is_none());
    let mut outer: Vec<&str> = w.joints_of("outer").map(|j| j.name.as_str()).collect();
    outer.sort();
    assert_eq!(outer, ["j", "k"]);
    assert_eq!(w.joints_of("outer/inner").count(), 1);
    assert_eq!(w.joints_of("outer2").count(), 0);
}

#[test]
fn flags_read_in_any_case_and_a_default_topic_is_none() {
    let xml = r#"<sdf version="1.9"><model name="m">
      <static>True</static><self_collide>FALSE</self_collide>
      <link name="l"><gravity>False</gravity>
        <sensor name="c" type="camera"><topic>__default__</topic></sensor>
      </link></model></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    let m = w.model("m").unwrap();
    assert!(m.is_static && !m.self_collide);
    let l = w.link("m/l").unwrap();
    assert!(!l.flags.gravity);
    assert_eq!(l.sensors[0].topic, None);
}

/// A model's drive takes Twists on its `<topic>` (rooted) or
/// `/model/<name>/cmd_vel`; each JointPositionController's joints on its
/// `<topic>`, its `<sub_topic>` under the model, or the default from its
/// first joint and its joint index; one taking Actuators (with an
/// `actuator_number`, as gz-sim needs) is left out.
#[test]
fn drives_and_joint_controllers_have_their_topics() {
    let xml = r#"<sdf version="1.10"><world name="w">
      <model name="amr">
        <plugin filename="gz-sim-diff-drive-system" name="gz::sim::systems::DiffDrive"/>
        <plugin filename="gz-sim-joint-position-controller-system"
                name="gz::sim::systems::JointPositionController">
          <joint_name>lift</joint_name>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="x">
          <joint_name>arm</joint_name><topic>arm_cmd</topic>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="y">
          <joint_name>wrist</joint_name><joint_index>1</joint_index>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="z">
          <joint_name>left</joint_name><joint_name>right</joint_name>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="s">
          <joint_name>pan</joint_name><sub_topic>pan_cmd</sub_topic>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="u">
          <joint_name>tilt</joint_name><use_actuator_msg>1</use_actuator_msg>
          <actuator_number>0</actuator_number>
        </plugin>
        <plugin filename="gz-sim-joint-position-controller-system" name="v">
          <joint_name>roll</joint_name><use_actuator_msg>true</use_actuator_msg>
        </plugin>
      </model>
      <model name="car">
        <plugin filename="gz-sim-ackermann-steering-system" name="a"><topic>drive</topic></plugin>
      </model>
      <model name="crate"/>
    </world></sdf>"#;
    let w = WorldExtras::parse(xml).unwrap();
    let amr = &w.models["amr"];
    assert_eq!(amr.drive_topic.as_deref(), Some("/model/amr/cmd_vel"));
    assert_eq!(
        amr.joint_controllers,
        [
            (
                "lift".to_string(),
                "/model/amr/joint/lift/0/cmd_pos".to_string()
            ),
            ("arm".to_string(), "/arm_cmd".to_string()),
            (
                "wrist".to_string(),
                "/model/amr/joint/wrist/1/cmd_pos".to_string()
            ),
            (
                "left".to_string(),
                "/model/amr/joint/left/0/cmd_pos".to_string()
            ),
            (
                "right".to_string(),
                "/model/amr/joint/left/0/cmd_pos".to_string()
            ),
            ("pan".to_string(), "/model/amr/pan_cmd".to_string()),
            (
                "roll".to_string(),
                "/model/amr/joint/roll/0/cmd_pos".to_string()
            ),
        ]
    );
    assert_eq!(w.models["car"].drive_topic.as_deref(), Some("/drive"));
    assert_eq!(w.models["crate"].drive_topic, None);
}
