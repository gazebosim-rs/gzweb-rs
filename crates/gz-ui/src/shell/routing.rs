//! Pointer and key routing (spec §3.4, §9.3; DECISIONS.md D-32): the
//! camera gets the pointer only over the view's node and not over any UI
//! node; the view keeps the keys after a press in it, and loses them to a
//! press elsewhere or a widget taking keyboard focus.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use gz_bevy::view::controls::ViewRouting;

use super::viewport::view::ViewNode;
use crate::widgets::layout::node_rect;

/// The view had the last pointer press (its keys apply while the pointer
/// is elsewhere). True at start, so keys work before the first click.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFocus(pub bool);

impl Default for ViewFocus {
    fn default() -> Self {
        ViewFocus(true)
    }
}

/// Pointer and key routing (spec §3.4, §9.3): the camera gets the pointer
/// only over the view's node and not over any UI node; the view keeps the
/// keys after a press in it, and loses them to a press elsewhere.
#[allow(clippy::too_many_arguments)]
pub fn route_pointer(
    windows: Query<&Window, With<PrimaryWindow>>,
    views: Query<(&ComputedNode, &UiGlobalTransform), With<ViewNode>>,
    nodes: Query<(), (With<ComputedNode>, Without<ViewNode>)>,
    hover: Option<Res<HoverMap>>,
    buttons: Res<ButtonInput<MouseButton>>,
    focus: Option<Res<InputFocus>>,
    mut view_focus: ResMut<ViewFocus>,
    mut routing: ResMut<ViewRouting>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.physical_cursor_position());
    let in_view = match (cursor, views.single()) {
        (Some(c), Ok((node, t))) => node_rect(node, t).contains(c),
        _ => false,
    };
    let over_ui = hover
        .as_ref()
        .and_then(|h| h.get(&PointerId::Mouse))
        .is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)));
    let pointer = in_view && !over_ui;
    if buttons.get_just_pressed().next().is_some() {
        view_focus.0 = pointer;
    }
    // A widget that takes keyboard focus takes the keys too (the window
    // itself holds the focus at start).
    if focus.is_some_and(|f| f.is_changed() && f.get().is_some_and(|e| nodes.contains(e))) {
        view_focus.0 = false;
    }
    routing.set_if_neq(ViewRouting {
        pointer,
        keys: pointer || view_focus.0,
    });
}
