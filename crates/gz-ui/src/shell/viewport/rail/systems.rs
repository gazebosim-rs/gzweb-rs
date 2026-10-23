//! The rail's behaviour: its controls send view and overlay commands to
//! their view, and show what `gz-bevy` says that view is doing (drag
//! action, projection, camera mode, overlays), however that changed (keys,
//! the cube, a menu). Each view has its own rail.

use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::ui::{Checked, ComputedNode, InteractionDisabled};
use bevy::ui_widgets::{Activate, MenuItem, ValueChange};
use gz_bevy::overlays::{OverlaySet, Overlays};
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
use crate::shell::viewport::ViewCameras;
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
    views: ViewCameras,
    mut out: MessageWriter<ViewCommand>,
) {
    if let (Ok(choice), Some((e, ..))) = (choices.get(ev.value), views.of(ev.value)) {
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
    views: ViewCameras,
    mut out: MessageWriter<ViewCommand>,
) {
    if let (Ok(choice), Some((e, ..))) = (choices.get(ev.entity), views.of(ev.entity)) {
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
    views: ViewCameras,
    time: Res<Time<Real>>,
    mut last_focus: Local<Option<f64>>,
    mut out: MessageWriter<ViewCommand>,
) {
    let (Ok(button), Some((e, ..))) = (buttons.get(ev.entity), views.of(ev.entity)) else {
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
    views: ViewCameras,
    choices: Query<(
        Entity,
        &DragChoice,
        Has<Checked>,
        Has<Active>,
        Has<MenuItem>,
    )>,
    mut compact: Query<
        (Entity, &Children, &mut Tooltip, &mut AccessibilityNode),
        With<CompactDragButton>,
    >,
    mut icons: Query<&mut Icon>,
    mut commands: Commands,
) {
    let action = |e| {
        views
            .of(e)
            .map_or(DragAction::Orbit, |(_, v, ..)| v.drag_action)
    };
    for (e, choice, checked, active, item) in &choices {
        let want = choice.0 == action(e);
        if item {
            mark::<Active>(&mut commands, e, active, want);
        } else {
            mark::<Checked>(&mut commands, e, checked, want);
        }
    }
    for (button, children, mut tip, mut access) in &mut compact {
        let (name, id, detail) = drag_look(action(button));
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
#[allow(clippy::type_complexity)]
pub(crate) fn compact_rail(
    theme: Res<Theme>,
    views: ViewCameras,
    sizes: Query<&ComputedNode>,
    mut full: Query<(Entity, &mut Node), (With<DragGroup>, Without<CompactDrag>)>,
    mut compact: Query<(Entity, &mut Node), (With<CompactDrag>, Without<DragGroup>)>,
) {
    let small = |e| {
        let h = views
            .node_of(e)
            .and_then(|n| sizes.get(n).ok())
            .map_or(0.0, |n| n.size().y * n.inverse_scale_factor());
        // Not laid out yet: keep the full rail.
        h > 0.0 && h < theme.s.rail_compact_below_view_h
    };
    for (e, mut n) in &mut full {
        set_shown(&mut n, !small(e));
    }
    for (e, mut n) in &mut compact {
        set_shown(&mut n, small(e));
    }
}

/// The projection button's icon, look and name follow the projection.
#[allow(clippy::type_complexity)]
pub(crate) fn projection_state(
    views: ViewCameras,
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
    for (e, which, children, mut tip, mut access, active) in &mut buttons {
        if *which != RailButton::Projection {
            continue;
        }
        let ortho = views
            .of(e)
            .is_some_and(|(_, _, _, p)| ortho_height(p).is_some());
        let (id, title) = if ortho {
            (IconId::ProjOrtho, "Orthographic")
        } else {
            (IconId::ProjPerspective, "Perspective")
        };
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
    views: ViewCameras<'w, 's>,
    selection: Option<Res<'w, Selection>>,
    names: Query<'w, 's, (Option<&'static GzEntity>, Option<&'static Name>)>,
    map: Option<Res<'w, InputMap>>,
}

impl CameraReadout<'_, '_> {
    /// The camera mode of the view `ui` is in.
    fn mode(&self, ui: Entity) -> CameraMode {
        self.views
            .of(ui)
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
        self.selection.as_ref().and_then(|s| s.first_target())
    }

    /// What Follow or Track names: the camera's target while it is the
    /// current mode (choosing it then changes nothing), else the
    /// selection, else the target.
    fn subject(&self, mode: CameraMode, current: bool) -> Option<String> {
        let target = mode.target();
        let e = if current {
            target
        } else {
            self.selected().or(target)
        };
        e.map(|e| entity_name(&self.names, e))
    }
}

/// The Camera mode button (on, with its dot, when not Free) and the
/// Camera menu's items: the mode checked, Follow and Track named after
/// their subject and enabled with a selection or while in that mode.
#[allow(clippy::type_complexity)]
pub(crate) fn camera_state(
    readout: CameraReadout,
    mut buttons: Query<(Entity, &mut Tooltip, Has<Active>), With<CameraModeButton>>,
    mut dots: Query<(Entity, &mut Node), With<CameraModeDot>>,
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
    let look = |mode| match mode {
        CameraMode::Free => ("Free", false),
        CameraMode::Follow(_) => ("Following", true),
        CameraMode::Track(_) => ("Tracking", true),
    };
    for (e, mut tip, active) in &mut buttons {
        let (name, on) = look(readout.mode(e));
        mark::<Active>(&mut commands, e, active, on);
        if tip.detail.as_deref() != Some(name) {
            tip.detail = Some(name.into());
        }
    }
    for (e, mut dot) in &mut dots {
        set_shown(&mut dot, look(readout.mode(e)).1);
    }
    let selected = readout.selected().is_some();
    for (e, item, children, active, disabled, tip) in &mut items {
        let mode = readout.mode(e);
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
        let subject = readout.subject(mode, current);
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

/// The overlays shown in the view `ui` is in: its own set, else (no
/// views keeping sets) what any view shows.
pub(crate) fn view_overlays(
    views: &ViewCameras,
    sets: &Query<&OverlaySet>,
    any: Option<&Overlays>,
    ui: Entity,
) -> Overlays {
    views
        .of(ui)
        .and_then(|(camera, ..)| sets.get(camera).ok())
        .map_or_else(|| any.copied().unwrap_or_default(), |s| s.0)
}

/// Each rail's Overlays badge counts what is on in its view (neutral
/// `text_2`, hidden at none); its menu's checkboxes show it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn overlay_states(
    overlays: Option<Res<Overlays>>,
    views: ViewCameras,
    sets: Query<&OverlaySet>,
    changed: Query<(), Changed<OverlaySet>>,
    mut badges: Query<(Entity, &mut Node, &Children), With<OverlaysBadge>>,
    mut texts: Query<&mut Text>,
    items: Query<(Entity, &OverlayItem, Has<Checked>)>,
    added: Query<(), Added<OverlayItem>>,
    mut commands: Commands,
) {
    let any = overlays.as_deref();
    if !overlays.as_ref().is_some_and(|o| o.is_changed()) && changed.is_empty() && added.is_empty()
    {
        return;
    }
    for (e, mut node, children) in &mut badges {
        let count = view_overlays(&views, &sets, any, e).count();
        set_shown(&mut node, count > 0);
        for &c in children {
            if let Ok(mut t) = texts.get_mut(c) {
                set_text(&mut t, &count.to_string());
            }
        }
    }
    for (e, item, checked) in &items {
        let on = view_overlays(&views, &sets, any, e).is_on(item.0);
        mark::<Checked>(&mut commands, e, checked, on);
    }
}
