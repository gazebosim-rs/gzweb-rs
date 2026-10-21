//! Pointer and key routing (spec §3.4, §9.3; DECISIONS.md D-32): the
//! camera gets the pointer only over the view's node and not over any UI
//! node; the view keeps the keys after a press in it, and loses them to a
//! press elsewhere or a widget taking keyboard focus. The view's own
//! controls (the rail, the cube, the target chip) block the camera's
//! pointer like any UI, but a press on them or keyboard focus in them keeps
//! the view's keys (§3.4: the active view is the one that got the last
//! press, "including on its rail or cube"; shortcuts.json: viewport keys
//! fire while "viewport or its rail has focus").

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
pub(crate) struct ViewFocus(pub bool);

impl Default for ViewFocus {
    fn default() -> Self {
        ViewFocus(true)
    }
}

/// Where the pointer is: over the view's node, and over a UI node.
#[derive(SystemParam)]
pub(crate) struct PointerOver<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    views: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<ViewNode>>,
    nodes: Query<'w, 's, (), (With<ComputedNode>, Without<ViewNode>)>,
    view_nodes: Query<'w, 's, (), With<ViewNode>>,
    parents: Query<'w, 's, &'static ChildOf>,
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

    /// The UI nodes under the pointer (bevy_picking's hover map).
    fn hits(&self) -> impl Iterator<Item = Entity> + '_ {
        self.hover
            .as_ref()
            .and_then(|h| h.get(&PointerId::Mouse))
            .into_iter()
            .flat_map(|hits| hits.keys().copied())
    }

    /// Over a UI node other than the view's.
    fn over_ui(&self) -> bool {
        self.hits().any(|e| self.is_ui(e))
    }

    /// Over one of the view's own controls (rail, cube, chip).
    fn over_view_controls(&self) -> bool {
        self.hits().any(|e| self.is_view_control(e))
    }

    /// `e` is a UI node other than the view's.
    fn is_ui(&self, e: Entity) -> bool {
        self.nodes.contains(e)
    }

    /// `e` is one of the view's own controls: inside the view's node.
    fn is_view_control(&self, e: Entity) -> bool {
        self.parents
            .iter_ancestors(e)
            .any(|a| self.view_nodes.contains(a))
    }
}

/// Pointer and key routing (spec §3.4, §9.3): the camera gets the pointer
/// only over the view's node and not over any UI node; the view keeps the
/// keys after a press in it, and loses them to a press elsewhere.
pub(crate) fn route_pointer(
    over: PointerOver,
    buttons: Res<ButtonInput<MouseButton>>,
    focus: Option<Res<InputFocus>>,
    mut view_focus: ResMut<ViewFocus>,
    mut routing: ResMut<ViewRouting>,
) {
    let pointer = over.in_view() && !over.over_ui();
    if buttons.get_just_pressed().next().is_some() {
        view_focus.0 = pointer || over.over_view_controls();
    }
    // A widget that takes keyboard focus takes the keys too (the window
    // itself holds the focus at start), unless it is one of the view's.
    if let Some(e) = focus.filter(|f| f.is_changed()).and_then(|f| f.get()) {
        if over.is_ui(e) {
            view_focus.0 = over.is_view_control(e);
        }
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
