//! Pointer and key routing (spec §3.4, §9.3; DECISIONS.md D-32, D-91): a
//! camera gets the pointer only over its view's node and not over any UI
//! node; the views keep the keys after a press in one, and lose them to a
//! press elsewhere or a widget taking keyboard focus. The view the pointer
//! is over is the hovered one (its single-key shortcuts go there), and the
//! one that got the last press is the active one. A view's own
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
use gz_bevy::view::ViewId;

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

/// Where the pointer is: over a view's node, and over a UI node.
#[derive(SystemParam)]
pub(crate) struct PointerOver<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    views: Query<
        'w,
        's,
        (
            &'static ViewNode,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    nodes: Query<'w, 's, (), (With<ComputedNode>, Without<ViewNode>)>,
    view_nodes: Query<'w, 's, (), With<ViewNode>>,
    parents: Query<'w, 's, &'static ChildOf>,
    hover: Option<Res<'w, HoverMap>>,
}

impl PointerOver<'_, '_> {
    /// The view whose node's rect the pointer is in (whatever is on top).
    fn view(&self) -> Option<ViewId> {
        let c = self
            .windows
            .single()
            .ok()
            .and_then(|w| w.physical_cursor_position())?;
        self.views
            .iter()
            .find(|(_, node, t)| node_rect(node, t).contains(c))
            .map(|(v, ..)| v.0)
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
    let hovered = over.view();
    let pointer = hovered.is_some() && !over.over_ui();
    let mut active = routing.active;
    if buttons.get_just_pressed().next().is_some() {
        view_focus.0 = pointer || over.over_view_controls();
        // A press in a view, its rail or cube included, makes it active.
        if hovered.is_some() && (pointer || over.over_view_controls()) {
            active = hovered;
        }
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
        hovered,
        active: active.or(Some(ViewId(0))),
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
