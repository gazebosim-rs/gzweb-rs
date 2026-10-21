//! Camera modes (spec §7.2, §3.4 target chip; DECISIONS.md D-60): the
//! Gazebo GUI's camera tracking, as gzweb's `follow` does it.
//!
//! - **Free:** the camera goes where the user moves it.
//! - **Follow:** the camera keeps its offset from the target as the target
//!   moves (its translation only, so the view doesn't spin with a turning
//!   robot); orbit, pan and zoom work as usual around it.
//! - **Track:** the camera stays where it is and keeps turning to look at
//!   the target, which is the pivot.
//!
//! Starting either turns the view to the target (animated, keeping the
//! distance) and makes it the pivot. Home frees the camera; Esc never
//! does (spec §6.3). A target that goes away frees it too.

use bevy::ecs::system::ParamSet;
use bevy::prelude::*;
use bevy::transform::helper::TransformHelper;

use super::{ortho_height, start, Pose, ViewCamera, ViewScene, MIN_DISTANCE};

/// What a view's camera does by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    #[default]
    Free,
    /// Move with this entity.
    Follow(Entity),
    /// Keep looking at this entity.
    Track(Entity),
}

impl CameraMode {
    /// The followed or tracked entity.
    pub fn target(self) -> Option<Entity> {
        match self {
            CameraMode::Free => None,
            CameraMode::Follow(e) | CameraMode::Track(e) => Some(e),
        }
    }
}

/// Enters `mode`: for a target, animates to look at it from where the
/// camera is (keeping the distance) with the target as the pivot. A target
/// that isn't in the scene leaves the camera free.
pub(super) fn set_mode(
    view: &mut ViewCamera,
    t: &Transform,
    p: &Projection,
    mode: CameraMode,
    scene: &ViewScene,
) {
    view.follow_last = None;
    let Some(at) = mode.target().and_then(|e| scene.position(e)) else {
        view.mode = CameraMode::Free;
        return;
    };
    view.mode = mode;
    view.follow_last = Some(at);
    view.snapped = None;
    let from = Pose::of(view, t, p);
    let distance = (t.translation - at).length().max(MIN_DISTANCE);
    let look = Transform::from_translation(t.translation).looking_at(at, up_for(t, at));
    let to = Pose {
        pivot: at,
        rotation: look.rotation,
        offset: Vec3::new(0.0, 0.0, -distance),
        ortho_height: ortho_height(p).unwrap_or(0.0),
    };
    start(view, from, to, None);
}

/// World up, unless the target is straight above or below the camera:
/// then the camera's own up.
fn up_for(t: &Transform, at: Vec3) -> Vec3 {
    let dir = (at - t.translation).normalize_or_zero();
    if dir.cross(Vec3::Y).length_squared() < 1e-8 {
        t.rotation * Vec3::Y
    } else {
        Vec3::Y
    }
}

/// The views (to read), the targets' placements, and the views (to move).
type ModeViews<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'static, 'static, (Entity, &'static ViewCamera)>,
        TransformHelper<'static, 'static>,
        Query<'static, 'static, (&'static mut ViewCamera, &'static mut Transform)>,
    ),
>;

/// Moves following views with their targets and turns tracking views to
/// theirs, every frame. Runs after the views' commands and the scene's
/// poses, with the targets' up-to-date world positions.
pub fn follow_targets(mut set: ModeViews) {
    let wanted: Vec<(Entity, Entity)> = set
        .p0()
        .iter()
        .filter_map(|(v, cam)| Some((v, cam.mode.target()?)))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let found: Vec<(Entity, Option<Vec3>)> = {
        let helper = set.p1();
        wanted
            .iter()
            .map(|(v, target)| {
                (
                    *v,
                    helper
                        .compute_global_transform(*target)
                        .ok()
                        .map(|g| g.translation()),
                )
            })
            .collect()
    };
    let mut views = set.p2();
    for (v, at) in found {
        let Ok((mut view, mut t)) = views.get_mut(v) else {
            continue;
        };
        let Some(at) = at else {
            view.mode = CameraMode::Free;
            view.follow_last = None;
            continue;
        };
        match view.mode {
            CameraMode::Follow(_) => follow(&mut view, &mut t, at),
            CameraMode::Track(_) => track(&mut view, &mut t, at),
            CameraMode::Free => {}
        }
    }
}

/// Moves camera and pivot (and a running animation) by how far the target
/// moved.
fn follow(view: &mut Mut<ViewCamera>, t: &mut Mut<Transform>, at: Vec3) {
    let last = view.follow_last.replace(at);
    let Some(delta) = last.map(|l| at - l).filter(|d| *d != Vec3::ZERO) else {
        return;
    };
    t.translation += delta;
    view.pivot += delta;
    if let Some(anim) = view.animation.as_mut() {
        anim.from.pivot += delta;
        anim.to.pivot += delta;
    }
}

/// Turns the camera, where it stands, to look at the target (the pivot).
/// Waits for the start's animation.
fn track(view: &mut Mut<ViewCamera>, t: &mut Mut<Transform>, at: Vec3) {
    view.follow_last = Some(at);
    if view.animating() || (at - t.translation).length() < MIN_DISTANCE {
        return;
    }
    let rotation = Transform::from_translation(t.translation)
        .looking_at(at, up_for(t, at))
        .rotation;
    if view.pivot != at {
        view.pivot = at;
    }
    if t.rotation.angle_between(rotation) > 1e-6 {
        t.rotation = rotation;
        view.snapped = None;
    }
}
