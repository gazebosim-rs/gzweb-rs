//! The Sensors panel: its rules, and the panel through the shell.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzPublish};
use gz_bevy::overlays::extras::{SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors, GzWorldRoot};
use gz_bevy::topics::GzTopics;
use gz_bevy::view::{Part, PartKind, Selection};
use gz_core::msgs;
use gz_core::scene::{NodeKind, Pose, SensorDesc};
use prost::Message as _;

use super::model::*;
use super::systems::{SensorList, NONE, SELECT, WAITING};
use super::view::{PreviewPart, SensorRow};
use super::{SensorsStart, ViewSensor, SENSORS};
use crate::panels::PanelView;
use crate::shell::sidebar::body::BodyView;
use crate::shell::{PanelStatus, ShellLayout, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

fn topics(list: &[(&str, &str)]) -> GzTopics {
    let mut t = GzTopics::default();
    t.set(
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    );
    t
}

/// A `<topic>` names the topic, or the root of several (an RGBD camera's
/// image beside its depth); without one, Gazebo's derived topic; the one
/// carrying the sensor's message wins; with no list, the `<topic>` alone.
#[test]
fn a_sensor_finds_its_topic() {
    let list = topics(&[
        ("/camera", "gz.msgs.Image"),
        ("/camera_info", "gz.msgs.CameraInfo"),
        ("/rgbd_camera/camera_info", "gz.msgs.CameraInfo"),
        ("/rgbd_camera/image", "gz.msgs.Image"),
        ("/rgbd_camera/points", "gz.msgs.PointCloudPacked"),
        ("/lidar", "gz.msgs.LaserScan"),
        ("/lidar/points", "gz.msgs.PointCloudPacked"),
        (
            "/world/w/model/robot/model/arm/link/tip/sensor/imu/imu",
            "gz.msgs.IMU",
        ),
    ]);
    let find = |path: &str, name: &str, ty: &str, explicit: Option<&str>| {
        sensor_topic("w", path, name, ty, explicit, &list)
    };
    assert_eq!(
        find("m/l", "c", "camera", Some("camera")).as_deref(),
        Some("/camera")
    );
    assert_eq!(
        find("m/l", "r", "rgbd_camera", Some("/rgbd_camera")).as_deref(),
        Some("/rgbd_camera/image")
    );
    assert_eq!(
        find("m/l", "l", "gpu_lidar", Some("lidar")).as_deref(),
        Some("/lidar")
    );
    assert_eq!(
        find("robot/arm/tip", "imu", "imu", None).as_deref(),
        Some("/world/w/model/robot/model/arm/link/tip/sensor/imu/imu")
    );
    assert_eq!(find("m/l", "x", "camera", None), None);
    let empty = GzTopics::default();
    assert_eq!(
        sensor_topic("w", "m/l", "c", "camera", Some("cam"), &empty).as_deref(),
        Some("/cam")
    );
    assert_eq!(badge("gpu_lidar"), "lidar");
    assert_eq!(badge("depth_camera"), "depth");
    assert_eq!(badge("imu"), "imu");
    assert_eq!(PreviewKind::of("gz.msgs.Image"), PreviewKind::Image);
    assert_eq!(PreviewKind::of("gz.msgs.LaserScan"), PreviewKind::Scan);
    assert_eq!(PreviewKind::of("gz.msgs.IMU"), PreviewKind::Text);
    let p = Preview {
        last_at: Some(1.0),
        ..default()
    };
    assert_eq!(p.stale(2.5), None);
    assert_eq!(
        p.stale(4.2).map(stale_label).as_deref(),
        Some("No data for 3.2 s")
    );
}

/// A world: a robot with a camera and a lidar on its base link and an IMU
/// on a nested arm's link, and a plain box.
struct World {
    robot: Entity,
    base: Entity,
    arm_link: Entity,
    boxed: Entity,
}

fn spawn_world(app: &mut App) -> World {
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let mut add = |parent: Entity, name: &str, path: &str, kind: NodeKind| {
        app.world_mut()
            .spawn((
                GzEntity {
                    name: name.into(),
                    id: 0,
                    kind,
                    path: path.into(),
                },
                Transform::default(),
                Visibility::default(),
                ChildOf(parent),
            ))
            .id()
    };
    let robot = add(root, "robot", "robot", NodeKind::Model);
    let base = add(robot, "base", "robot/base", NodeKind::Link);
    let arm = add(robot, "arm", "robot/arm", NodeKind::Model);
    let arm_link = add(arm, "tip", "robot/arm/tip", NodeKind::Link);
    let boxed = add(root, "box", "box", NodeKind::Model);
    let sensor = |name: &str, ty: &str| SensorDesc {
        name: name.into(),
        id: 0,
        ty: ty.into(),
        pose: Pose::IDENTITY,
    };
    app.world_mut().entity_mut(base).insert(GzSensors(vec![
        sensor("front", "camera"),
        sensor("lidar", "gpu_lidar"),
    ]));
    app.world_mut()
        .entity_mut(arm_link)
        .insert(GzSensors(vec![sensor("imu", "imu")]));
    World {
        robot,
        base,
        arm_link,
        boxed,
    }
}

const SCENE_SDF: &str = r#"<sdf version="1.9"><world name="w">
  <model name="robot">
    <link name="base">
      <sensor name="front" type="camera"><topic>camera</topic><update_rate>30</update_rate></sensor>
      <sensor name="lidar" type="gpu_lidar"><topic>lidar</topic><update_rate>10</update_rate></sensor>
    </link>
    <model name="arm"><link name="tip"><sensor name="imu" type="imu"/></link></model>
  </model>
</world></sdf>"#;

/// The panel live and open on a world with sensors, its SDFormat and
/// topics in.
fn live_sensors() -> (App, World) {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )))
    .init_resource::<SdfExtras>()
    .init_resource::<SdfWanted>();
    frames(&mut app, 2);
    let w = spawn_world(&mut app);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: SENSORS,
        slot: Slot::Primary,
    });
    go_live(&mut app, false);
    app.world_mut()
        .resource_mut::<gz_bevy::connection::GzStatus>()
        .world = Some("w".into());
    app.world_mut().resource_mut::<SdfExtras>().extras =
        Some(gz_core::sdf::WorldExtras::parse(SCENE_SDF).unwrap());
    *app.world_mut().resource_mut::<GzTopics>() = topics(&[
        ("/camera", "gz.msgs.Image"),
        ("/lidar", "gz.msgs.LaserScan"),
        (
            "/world/w/model/robot/model/arm/link/tip/sensor/imu/imu",
            "gz.msgs.IMU",
        ),
    ]);
    frames(&mut app, 2);
    sent(&mut app);
    (app, w)
}

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world
        .query::<(&BodyView, &crate::panels::PanelBody)>()
        .iter(world)
        .find(|(_, b)| b.0 == SENSORS)
        .unwrap()
        .0
         .0
        .clone()
}

