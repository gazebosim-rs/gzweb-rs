//! Pointer and key routing (spec §3.4, §9.3; DECISIONS.md D-32): the
//! camera gets the pointer only over the view's node and not over any UI
//! node; the view keeps the keys after a press in it, and loses them to a
//! press elsewhere or a widget taking keyboard focus.

use bevy::ecs::system::SystemParam;
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

/// Where the pointer is: over the view's node, and over a UI node.
#[derive(SystemParam)]
pub struct PointerOver<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    views: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<ViewNode>>,
    nodes: Query<'w, 's, (), (With<ComputedNode>, Without<ViewNode>)>,
    hover: Option<Res<'w, HoverMap>>,
}

impl PointerOver<'_, '_> {
    /// Inside the view node's rect (whatever is on top).
    fn in_view(&self) -> bool {
        let cursor = self
            .windows
            .single()
            .ok()
            .and_then(|w| w.physical_cursor_position());
        match (cursor, self.views.single()) {
            (Some(c), Ok((node, t))) => node_rect(node, t).contains(c),
            _ => false,
        }
    }

    /// Over a UI node other than the view's (bevy_picking's hover map).
    fn over_ui(&self) -> bool {
        self.hover
            .as_ref()
            .and_then(|h| h.get(&PointerId::Mouse))
            .is_some_and(|hits| hits.keys().any(|e| self.is_ui(*e)))
    }

    /// `e` is a UI node other than the view's.
    fn is_ui(&self, e: Entity) -> bool {
        self.nodes.contains(e)
    }
}

/// Pointer and key routing (spec §3.4, §9.3): the camera gets the pointer
/// only over the view's node and not over any UI node; the view keeps the
/// keys after a press in it, and loses them to a press elsewhere.
pub fn route_pointer(
    over: PointerOver,
    buttons: Res<ButtonInput<MouseButton>>,
    focus: Option<Res<InputFocus>>,
    mut view_focus: ResMut<ViewFocus>,
    mut routing: ResMut<ViewRouting>,
) {
    let pointer = over.in_view() && !over.over_ui();
    if buttons.get_just_pressed().next().is_some() {
        view_focus.0 = pointer;
    }
    // A widget that takes keyboard focus takes the keys too (the window
    // itself holds the focus at start).
    if focus.is_some_and(|f| f.is_changed() && f.get().is_some_and(|e| over.is_ui(e))) {
        view_focus.0 = false;
    }
    routing.set_if_neq(ViewRouting {
        pointer,
        keys: pointer || view_focus.0,
    });
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<ViewRouting>()
        .init_resource::<ViewFocus>()
        .add_systems(
            PreUpdate,
            route_pointer.after(bevy::picking::PickingSystems::Hover),
        );
}
