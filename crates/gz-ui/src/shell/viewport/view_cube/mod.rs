//! The view cube (spec §3.4, §5.17; board 03; DECISIONS.md D-61): a 56 px
//! cube at the view's top right, turned as the view's camera is, that
//! snaps the view to what is clicked on it.
//!
//! As §5.17 asks, it is a render-to-texture of a small cube mesh: a camera
//! of its own ([`scene`]) renders the cube, on its own render layer and
//! far from the world, into a texture the node shows as a UI image, sized
//! to the node's physical pixels (so it is sharp at any scale factor and
//! UI scale, like the 3D view's, D-31). Its faces are unlit in the
//! `cube_top` / `cube_right` / `cube_left` tokens by where they face on
//! screen, with `cube_edge` edges and the snapped face in `cube_snapped`.
//! Labels (TOP, N, E, …: Gazebo is east-north-up) are UI text placed over
//! the faces that face the camera.
//!
//! Picking ([`pick`]) works on the image: the pointer's place on it is a
//! ray through the cube in the cube camera's orthographic projection,
//! which lands on a face's middle (snap to that axis view, orthographic),
//! an edge strip (the 45° view) or a corner square. Hovering lights up
//! what a click would pick; a drag on the cube orbits the view; a click
//! animates per the motion tokens (250 ms, cubic-out). It never opens a
//! menu. The cube isn't a tab stop: keys 7, 1 and 3 do what it does.

pub(crate) mod pick;
pub(crate) mod scene;
pub(crate) mod systems;

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::theme::Theme;
use crate::GzUiSystems;

/// The cube's image node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ViewCube;

/// A face's label over the cube.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CubeLabel(pub IVec3);

/// The cube node (§3.4): 56 × 56 at top 10, right 2 of the view (over the
/// rail's middle), no card.
pub(crate) fn view_cube(theme: &Theme) -> impl Bundle {
    let s = &theme.s;
    (
        ViewCube,
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
    app.init_resource::<systems::CubeHover>()
        .init_resource::<systems::CubeDrag>()
        .add_systems(
            Startup,
            scene::spawn_cube_scene.run_if(resource_exists::<Assets<StandardMaterial>>),
        )
        .add_systems(Update, systems::spawn_labels.in_set(GzUiSystems::Behaviour))
        .add_systems(
            PostUpdate,
            (
                scene::size_cube_texture,
                scene::turn_cube,
                scene::paint_cube,
                systems::place_labels,
            )
                .chain()
                .after(UiSystems::Layout),
        )
        .add_observer(systems::on_cube_move)
        .add_observer(systems::on_cube_out)
        .add_observer(systems::on_cube_drag_start)
        .add_observer(systems::on_cube_drag)
        .add_observer(systems::on_cube_click);
}

#[cfg(test)]
mod tests;
