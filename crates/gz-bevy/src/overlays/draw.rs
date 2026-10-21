//! The overlays drawn as gizmo lines, over the scene (they'd hide inside
//! the bodies otherwise), after transforms propagate so they move with
//! this frame's poses. As the Gazebo GUI draws them (gz-rendering's
//! `AxisVisual`, `JointVisual`, `InertiaVisual`, `COMVisual`):
//!
//! - **Frames:** red/green/blue X/Y/Z axes at every model's and link's
//!   origin ([`FRAME_MODEL`], [`FRAME_LINK`] long).
//! - **Joints:** axes at the joint frame, an arrow along each joint axis,
//!   with a ring about it for joints that turn.
//! - **Inertia:** the box of uniform density with the link's mass and
//!   principal moments, at the inertial frame turned to the principal axes
//!   ([`equivalent_box`]).
//! - **Centre of mass:** a sphere of lead with the link's mass
//!   ([`com_radius`]) with a cross through it.

use bevy::prelude::*;
use gz_core::scene::{NodeKind, Pose};
use gz_core::sdf::{Inertial, LinkExtras};

use super::extras::SdfExtras;
use super::Overlays;
use crate::spawn::{pose_transform, GzEntity};

/// Gizmo settings for the overlays.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct OverlayGizmos;

/// A model's frame axes, in metres.
pub const FRAME_MODEL: f32 = 0.5;
/// A link's frame axes.
pub const FRAME_LINK: f32 = 0.25;
/// A joint's frame axes and axis arrows.
pub const JOINT_SIZE: f32 = 0.3;
/// Joint axes and rings (gz-rendering draws joint axes in yellow).
pub const JOINT_COLOR: Color = Color::srgb(1.0, 0.82, 0.1);
/// The inertia box (gz-rendering's translucent purple).
pub const INERTIA_COLOR: Color = Color::srgb(0.69, 0.32, 0.92);
/// The centre-of-mass sphere.
pub const COM_COLOR: Color = Color::srgb(0.1, 0.85, 0.85);
/// Lead, kg/m³: the centre-of-mass sphere's density (gz-rendering's
/// `COMVisual`).
pub const LEAD_DENSITY: f32 = 11340.0;

/// Draws overlay gizmos over everything.
pub fn setup_overlay_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<OverlayGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 2.0;
}

/// The links' world transforms and their SDFormat extras.
fn links_with<'a>(
    nodes: &'a Query<(&GzEntity, &GlobalTransform)>,
    extras: &'a SdfExtras,
) -> Vec<(&'a GlobalTransform, &'a LinkExtras)> {
    let Some(world) = extras.extras.as_ref() else {
        return Vec::new();
    };
    nodes
        .iter()
        .filter(|(g, _)| g.kind == NodeKind::Link)
        .filter_map(|(g, t)| Some((t, world.link(&g.path)?)))
        .collect()
}

/// `pose` (in `frame`'s frame) in the world.
fn at(frame: &GlobalTransform, pose: &Pose) -> GlobalTransform {
    frame.mul_transform(pose_transform(pose, [1.0; 3]))
}

/// Frames: axes at every model and link.
pub fn draw_frames(
    overlays: Res<Overlays>,
    nodes: Query<(&GzEntity, &GlobalTransform)>,
    mut gizmos: Gizmos<OverlayGizmos>,
) {
    if !overlays.frames {
        return;
    }
    for (g, t) in &nodes {
        let len = match g.kind {
            NodeKind::Model => FRAME_MODEL,
            NodeKind::Link => FRAME_LINK,
            _ => continue,
        };
        gizmos.axes(unscaled(t), len);
    }
}

/// A transform without its scale, so axes keep their length on a scaled
/// model.
fn unscaled(t: &GlobalTransform) -> Transform {
    let (_, rotation, translation) = t.to_scale_rotation_translation();
    Transform::from_translation(translation).with_rotation(rotation)
}

/// Joints: axes at the joint frame and an arrow (with a ring for turning
/// joints) along each axis.
pub fn draw_joints(
    overlays: Res<Overlays>,
    extras: Res<SdfExtras>,
    nodes: Query<(&GzEntity, &GlobalTransform)>,
    mut gizmos: Gizmos<OverlayGizmos>,
) {
    if !overlays.joints {
        return;
    }
    for (link, x) in links_with(&nodes, &extras) {
        for j in &x.joints {
            let frame = unscaled(&at(link, &j.pose));
            gizmos.axes(frame, JOINT_SIZE * 0.5);
            for axis in &j.axes {
                let dir =
                    frame.rotation * Vec3::new(axis[0] as f32, axis[1] as f32, axis[2] as f32);
                let tip = frame.translation + dir * JOINT_SIZE;
                gizmos.arrow(frame.translation, tip, JOINT_COLOR);
                if j.kind.rotates() {
                    let iso = Isometry3d::new(
                        frame.translation + dir * JOINT_SIZE * 0.6,
                        Quat::from_rotation_arc(Vec3::Z, dir),
                    );
                    gizmos.circle(iso, JOINT_SIZE * 0.2, JOINT_COLOR);
                } else {
                    gizmos.arrow(
                        frame.translation,
                        frame.translation - dir * JOINT_SIZE,
                        JOINT_COLOR,
                    );
                }
            }
        }
    }
}

