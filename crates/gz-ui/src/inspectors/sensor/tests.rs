use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::{Part, PartKind, Selection};

use super::{message_type, ViewInSensors};
use crate::inspectors::common::fixture;
use crate::panels::SENSORS;
use crate::test_support::frames;
use crate::test_support::shell::*;

#[test]
fn a_sensor_shows_its_output_and_opens_the_sensors_panel() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Sensor,
        owner: s.base,
        name: "lidar".into(),
    }];
    frames(&mut app, 3);
    let text = all_text(&mut app);
    for want in [
        "SENSOR",
        "cart › base › lidar",
        "gpu_lidar",
        "/scan",
        "gz.msgs.LaserScan",
        "10 Hz",
        "Vertical scan",
        "sel: lidar",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
    let view = find::<ViewInSensors>(&mut app);
    app.world_mut().trigger(Activate { entity: view });
    frames(&mut app, 3);
    assert_eq!(layout(&app).primary, Some(SENSORS));
    assert_eq!(message_type("imu"), Some("gz.msgs.IMU"));
    assert_eq!(message_type("custom"), None);
}

#[test]
fn before_the_sdformat_the_output_is_unknown() {
    use gz_bevy::overlays::extras::SdfExtras;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    app.world_mut().insert_resource(SdfExtras::default());
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Sensor,
        owner: s.base,
        name: "lidar".into(),
    }];
    frames(&mut app, 3);
    let text = all_text(&mut app);
    assert!(
        !text.contains("Default") && !text.contains("/scan"),
        "{text}"
    );
}
