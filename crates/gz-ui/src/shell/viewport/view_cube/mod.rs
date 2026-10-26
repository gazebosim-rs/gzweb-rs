//! The view cube (spec §3.4, §5.17; board 03; DECISIONS.md D-61): a 56 px
//! cube at each view's top right, turned as the view's camera is, that
//! snaps the view to what is clicked on it.
//!
//! As §5.17 asks, it is a render-to-texture of a small cube mesh: a camera
//! of its own ([`scene`]) renders the cube, on its own render layer and
//! far from the world, into a texture the node shows as a UI image, sized
//! to the node's physical pixels (so it is sharp at any scale factor and
//! UI scale, like the 3D view's, D-31), with the same texture tinted in
//! `shadow` a pixel lower under it for its drop shadow (the cube's own
//! silhouette, which never draws over it). Its faces are unlit in the
//! `cube_top` / `cube_right` / `cube_left` tokens by where they face on
//! screen, with `cube_edge` edges and the snapped face in `cube_snapped`.
//! Labels (TOP, N, E, …: Gazebo is east-north-up) are UI text placed over
//! the faces that face the camera.
//!
//! The cube camera is a perspective (about the view's own) at a fixed
//! distance, framed so
//! the cube's bounding sphere (its corner view, the board's) fills the
//! node less a 2 px margin; from anywhere else it is smaller, never
//! clipped, and doesn't change size as the view turns.
//!
//! Picking ([`pick`]) works on the image: the pointer's place on it is a
//! ray through the cube in the cube camera's perspective,
//! which lands on a face's middle (snap to that axis view, orthographic),
//! an edge strip (the 45° view) or a corner square. Hovering lights up
//! what a click would pick; a drag on the cube orbits the view; a click
//! animates per the motion tokens (250 ms, cubic-out). It never opens a
//! menu. Its tooltip names the view a click gives ("Top view · 7", "Top
//! front right view"), and "View cube" off its faces. The cube isn't a
//! tab stop: keys 7, 1 and 3 do what it does. To AccessKit it is a named
//! group that says so, with an action for each face's view.

pub(crate) mod pick;
pub(crate) mod scene;
pub(crate) mod systems;

use bevy::a11y::ActionRequest;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::ui::UiSystems;

use crate::theme::Theme;
use crate::GzUiSystems;

/// The cube's image node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ViewCube;

/// The child showing the cube's texture.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CubeImage;

/// The child under [`CubeImage`] showing the same texture tinted in
/// `shadow`, `elevation.cube`'s offset lower: the cube's drop shadow.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CubeShadow;

/// A face's label over the cube.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CubeLabel(pub IVec3);

/// The cube node (§3.4): 56 × 56 at top 10, right 2 of the view (over the
/// rail's middle), no card.
pub(crate) fn view_cube(theme: &Theme) -> impl Bundle {
    let s = &theme.s;
    (
        ViewCube,
        systems::CubeHover::default(),
        systems::CubePointerAt::default(),
        Hovered::default(),
        systems::cube_tooltip(None, None),
        systems::cube_access(),
        Name::new("view cube"),
        Node {
            position_type: PositionType::Absolute,
            top: px(s.viewport_inset),
            right: px(CUBE_RIGHT),
            width: px(s.view_cube),
            height: px(s.view_cube),
            ..default()
        },
    )
}

/// The cube's right inset: centred over the rail (`viewport_inset` +
/// half the rail, less half the cube).
pub(crate) const CUBE_RIGHT: f32 = 2.0;

pub(crate) fn build(app: &mut App) {
    // Winit sends AccessKit's requests; without it (headless) none come.
    app.add_message::<ActionRequest>();
    app.init_resource::<systems::CubeDrag>()
        .add_systems(
            Update,
            (
                systems::spawn_parts,
                scene::spawn_cube_scenes,
                systems::cube_access_shown,
                systems::cube_actions,
            )
                .chain()
                .in_set(GzUiSystems::Behaviour),
        )
        .add_observer(scene::on_cube_removed)
        // The cube turns, and its labels move, in the frame the view does:
        // the camera before transforms propagate, the labels before layout.
        .add_systems(
            PostUpdate,
            (
                scene::turn_cube.before(TransformSystems::Propagate),
                systems::place_labels.before(UiSystems::Layout),
                (
                    scene::size_cube_texture,
                    scene::show_cube_texture,
                    systems::follow_hover,
                    systems::name_hover,
                    scene::paint_cube,
                )
                    .chain()
                    .after(UiSystems::Layout),
            ),
        )
        .add_observer(systems::on_cube_move)
        .add_observer(systems::on_cube_out)
        .add_observer(systems::on_cube_press)
        .add_observer(systems::on_cube_release)
        .add_observer(systems::on_cube_drag_end)
        .add_observer(systems::on_pointer_cancel)
        .add_observer(systems::on_cube_drag_start)
        .add_observer(systems::on_cube_drag)
        .add_observer(systems::on_cube_click);
}

#[cfg(test)]
mod tests;
