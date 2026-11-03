// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::spawn::{spawn_root, GzIndex};

fn frame(time: u64) -> ScanFrame {
    ScanFrame {
        version: 1,
        world_id: 1,
        epoch: 0,
        sim_time_ns: time,
        scan: Some(gz_core::primitive::Scan {
            sensor_id: 7,
            acquired_ns: time,
            sensor_position: [1.0, 2.0, 3.0],
            sensor_orientation: [
                0.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
            angle_min: 0.0,
            angle_max: std::f64::consts::FRAC_PI_2,
            angle_step: std::f64::consts::FRAC_PI_4,
            range_min: 0.1,
            range_max: 10.0,
            ranges: vec![Some(2.0), None, Some(3.0)],
        }),
    }
}

#[test]
fn measured_returns_follow_acquisition_pose_and_existing_world_root_exactly_once() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .init_resource::<GzIndex>()
        .init_resource::<GzInbox>()
        .add_systems(Startup, spawn_root)
        .add_systems(Update, apply);
    app.update();
    let count = app.world().entities().len();
    assert!(!app.world().contains_resource::<MeasuredScan>());
    let measured = frame(10);
    measured.validate().unwrap();
    app.world_mut().resource_mut::<GzInbox>().primitive_scan = Some(Some(measured));
    app.update();
    assert_eq!(app.world().entities().len(), count);
    let root = app.world().resource::<GzIndex>().root.unwrap();
    let root = app.world().get::<GlobalTransform>(root).unwrap();
    let actual: Vec<_> = segments(&app.world().resource::<MeasuredScan>().0, *root).collect();
    assert_eq!(actual.len(), 2, "null has no drawn endpoint");
    let expected = [
        (Vec3::new(1.0, 3.0, -2.0), Vec3::new(1.0, 3.0, -4.0)),
        (Vec3::new(1.0, 3.0, -2.0), Vec3::new(-2.0, 3.0, -2.0)),
    ];
    for ((start, end), (a, b)) in actual.into_iter().zip(expected) {
        assert!((start - a).length() < 1e-5);
        assert!((end - b).length() < 1e-5);
    }
    // A new measurement replaces only the resource: no geometry/ID allocation.
    let mut next = frame(20);
    next.scan.as_mut().unwrap().sensor_position = [0.0; 3];
    app.world_mut().resource_mut::<GzInbox>().primitive_scan = Some(Some(next));
    app.update();
    assert_eq!(app.world().resource::<MeasuredScan>().0.sim_time_ns, 20);
    assert_eq!(app.world().entities().len(), count);
    app.world_mut().resource_mut::<GzInbox>().primitive_scan = Some(None);
    app.update();
    assert!(!app.world().contains_resource::<MeasuredScan>());
    assert_eq!(app.world().entities().len(), count);
}

#[test]
fn absent_or_all_no_return_scan_allocates_no_endpoints() {
    let mut empty = frame(0);
    empty.scan = None;
    assert_eq!(segments(&empty, GlobalTransform::IDENTITY).count(), 0);
    let mut no_return = frame(0);
    no_return.scan.as_mut().unwrap().ranges.fill(None);
    assert_eq!(segments(&no_return, GlobalTransform::IDENTITY).count(), 0);
}
