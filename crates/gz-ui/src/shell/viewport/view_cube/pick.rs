//! The view cube's geometry and picking maths (spec §3.4, §5.17;
//! DECISIONS.md D-61), in Gazebo axes: a cube from -1 to 1 whose faces
//! are split 3 × 3. A face's middle square is the face (snap to that axis
//! view), the strips along its sides are the edges it shares (45° views)
//! and its corner squares are the corners (the views from those corners).
//! A region is the direction it looks from: the signs of the cube
//! coordinates it covers.

use bevy::prelude::*;
use gz_bevy::spawn::gz_to_bevy;
use gz_bevy::view::{SnapView, ViewOp};

/// How far an edge or corner region reaches into a face, of the face's
/// half side (1).
pub(crate) const BAND: f32 = 0.32;
/// The edges' thickness, in cube units (about a pixel at 56 px).
pub(crate) const EDGE: f32 = 0.07;
/// The cube camera's distance from the cube's middle, in cube units: a
/// perspective about as wide as the view's own (57° across where the
/// view's is 60°), so the near edge or corner stands out and a view along
/// an axis reads as a box, not as flat rectangles.
pub(crate) const CAMERA_DISTANCE: f32 = 4.0;
/// How much of the image the cube may take at most, side to side: 52 of
/// 56 pixels, two pixels' room on each side for the edges' antialiasing
/// and the drop shadow.
pub(crate) const FILL: f32 = 52.0 / 56.0;

/// The tangent of the cube camera's half field of view: the cone round the
/// cube's bounding sphere (its corners and the edges' ends) fills
/// [`FILL`] of the image. The camera never rolls away from the view's, so
/// that sphere is the cube's silhouette in the worst case, a corner view
/// (the board's): the cube fills the node there and is smaller, never
/// clipped, from anywhere else (gzweb's home view about 42 × 40 pixels,
/// a face on 36 × 36).
pub(crate) fn tan_half_fov() -> f32 {
    let r = 3f32.sqrt() * (1.0 + EDGE / 2.0);
    r / (CAMERA_DISTANCE * CAMERA_DISTANCE - r * r).sqrt() / FILL
}

/// Where the point `p` (Gazebo axes, cube units) lands on the cube's image
/// (-1..1, x right, y up) for a camera turned by `rot`, and its distance
/// in front of the camera.
pub(crate) fn project(rot: Quat, p: Vec3) -> (Vec2, f32) {
    let v = rot.inverse() * p;
    let depth = CAMERA_DISTANCE - v.z;
    (v.truncate() / (depth * tan_half_fov()), depth)
}

/// A face, edge or corner: each component -1, 0 or 1, not all 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CubeRegion(pub IVec3);

impl CubeRegion {
    /// The faces, in [`SnapView`]'s order of the face they show.
    pub(crate) const FACES: [IVec3; 6] = [
        IVec3::Z,
        IVec3::NEG_Z,
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
    ];

    /// The axis view a face shows: looking at it from outside. The +X face
    /// is the front view (looking along -X, D-13), +Y the right view.
    pub(crate) fn snap(self) -> Option<SnapView> {
        Some(match self.0.to_array() {
            [0, 0, 1] => SnapView::Top,
            [0, 0, -1] => SnapView::Bottom,
            [1, 0, 0] => SnapView::Front,
            [-1, 0, 0] => SnapView::Back,
            [0, 1, 0] => SnapView::Right,
            [0, -1, 0] => SnapView::Left,
            _ => return None,
        })
    }

    /// The face `view` looks at.
    pub(crate) fn of_snap(view: SnapView) -> CubeRegion {
        let (dir, _) = view.gz_axes();
        CubeRegion((-dir).round().as_ivec3())
    }

    /// What a click on it does: a face snaps to its axis view (which turns
    /// orthographic, §3.4); an edge or a corner turns the view to look
    /// from it, keeping the projection.
    pub(crate) fn op(self) -> ViewOp {
        match self.snap() {
            Some(face) => ViewOp::Snap(face),
            None => ViewOp::LookFrom(self.0.as_vec3()),
        }
    }
}

/// A camera rotation (Bevy space) in Gazebo axes.
pub(crate) fn gz_rotation(bevy_rotation: Quat) -> Quat {
    gz_to_bevy().inverse() * bevy_rotation
}

