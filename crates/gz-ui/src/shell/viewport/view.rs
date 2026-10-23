//! The 3D views in the viewport area (spec §3.4, §9.3, §7.10): their
//! nodes, each view's placement on its node (rendered into a texture the
//! node shows), and the dimming over the last frame while disconnected.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use gz_bevy::camera::MainCamera;
use gz_bevy::view::{ViewId, ViewPlacement};

use crate::shell::connection::{CurrentPhase, Phase};
use crate::widgets::layout::{node_rect, set_shown};

/// A 3D view's placeholder node: its camera renders into its rect.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewNode(pub(crate) ViewId);

/// The last frame shows at 70% while disconnected (§7.10).
pub(crate) const DIM_OPACITY: f32 = 0.3;

/// The dimming over the last frame.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DimOverlay;

/// The last frame dims while the connection is lost (§7.10).
pub(crate) fn dim_state(phase: Res<CurrentPhase>, mut dims: Query<&mut Node, With<DimOverlay>>) {
    let lost = matches!(phase.0, Phase::Lost { .. });
    for mut node in &mut dims {
        set_shown(&mut node, lost);
    }
}

/// Places each 3D view on its node (spec §9.3): its rect, in window
/// logical pixels, goes to its camera in `gz-bevy`, which renders it into
/// a texture of the node's physical size in the same frame (after UI
/// layout, before culling); the node shows that texture. A view with no
/// node (the layout dropped it) is placed nowhere, so its camera is off.
pub(crate) fn place_view(
    views: Query<(&ViewNode, &ComputedNode, &UiGlobalTransform)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, &ViewId, Option<&mut ViewPlacement>), With<MainCamera>>,
    mut commands: Commands,
) {
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    for (camera, id, placement) in &mut cameras {
        let rect = views
            .iter()
            .find(|v| v.0 .0 == *id)
            .and_then(|(_, node, t)| {
                let r = node_rect(node, t);
                (r.width() >= 1.0 && r.height() >= 1.0)
                    .then(|| Rect::from_corners(r.min / scale, r.max / scale))
            });
        match placement {
            Some(mut p) => {
                if p.rect != rect || p.scale != scale {
                    p.rect = rect;
                    p.scale = scale;
                }
            }
            None => {
                commands.entity(camera).insert(ViewPlacement {
                    rect,
                    scale,
                    image: None,
                });
            }
        }
    }
}

/// The views (and their dividers) show while a world is loaded; without
/// one the area's `bg` shows, and nothing in the grid takes the pointer.
pub(crate) fn show_views(
    phase: Res<CurrentPhase>,
    mut grids: Query<&mut Visibility, With<super::views::ViewGrid>>,
) {
    let shown = if phase.0.no_world() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut v in &mut grids {
        v.set_if_neq(shown);
    }
}

/// Each view node shows its camera's texture: after placement, so a view
/// node new this frame (a layout change) or a new texture shows at once.
pub(crate) fn show_view_image(
    cameras: Query<(&ViewId, &ViewPlacement), With<MainCamera>>,
    mut views: Query<(Entity, &ViewNode, Option<&mut ImageNode>)>,
    mut commands: Commands,
) {
    for (e, node, image) in &mut views {
        let Some(handle) = cameras
            .iter()
            .find(|(id, _)| **id == node.0)
            .and_then(|(_, p)| p.image.as_ref())
        else {
            continue;
        };
        match image {
            Some(mut image) => {
                if image.image != *handle {
                    image.image = handle.clone();
                }
            }
            None => {
                commands.entity(e).insert(ImageNode::new(handle.clone()));
            }
        }
    }
}
