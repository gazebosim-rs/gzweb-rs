//! The rail's behaviour: its controls send view and overlay commands, and
//! show what `gz-bevy` says the view is doing (drag action, projection,
//! camera mode, overlays), however that changed (keys, the cube, a menu).

use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::ui::{Checked, ComputedNode, InteractionDisabled};
use bevy::ui_widgets::{Activate, MenuItem, ValueChange};
use gz_bevy::overlays::Overlays;
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{ortho_height, CameraMode, DragAction, Selection, ViewCommand, ViewOp};

use super::menus::{CameraItem, OverlayItem};
use super::{
    drag_look, CameraModeButton, CameraModeDot, CompactDrag, CompactDragButton, DragChoice,
    DragGroup, OverlaysBadge, RailButton,
};
use crate::assets::icons::{Icon, IconId};
use crate::shell::viewport::hud::systems::entity_name;
use crate::shell::viewport::view::ViewNode;
use crate::shell::viewport::MainView;
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::layout::set_shown;
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::Tooltip;

/// Two activations of Focus this close (seconds) are a double-click:
/// home (§3.4).
pub(crate) const DOUBLE_CLICK_SECS: f64 = 0.4;

/// Adds or removes marker `C` on `e` to match `want`.
fn mark<C: Component + Default>(commands: &mut Commands, e: Entity, has: bool, want: bool) {
    if has != want {
        if want {
            commands.entity(e).insert(C::default());
        } else {
            commands.entity(e).remove::<C>();
        }
    }
}

/// Orbit/Pan/Zoom → the view's drag action.
pub(crate) fn on_drag_change(
    ev: On<ValueChange<Entity>>,
    choices: Query<&DragChoice>,
    view: MainView,
    mut out: MessageWriter<ViewCommand>,
) {
    if let (Ok(choice), Some((e, ..))) = (choices.get(ev.value), view.get()) {
        out.write(ViewCommand {
            view: e,
            op: ViewOp::SetDragAction(choice.0),
        });
    }
}

/// The compact flyout's items → the view's drag action.
pub(crate) fn on_drag_item(
    ev: On<Activate>,
    choices: Query<&DragChoice, With<MenuItem>>,
    view: MainView,
    mut out: MessageWriter<ViewCommand>,
) {
    if let (Ok(choice), Some((e, ..))) = (choices.get(ev.entity), view.get()) {
        out.write(ViewCommand {
            view: e,
            op: ViewOp::SetDragAction(choice.0),
        });
    }
}

/// Projection toggles the projection; Focus frames, and a second click
/// soon after goes home.
pub(crate) fn on_rail_button(
    ev: On<Activate>,
    buttons: Query<&RailButton>,
    view: MainView,
    time: Res<Time<Real>>,
    mut last_focus: Local<Option<f64>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let (Ok(button), Some((e, ..))) = (buttons.get(ev.entity), view.get()) else {
        return;
    };
    let op = match button {
        RailButton::Projection => ViewOp::ToggleProjection,
        RailButton::Focus => {
            let now = time.elapsed_secs_f64();
            let double = last_focus.is_some_and(|t| now - t < DOUBLE_CLICK_SECS);
            *last_focus = (!double).then_some(now);
            if double {
                ViewOp::Home
            } else {
                ViewOp::Focus { at: None }
            }
        }
    };
    out.write(ViewCommand { view: e, op });
}

/// The drag action shows as the checked radio button, the compact
/// button's icon and the flyout's current item.
#[allow(clippy::type_complexity)]
pub(crate) fn drag_states(
    view: MainView,
    choices: Query<(
        Entity,
        &DragChoice,
        Has<Checked>,
        Has<Active>,
        Has<MenuItem>,
    )>,
    mut compact: Query<(&Children, &mut Tooltip, &mut AccessibilityNode), With<CompactDragButton>>,
    mut icons: Query<&mut Icon>,
    mut commands: Commands,
) {
    let action = view
        .get()
        .map_or(DragAction::Orbit, |(_, v, ..)| v.drag_action);
    for (e, choice, checked, active, item) in &choices {
        let want = choice.0 == action;
        if item {
            mark::<Active>(&mut commands, e, active, want);
        } else {
            mark::<Checked>(&mut commands, e, checked, want);
        }
    }
    let (name, id, detail) = drag_look(action);
    for (children, mut tip, mut access) in &mut compact {
        for &c in children {
            if let Ok(mut icon) = icons.get_mut(c) {
                if icon.id != id {
                    icon.id = id;
                }
            }
        }
        if tip.title != name {
            tip.title = name.into();
            tip.detail = Some(detail.into());
            access.set_label(name);
        }
    }
}

/// The compact rail (§3.4) in a view shorter than
/// `rail_compact_below_view_h` UI pixels.
pub(crate) fn compact_rail(
    theme: Res<Theme>,
    views: Query<&ComputedNode, With<ViewNode>>,
    mut full: Query<&mut Node, (With<DragGroup>, Without<CompactDrag>)>,
    mut compact: Query<&mut Node, (With<CompactDrag>, Without<DragGroup>)>,
) {
    let Ok(view) = views.single() else { return };
    let h = view.size().y * view.inverse_scale_factor();
    // Not laid out yet: keep the full rail.
    let small = h > 0.0 && h < theme.s.rail_compact_below_view_h;
    for mut n in &mut full {
        set_shown(&mut n, !small);
    }
    for mut n in &mut compact {
        set_shown(&mut n, small);
    }
}

