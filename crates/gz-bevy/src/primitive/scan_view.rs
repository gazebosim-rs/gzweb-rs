// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Measured scan lines only; no scene nodes, Core IDs, meshes or materials.
use crate::{connection::GzInbox, spawn::GzWorldRoot};
use bevy::prelude::*;
use gz_core::primitive::ScanFrame;

#[derive(Resource)]
pub(crate) struct MeasuredScan(pub ScanFrame);

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(crate) struct ScanGizmos;

pub(crate) fn apply(mut commands: Commands, mut inbox: ResMut<GzInbox>) {
    match inbox.primitive_scan.take() {
        Some(Some(frame)) => {
            commands.insert_resource(MeasuredScan(frame));
        }
        Some(None) => {
            commands.remove_resource::<MeasuredScan>();
        }
        None => {}
    }
}

// Compute in the actual acquisition sensor frame in f64, then apply the existing
// world-root transform once. No current body pose can move an old measurement.
fn segments(frame: &ScanFrame, root: GlobalTransform) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    frame.scan.iter().flat_map(move |scan| {
        let origin = bevy::math::DVec3::from_array(scan.sensor_position);
        let rotation = bevy::math::DQuat::from_array(scan.sensor_orientation);
        scan.ranges
            .iter()
            .enumerate()
            .filter_map(move |(index, range)| {
                let range = (*range)?;
                let angle = scan.angle(index);
                let point = origin
                    + rotation
                        * bevy::math::DVec3::new(range * angle.cos(), range * angle.sin(), 0.0);
                let start = root.transform_point(origin.as_vec3());
                let end = root.transform_point(point.as_vec3());
                (start.is_finite() && end.is_finite()).then_some((start, end))
            })
    })
}

pub(crate) fn draw(
    measured: Option<Res<MeasuredScan>>,
    roots: Query<&GlobalTransform, With<GzWorldRoot>>,
    mut gizmos: Gizmos<ScanGizmos>,
) {
    let (Some(frame), Ok(root)) = (measured, roots.single()) else {
        return;
    };
    for (start, end) in segments(&frame.0, *root) {
        // Only actual finite returns are drawn; null beams have no endpoint.
        gizmos.line(start, end, Color::srgb(1.0, 0.25, 0.05));
        gizmos.cross(
            Isometry3d::from_translation(end),
            0.035,
            Color::srgb(1.0, 0.8, 0.1),
        );
    }
}

#[cfg(test)]
#[path = "scan_view_tests.rs"]
mod tests;
