//! Placing (DECISIONS.md D-102): while [`Placing`] is on (the UI sets it
//! when something is chosen to insert), a click in a view (a press and
//! release that doesn't drag) places instead of selecting: a [`PlaceAt`]
//! names the surface point under it, the nearest mesh or else the
//! ground, in Gazebo's world coordinates, and whether Shift asked to
//! place more. Drags still move the camera. [`PlaceHover`] follows the
//! point under the pointer, for a preview.

use bevy::prelude::*;

use super::controls::{target, ViewRouting, Views};
use super::{viewport_ray, ViewScene};
use crate::spawn::gz_to_bevy;

/// How far from the camera a place can be.
pub const PLACE_FAR: f32 = 1000.0;

/// Clicks in the views place (set by the UI).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placing(pub bool);

/// A click placed at `point` (Gazebo world coordinates); `keep`: Shift was
/// held, so placing goes on.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlaceAt {
    pub point: [f64; 3],
    pub keep: bool,
}

/// While placing, the surface point under the pointer (Bevy world
/// coordinates), if it is over a view.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaceHover(pub Option<Vec3>);

/// A Bevy world point in Gazebo's coordinates.
pub fn to_gazebo(p: Vec3) -> [f64; 3] {
    let g = gz_to_bevy().inverse() * p;
    [f64::from(g.x), f64::from(g.y), f64::from(g.z)]
}

/// The surface point under `at` in the view whose camera is `camera`.
pub(super) fn surface_at(
    scene: &mut ViewScene,
    camera: (&Transform, &Projection),
    size: Vec2,
    at: Vec2,
) -> Option<Vec3> {
    let ray = viewport_ray(camera.1, camera.0, size, at)?;
    scene.surface(ray, PLACE_FAR)
}

/// Follows the point under the pointer while placing.
pub(super) fn place_hover(
    placing: Res<Placing>,
    windows: Query<&Window>,
    views: Views,
    routing: Res<ViewRouting>,
    cameras: Query<(&Transform, &Projection)>,
    mut scene: ViewScene,
    mut hover: ResMut<PlaceHover>,
) {
    let at = placing
        .0
        .then(|| target(&windows, &views, routing.hovered))
        .flatten()
        .filter(|_| routing.pointer)
        .and_then(|t| {
            let cursor = t.cursor?;
            let camera = cameras.get(t.view).ok()?;
            surface_at(&mut scene, camera, t.size, cursor)
        });
    hover.set_if_neq(PlaceHover(at));
}