/// The region under `uv` (-1..1, x right, y up, over the cube image) for
/// a camera turned by `rot` (Gazebo axes), if the cube is there.
pub(crate) fn pick(rot: Quat, uv: Vec2) -> Option<CubeRegion> {
    // The ray from the camera through `uv`, in the cube camera's
    // perspective (the inverse of [`project`]).
    let origin = rot * Vec3::Z * CAMERA_DISTANCE;
    let fwd = rot * (uv * tan_half_fov()).extend(-1.0);
    // Slab test against the cube.
    let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..3 {
        let (o, d) = (origin[i], fwd[i]);
        if d.abs() < 1e-9 {
            if o.abs() > 1.0 {
                return None;
            }
            continue;
        }
        let (a, b) = ((-1.0 - o) / d, (1.0 - o) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    if t0 > t1 {
        return None;
    }
    Some(region_at(origin + fwd * t0))
}

/// The region a point on the cube's surface is in.
pub(crate) fn region_at(p: Vec3) -> CubeRegion {
    let edge = 1.0 - BAND;
    let s = |v: f32| {
        if v >= edge {
            1
        } else if v <= -edge {
            -1
        } else {
            0
        }
    };
    CubeRegion(IVec3::new(s(p.x), s(p.y), s(p.z)))
}

/// How far a side face may turn right and still count as facing the
/// camera squarely (the sine of about 6°).
const SQUARE: f32 = 0.1;

/// How a face is shaded, by where it faces on screen (spec §3.4: faces
/// `cube_top`, `cube_right`, `cube_left`; the isometric icon's light).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    Top,
    Right,
    Left,
}

/// The shade of the face with normal `n` (Gazebo axes) seen by a camera
/// turned by `rot`: facing up the screen is the top; else the side it
/// faces, and a side facing the camera squarely (gzweb's home view looks
/// at the west face so) is the left, the darker, to stand off the top.
pub(crate) fn shade(rot: Quat, n: Vec3) -> Shade {
    let v = rot.inverse() * n;
    if v.y > 0.35 {
        Shade::Top
    } else if v.x > SQUARE {
        Shade::Right
    } else {
        Shade::Left
    }
}

/// Where the middle of the face with normal `n` lands on the cube image of
/// side `size` (pixels from its top left), and how squarely it faces the
/// camera (the cosine between its normal and the way to the camera: 1
/// face on, 0 edge on, negative facing away).
pub(crate) fn label_at(rot: Quat, n: Vec3, size: f32) -> (Vec2, f32) {
    let (uv, _) = project(rot, n);
    let to_camera = rot * Vec3::Z * CAMERA_DISTANCE - n;
    let half = size / 2.0;
    (
        Vec2::new(half + uv.x * half, half - uv.y * half),
        n.dot(to_camera.normalize()),
    )
}

/// The face labels (spec §3.4: TOP, N, E, …): compass directions, Gazebo
/// being east-north-up.
pub(crate) fn face_label(n: IVec3) -> &'static str {
    match n.to_array() {
        [0, 0, 1] => "TOP",
        [0, 0, -1] => "BTM",
        [1, 0, 0] => "E",
        [-1, 0, 0] => "W",
        [0, 1, 0] => "N",
        _ => "S",
    }
}

/// The nine patches of the face with normal `n`: their region and corners
/// (counter-clockwise seen from outside, so they face out).
pub(crate) fn face_patches(n: IVec3) -> Vec<(CubeRegion, [Vec3; 4])> {
    let nf = n.as_vec3();
    // Two in-face axes with t1 × t2 = n.
    let t1 = if n.x != 0 {
        Vec3::Y * nf.x
    } else if n.y != 0 {
        Vec3::Z * nf.y
    } else {
        Vec3::X * nf.z
    };
    let t2 = nf.cross(t1);
    let edge = 1.0 - BAND;
    let spans = [(-1.0, -edge, -1), (-edge, edge, 0), (edge, 1.0, 1)];
    let mut out = Vec::with_capacity(9);
    for (u0, u1, su) in spans {
        for (v0, v1, sv) in spans {
            let p = |u: f32, v: f32| nf + t1 * u + t2 * v;
            let s = n + t1.as_ivec3() * su + t2.as_ivec3() * sv;
            out.push((CubeRegion(s), [p(u0, v0), p(u1, v0), p(u1, v1), p(u0, v1)]));
        }
    }
    out
}
