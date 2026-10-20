//! The 3D view in the viewport area (spec §3.4, §9.3, §7.10): its node,
//! the view's placement on it (rendered into a texture the node shows),
//! and the dimming over the last frame while disconnected.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use gz_bevy::view::ViewPlacement;

use crate::shell::connection::CurrentPhase;

/// The 3D view's placeholder node: the main camera renders into its rect.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ViewNode;

/// The last frame shows at 70% while disconnected (§7.10).
pub const DIM_OPACITY: f32 = 0.3;

/// The dimming over the last frame.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DimOverlay;

/// The physical rect of a laid-out node.
pub fn node_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(transform.translation, node.size())
}

/// Places the 3D view on the view node (spec §9.3): its rect, in window
/// logical pixels, goes to `gz-bevy`, which renders the main camera into a
/// texture of the node's physical size in the same frame (after UI layout,
/// before culling); the node shows that texture.
pub fn place_view(
    views: Query<(&ComputedNode, &UiGlobalTransform), With<ViewNode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut placement: ResMut<ViewPlacement>,
) {
    let Ok((node, transform)) = views.single() else {
        return;
    };
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    let r = node_rect(node, transform);
    let rect = (r.width() >= 1.0 && r.height() >= 1.0)
        .then(|| Rect::from_corners(r.min / scale, r.max / scale));
    if placement.rect != rect || placement.scale != scale {
        placement.rect = rect;
        placement.scale = scale;
    }
}

/// The view node shows the camera's texture once it exists, and nothing
/// (the area's `bg`) while no world is loaded.
pub fn show_view_image(
    placement: Res<ViewPlacement>,
    phase: Res<CurrentPhase>,
    mut views: Query<(Entity, Option<&mut ImageNode>, &mut Visibility), With<ViewNode>>,
    mut commands: Commands,
) {
    let shown = if phase.0.no_world() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for (_, _, mut v) in &mut views {
        v.set_if_neq(shown);
    }
    let Some(handle) = &placement.image else {
        return;
    };
    for (e, image, _) in &mut views {
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
