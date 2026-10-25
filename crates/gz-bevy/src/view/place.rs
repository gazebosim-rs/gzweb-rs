//! Placing (DECISIONS.md D-102): while [`Placing`] is on (the UI sets it
//! when something is chosen to insert), a click in a view (a press and
//! release that doesn't drag) places instead of selecting: a [`PlaceAt`]
//! names the surface point under it, the nearest mesh or else the
//! ground, and the surface's normal there, in Gazebo's world coordinates,
//! the mesh it's on, and whether Shift asked to place more. Drags still move the camera.
//! [`PlaceHover`] follows the point under the pointer, for a preview.

use bevy::prelude::*;

use super::controls::{target, ViewRouting, Views};
use super::{viewport_ray, ViewScene};
use crate::spawn::gz_to_bevy;

/// How far from the camera a place can be.
pub const PLACE_FAR: f32 = 1000.0;

/// Clicks in the views place (set by the UI).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placing(pub bool);

/// A click placed at `point` on a surface facing `normal` (Gazebo world
/// coordinates; the ground's is +Z); `keep`: Shift was held, so placing
/// goes on; `hit`: the mesh clicked (none for the ground), so a part goes
/// on the link it is in.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlaceAt {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub keep: bool,
    pub hit: Option<Entity>,
}

/// While placing, the surface point under the pointer and its normal
/// (Bevy world coordinates), if it is over a view.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaceHover(pub Option<(Vec3, Vec3)>);

/// A Bevy world point in Gazebo's coordinates.
pub fn to_gazebo(p: Vec3) -> [f64; 3] {
    let g = gz_to_bevy().inverse() * p;
    [f64::from(g.x), f64::from(g.y), f64::from(g.z)]
}

/// The surface point under `at` in the view whose camera is `camera`,
/// its normal, and the mesh it's on.
pub(super) fn surface_at(
    scene: &mut ViewScene,
    camera: (&Transform, &Projection),
    size: Vec2,
    at: Vec2,
) -> Option<(Vec3, Vec3, Option<Entity>)> {
    let ray = viewport_ray(camera.1, camera.0, size, at)?;
    scene.surface_hit(ray, PLACE_FAR)
}

/// What the hover was last cast for: the view, the cursor and the
/// camera's place.
type HoverKey = (Entity, Vec2, Transform);

/// Follows the point under the pointer while placing; it casts again only
/// when the pointer, the view or its camera moved.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_hover(
    placing: Res<Placing>,
    windows: Query<&Window>,
    views: Views,
    routing: Res<ViewRouting>,
    cameras: Query<(&Transform, &Projection)>,
    mut scene: ViewScene,
    mut hover: ResMut<PlaceHover>,
    mut cast: Local<Option<HoverKey>>,
) {
    let under = placing
        .0
        .then(|| target(&windows, &views, routing.hovered))
        .flatten()
        .filter(|_| routing.pointer)
        .and_then(|t| Some((t.view, t.cursor?, t.size)));
    let Some((view, cursor, size)) = under else {
        *cast = None;
        hover.set_if_neq(PlaceHover(None));
        return;
    };
    let Ok(camera) = cameras.get(view) else {
        return;
    };
    let key = (view, cursor, *camera.0);
    if *cast == Some(key) {
        return;
    }
    *cast = Some(key);
    let at = surface_at(&mut scene, camera, size, cursor).map(|(p, n, _)| (p, n));
    hover.set_if_neq(PlaceHover(at));
}