fn select(app: &mut App, entities: Vec<Entity>, parts: Vec<Part>) {
    *app.world_mut().resource_mut::<Selection>() = Selection { entities, parts };
    frames(app, 3);
}

fn chosen(app: &App) -> Option<String> {
    app.world()
        .resource::<SensorsState>()
        .chosen
        .as_ref()
        .map(|c| c.1.clone())
}

fn status(app: &mut App) -> Option<String> {
    let world = app.world_mut();
    world
        .query::<(&PreviewPart, &Node, &Text)>()
        .iter(world)
        .find(|(p, ..)| **p == PreviewPart::Status)
        .filter(|(_, n, _)| n.display != Display::None)
        .map(|(_, _, t)| t.0.clone())
}

/// With nothing selected it asks for a model; a model without sensors
/// asks again; with no sensors anywhere it says so.
#[test]
fn the_panel_asks_for_a_model_with_sensors() {
    let (mut app, w) = live_sensors();
    assert_eq!(view(&mut app), PanelView::empty(SELECT));
    select(&mut app, vec![w.boxed], vec![]);
    assert_eq!(view(&mut app), PanelView::empty(SELECT));
    for e in [w.base, w.arm_link] {
        app.world_mut().entity_mut(e).insert(GzSensors(vec![]));
    }
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::empty(NONE));
}