/// The box of uniform density with `inertial`'s mass and moments: its
/// size, and its rotation in the inertial frame (the principal axes).
/// Zero size along an axis whose moments can't come from a box (a
/// point mass, a rod).
pub fn equivalent_box(inertial: &Inertial) -> (Vec3, Quat) {
    let [ixx, ixy, ixz, iyy, iyz, izz] = inertial.inertia.map(|v| v as f32);
    let m = Mat3::from_cols(
        Vec3::new(ixx, ixy, ixz),
        Vec3::new(ixy, iyy, iyz),
        Vec3::new(ixz, iyz, izz),
    );
    let (moments, axes) = symmetric_eigen(m);
    let mass = (inertial.mass as f32).max(f32::EPSILON);
    let [a, b, c] = moments.to_array();
    // A box w × h × d: Ixx = m (h² + d²) / 12, and so on.
    let side = |p: f32, q: f32, r: f32| ((6.0 / mass) * (q + r - p)).max(0.0).sqrt();
    let size = Vec3::new(side(a, b, c), side(b, c, a), side(c, a, b));
    (size, Quat::from_mat3(&axes))
}

/// Eigenvalues and eigenvectors (columns, a right-handed rotation) of a
/// symmetric 3×3 matrix, by Jacobi rotations.
pub fn symmetric_eigen(m: Mat3) -> (Vec3, Mat3) {
    // Row-major: a[r][c]. `m` is symmetric, so its columns are its rows.
    let mut a = m.to_cols_array_2d();
    let mut v = [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (mut p, mut q, mut max) = (0, 1, 0.0f32);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > max {
                (p, q, max) = (i, j, a[i][j].abs());
            }
        }
        if max < 1e-12 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;
        let apq = a[p][q];
        a[p][p] -= t * apq;
        a[q][q] += t * apq;
        a[p][q] = 0.0;
        a[q][p] = 0.0;
        let r = 3 - p - q;
        let (arp, arq) = (a[r][p], a[r][q]);
        a[r][p] = c * arp - s * arq;
        a[p][r] = a[r][p];
        a[r][q] = s * arp + c * arq;
        a[q][r] = a[r][q];
        for row in &mut v {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    // `v` is row-major with the eigenvectors in its columns.
    let mut axes = Mat3::from_cols_array_2d(&v).transpose();
    if axes.determinant() < 0.0 {
        axes.z_axis = -axes.z_axis;
    }
    (Vec3::new(a[0][0], a[1][1], a[2][2]), axes)
}

/// Inertia: each link's equivalent box.
pub fn draw_inertia(
    overlays: Res<Overlays>,
    extras: Res<SdfExtras>,
    nodes: Query<(&GzEntity, &GlobalTransform)>,
    mut gizmos: Gizmos<OverlayGizmos>,
) {
    if !overlays.inertia {
        return;
    }
    for (link, x) in links_with(&nodes, &extras) {
        let Some(i) = &x.inertial else { continue };
        let (size, rotation) = equivalent_box(i);
        let frame = unscaled(&at(link, &i.pose));
        let t = Transform {
            translation: frame.translation,
            rotation: frame.rotation * rotation,
            scale: size.max(Vec3::splat(1e-3)),
        };
        gizmos.cube(t, INERTIA_COLOR);
    }
}

/// The radius of a lead sphere of `mass` kilograms (at least 1 cm).
pub fn com_radius(mass: f64) -> f32 {
    let m = (mass as f32).max(0.0);
    (3.0 * m / (4.0 * std::f32::consts::PI * LEAD_DENSITY))
        .cbrt()
        .max(0.01)
}

/// Centre of mass: a lead sphere and a cross at each link's.
pub fn draw_center_of_mass(
    overlays: Res<Overlays>,
    extras: Res<SdfExtras>,
    nodes: Query<(&GzEntity, &GlobalTransform)>,
    mut gizmos: Gizmos<OverlayGizmos>,
) {
    if !overlays.center_of_mass {
        return;
    }
    for (link, x) in links_with(&nodes, &extras) {
        let Some(i) = &x.inertial else { continue };
        let frame = unscaled(&at(link, &i.pose));
        let r = com_radius(i.mass);
        gizmos.sphere(
            Isometry3d::new(frame.translation, frame.rotation),
            r,
            COM_COLOR,
        );
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let d = frame.rotation * axis * r * 1.6;
            gizmos.line(frame.translation - d, frame.translation + d, COM_COLOR);
        }
    }
}