/// The projection button's icon, look and name follow the projection.
#[allow(clippy::type_complexity)]
pub(crate) fn projection_state(
    view: MainView,
    mut buttons: Query<(
        Entity,
        &RailButton,
        &Children,
        &mut Tooltip,
        &mut AccessibilityNode,
        Has<Active>,
    )>,
    mut icons: Query<&mut Icon>,
    mut commands: Commands,
) {
    let ortho = view
        .get()
        .is_some_and(|(_, _, _, p)| ortho_height(p).is_some());
    let (id, title) = if ortho {
        (IconId::ProjOrtho, "Orthographic")
    } else {
        (IconId::ProjPerspective, "Perspective")
    };
    for (e, which, children, mut tip, mut access, active) in &mut buttons {
        if *which != RailButton::Projection {
            continue;
        }
        mark::<Active>(&mut commands, e, active, ortho);
        for &c in children {
            if let Ok(mut icon) = icons.get_mut(c) {
                if icon.id != id {
                    icon.id = id;
                }
            }
        }
        if tip.title != title {
            tip.title = title.into();
            access.set_label(title);
        }
    }
}

/// Reads the camera mode for the Camera mode button and menu.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CameraReadout<'w, 's> {
    view: MainView<'w, 's>,
    selection: Option<Res<'w, Selection>>,
    names: Query<'w, 's, (Option<&'static GzEntity>, Option<&'static Name>)>,
    map: Option<Res<'w, InputMap>>,
}

impl CameraReadout<'_, '_> {
    fn mode(&self) -> CameraMode {
        self.view
            .get()
            .map_or(CameraMode::Free, |(_, v, ..)| v.mode)
    }

    /// The keys of `binding`, as the tooltips print them.
    fn keys(&self, binding: &str) -> String {
        self.map
            .as_deref()
            .and_then(|m| shortcut_label(m, binding))
            .unwrap_or_default()
    }

    fn selected(&self) -> Option<Entity> {
        self.selection
            .as_ref()
            .and_then(|s| s.entities.first().copied())
    }

    /// What Follow and Track name: the selection, else what the camera
    /// follows or tracks.
    fn subject(&self) -> Option<String> {
        self.selected()
            .or(self.mode().target())
            .map(|e| entity_name(&self.names, e))
    }
}

/// The Camera mode button (on, with its dot, when not Free) and the
/// Camera menu's items: the mode checked, Follow and Track named after
/// their subject and enabled with a selection or while in that mode.
#[allow(clippy::type_complexity)]
pub(crate) fn camera_state(
    readout: CameraReadout,
    mut buttons: Query<(Entity, &mut Tooltip, Has<Active>), With<CameraModeButton>>,
    mut dots: Query<&mut Node, With<CameraModeDot>>,
    mut items: Query<
        (
            Entity,
            &CameraItem,
            &Children,
            Has<Active>,
            Has<InteractionDisabled>,
            Option<&mut Tooltip>,
        ),
        Without<CameraModeButton>,
    >,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let mode = readout.mode();
    let (name, on) = match mode {
        CameraMode::Free => ("Free", false),
        CameraMode::Follow(_) => ("Following", true),
        CameraMode::Track(_) => ("Tracking", true),
    };
    for (e, mut tip, active) in &mut buttons {
        mark::<Active>(&mut commands, e, active, on);
        if tip.detail.as_deref() != Some(name) {
            tip.detail = Some(name.into());
        }
    }
    for mut dot in &mut dots {
        set_shown(&mut dot, on);
    }
    let subject = readout.subject();
    let selected = readout.selected().is_some();
    for (e, item, children, active, disabled, tip) in &mut items {
        let (current, verb, key) = match item {
            CameraItem::Free => {
                mark::<Active>(&mut commands, e, active, mode == CameraMode::Free);
                continue;
            }
            CameraItem::Follow => (
                matches!(mode, CameraMode::Follow(_)),
                "Follow",
                readout.keys("view.follow"),
            ),
            CameraItem::Track => (
                matches!(mode, CameraMode::Track(_)),
                "Track",
                readout.keys("view.track"),
            ),
            CameraItem::FollowOffset | CameraItem::FovSpeed => continue,
        };
        mark::<Active>(&mut commands, e, active, current);
        let enabled = selected || current;
        mark::<InteractionDisabled>(&mut commands, e, disabled, !enabled);
        let label = format!("{verb} {}", subject.as_deref().unwrap_or("selection"));
        if let Some(t) = children.iter().find(|c| texts.contains(*c)) {
            if let Ok(mut t) = texts.get_mut(t) {
                set_text(&mut t, &label);
            }
        }
        let detail = (!enabled)
            .then(|| format!("Select a model first, or point at one in the view and press {key}"));
        if let Some(mut tip) = tip.filter(|t| t.detail != detail) {
            tip.detail = detail;
        }
    }
}

/// The Overlays button's badge counts what is on (neutral `text_2`,
/// hidden at none); the menu's checkboxes show it.
#[allow(clippy::type_complexity)]
pub(crate) fn overlay_states(
    overlays: Option<Res<Overlays>>,
    mut badges: Query<(&mut Node, &Children), With<OverlaysBadge>>,
    mut texts: Query<&mut Text>,
    items: Query<(Entity, &OverlayItem, Has<Checked>)>,
    added: Query<(), Added<OverlayItem>>,
    mut commands: Commands,
) {
    let Some(overlays) = overlays else { return };
    if !overlays.is_changed() && added.is_empty() {
        return;
    }
    let count = overlays.count();
    for (mut node, children) in &mut badges {
        set_shown(&mut node, count > 0);
        for &c in children {
            if let Ok(mut t) = texts.get_mut(c) {
                set_text(&mut t, &count.to_string());
            }
        }
    }
    for (e, item, checked) in &items {
        mark::<Checked>(&mut commands, e, checked, overlays.is_on(item.0));
    }
}