/// A model's sensors list with their types and rates (nested models'
/// too); the camera is chosen first and previewed: its topic is held
/// while the panel shows, a frame decodes and shows, the header counts
/// them; a row picks another; going quiet says so.
#[test]
fn a_model_lists_and_previews_its_sensors() {
    let (mut app, w) = live_sensors();
    select(&mut app, vec![w.robot], vec![]);
    assert_eq!(view(&mut app), PanelView::Ready);
    let list = app.world().resource::<SensorList>().clone();
    let names: Vec<&str> = list.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["front", "lidar", "imu"]);
    assert_eq!(list.entries[0].rate, Some(30.0));
    assert_eq!(
        app.world().resource::<PanelStatus>().count(SENSORS),
        Some("3 on robot")
    );
    let text = all_text(&mut app);
    assert!(text.contains("camera") && text.contains("30 Hz") && text.contains("lidar"));
    assert_eq!(chosen(&app).as_deref(), Some("front"));
    let subs = sent(&mut app);
    assert!(
        subs.contains(&GzCommand::Subscribe("/camera".into())),
        "{subs:?}"
    );
    assert!(subs.contains(&GzCommand::Throttle("/camera".into(), PREVIEW_RATE)));
    assert_eq!(status(&mut app).as_deref(), Some(WAITING));

    // A 2×1 PNG.
    let mut png = vec![];
    {
        let mut enc = png::Encoder::new(&mut png, 2, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()
            .unwrap()
            .write_image_data(&[255; 8])
            .unwrap();
    }
    app.world_mut().write_message(GzPublish {
        topic: "/camera".into(),
        msg_type: "gz.msgs.Image".into(),
        payload: png,
    });
    for _ in 0..40 {
        app.update();
        if app.world().resource::<Preview>().fresh {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = app.world().resource::<Preview>().clone();
    assert_eq!(p.size, Some((2, 1)));
    assert!(p.fresh);
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains("2×1 · 30 Hz"));
    assert_eq!(status(&mut app), None);
    frames(&mut app, 50);
    assert_eq!(
        status(&mut app).map(|s| s.starts_with("No data for")),
        Some(true)
    );

    let lidar = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SensorRow)>()
            .iter(world)
            .find(|(_, r)| r.0 .1 == "lidar")
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: lidar });
    frames(&mut app, 2);
    assert_eq!(chosen(&app).as_deref(), Some("lidar"));
    let subs = sent(&mut app);
    assert!(
        subs.contains(&GzCommand::Unsubscribe("/camera".into())),
        "{subs:?}"
    );
    assert!(
        subs.contains(&GzCommand::Subscribe("/lidar".into())),
        "{subs:?}"
    );
    let scan = msgs::LaserScan {
        angle_min: -1.0,
        angle_max: 1.0,
        angle_step: 0.5,
        range_min: 0.1,
        range_max: 10.0,
        count: 5,
        vertical_count: 1,
        ranges: vec![2.0, 3.0, f64::INFINITY, 3.0, 2.0],
        ..Default::default()
    };
    app.world_mut().write_message(GzPublish {
        topic: "/lidar".into(),
        msg_type: "gz.msgs.LaserScan".into(),
        payload: scan.encode_to_vec(),
    });
    frames(&mut app, 3);
    assert_eq!(
        app.world()
            .resource::<Preview>()
            .scan
            .as_ref()
            .map(|s| s.beams.len()),
        Some(5)
    );
    assert!(all_text(&mut app).contains("5 × 1 · 10 Hz"));

    // Collapsed, it lets go.
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert!(sent(&mut app).contains(&GzCommand::Unsubscribe("/lidar".into())));
}

/// A selected sensor (from the tree or the inspector's View in Sensors)
/// is the one shown; an IMU shows its message as text.
#[test]
fn a_selected_sensor_is_shown() {
    let (mut app, w) = live_sensors();
    select(
        &mut app,
        vec![],
        vec![Part {
            kind: PartKind::Sensor,
            owner: w.arm_link,
            name: "imu".into(),
        }],
    );
    assert_eq!(chosen(&app).as_deref(), Some("imu"));
    let imu = msgs::Imu {
        entity_name: "tip".into(),
        ..Default::default()
    };
    app.world_mut().write_message(GzPublish {
        topic: "/world/w/model/robot/model/arm/link/tip/sensor/imu/imu".into(),
        msg_type: "gz.msgs.IMU".into(),
        payload: imu.encode_to_vec(),
    });
    frames(&mut app, 3);
    assert!(all_text(&mut app).contains("entity_name: \"tip\""));
    app.world_mut()
        .write_message(ViewSensor(w.base, "lidar".into()));
    frames(&mut app, 2);
    assert_eq!(chosen(&app).as_deref(), Some("lidar"));
}

/// A start opens the panel and shows its sensor once listed.
#[test]
fn a_start_shows_its_sensor() {
    let (mut app, w) = live_sensors();
    app.insert_resource(SensorsStart {
        sensor: Some("lidar".into()),
    });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<ShellLayout>().primary, Some(SENSORS));
    assert!(app.world().contains_resource::<SensorsStart>());
    select(&mut app, vec![w.robot], vec![]);
    assert_eq!(chosen(&app).as_deref(), Some("lidar"));
    assert!(!app.world().contains_resource::<SensorsStart>());
}
