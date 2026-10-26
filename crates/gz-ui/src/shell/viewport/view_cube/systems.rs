//! The view cube's pointer handling (hover, click, drag), its tooltip,
//! its actions to AccessKit and its face labels.

use accesskit::{Action, ActionData, CustomAction, NodeId, Role};
use bevy::a11y::{AccessibilityNode, ActionRequest};
use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragEnd, PointerDragStart, PointerMove, PointerOut,
    PointerPress, PointerRelease,
};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, Pressed, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{SnapView, ViewCommand, ViewOp};

use super::pick::{self, CubeRegion};
use super::{CubeImage, CubeLabel, CubeShadow, ViewCube};
use crate::assets::fonts::text;
use crate::shell::viewport::ViewCameras;
use crate::theme::{elevation, ColorRole, FontFamily, Theme, TypeStyle};
use crate::widgets::layout::node_rect;
use crate::widgets::{a11y, Tooltip, TooltipSide};

/// The region under the pointer on a cube, lit up (on the cube's node).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CubeHover(pub Option<CubeRegion>);

/// Where the pointer is on a cube's image ([`cube_uv`]) while it is over
/// it, so the hover follows the cube as the view turns under a still
/// pointer (on the cube's node).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CubePointerAt(pub Option<Vec2>);

/// How far the current drag on the cube has gone (logical pixels): a
/// press that moved further than [`CLICK_SLOP`] is an orbit, not a click.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CubeDrag(pub f32);

/// The most a click may move and still pick.
pub(crate) const CLICK_SLOP: f32 = 3.0;

/// The face labels' type: mono 7 (`view_cube_label_px`), weight 500.
pub(crate) fn label_type(theme: &Theme) -> TypeStyle {
    TypeStyle::new(FontFamily::Mono, theme.s.view_cube_label_px, 500)
}

/// The label boxes' size (the text centres in it).
const LABEL_BOX: Vec2 = Vec2::new(30.0, 10.0);

/// Where `at` (window logical pixels) is on the cube's image, -1..1 with
/// y up, from its node's rect (physical) and the window's scale.
pub(crate) fn cube_uv(at: Vec2, rect: Rect, scale: f32) -> Vec2 {
    let p = at * scale - rect.min;
    let size = rect.size().max(Vec2::ONE);
    Vec2::new(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0)
}

/// What the pointer's place on the cube needs: the cube's rect and the
/// window's scale, and the view's camera.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CubePointer<'w, 's> {
    cubes: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<ViewCube>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    views: ViewCameras<'w, 's>,
}

impl CubePointer<'_, '_> {
    /// Where `at` is on cube `e`'s image, the region there, and the view to
    /// act on.
    fn region(&self, e: Entity, at: Vec2) -> Option<(Entity, Vec2, Option<CubeRegion>)> {
        let (node, t) = self.cubes.get(e).ok()?;
        let (view, _, transform, _) = self.views.of(e)?;
        let scale = self.windows.single().map_or(1.0, Window::scale_factor);
        let uv = cube_uv(at, node_rect(node, t), scale);
        Some((
            view,
            uv,
            pick::pick(pick::gz_rotation(transform.rotation), uv),
        ))
    }
}

/// The cube's name, to AccessKit and in its tooltip off its faces.
pub(crate) const CUBE_NAME: &str = "View cube";

/// What the cube does: its tooltip's second line off its faces.
pub(crate) const CUBE_DETAIL: &str = "Click a face, edge or corner to look from it · Drag to orbit";

/// What the cube does, to AccessKit: it isn't a tab stop, so it says
/// which keys do the same, and offers each face's view as an action.
pub(crate) const CUBE_DESCRIPTION: &str =
    "Turns with the view. Click a face, an edge or a corner to look from it, or drag to orbit. \
     The view's keys do the same: 7, 1 and 3 look from the top, front and right, with Shift \
     from the opposite side";

/// The cube to AccessKit: a group named [`CUBE_NAME`], described, with a
/// custom action for each face's view ("Top view", in
/// [`SnapView::ALL`]'s order, which [`cube_actions`] runs). Its images
/// are hidden ([`spawn_parts`]).
pub(crate) fn cube_access() -> AccessibilityNode {
    let mut access = a11y(Role::Group, CUBE_NAME);
    access.set_description(CUBE_DESCRIPTION);
    access.add_action(Action::CustomAction);
    access.set_custom_actions(
        SnapView::ALL
            .iter()
            .zip(0..)
            .map(|(&v, id)| CustomAction {
                id,
                description: snap_title(v).into(),
            })
            .collect::<Vec<_>>(),
    );
    access
}

/// "Top view": what a face's region looks from.
fn snap_title(view: SnapView) -> String {
    format!("{} view", CubeRegion::of_snap(view).name())
}

/// The tooltip for what the pointer is over on a cube: the view a click
/// gives ("Top view" with its key, "Top front right view"), else the
/// cube's name and what it does. Left of the cube, as the rail's are.
pub(crate) fn cube_tooltip(region: Option<CubeRegion>, map: Option<&InputMap>) -> Tooltip {
    let tip = match region {
        Some(r) => {
            let tip = Tooltip::new(format!("{} view", r.name()));
            match (r.snap(), map) {
                (Some(face), Some(map)) => tip.binding(map, pick::snap_binding(face)),
                _ => tip,
            }
        }
        None => Tooltip::new(CUBE_NAME).detail(CUBE_DETAIL),
    };
    tip.side(TooltipSide::Left)
}

/// A cube's tooltip follows its hover: it names what a click there gives.
pub(crate) fn name_hover(
    map: Option<Res<InputMap>>,
    mut cubes: Query<(&CubeHover, &mut Tooltip), Changed<CubeHover>>,
) {
    for (hover, mut tip) in &mut cubes {
        tip.set_if_neq(cube_tooltip(hover.0, map.as_deref()));
    }
}

/// A screen reader's action on a cube (one of [`cube_access`]'s) snaps
/// its view to that face's view, as a click on the face does.
pub(crate) fn cube_actions(
    mut requests: MessageReader<ActionRequest>,
    cubes: Query<Entity, With<ViewCube>>,
    views: ViewCameras,
    mut out: MessageWriter<ViewCommand>,
) {
    for request in requests.read() {
        let (Action::CustomAction, Some(ActionData::CustomAction(id))) =
            (request.action, &request.data)
        else {
            continue;
        };
        let Some(cube) = cubes
            .iter()
            .find(|c| NodeId(c.to_bits()) == request.target_node)
        else {
            continue;
        };
        let face = usize::try_from(*id).ok().and_then(|i| SnapView::ALL.get(i));
        if let (Some(&face), Some((view, ..))) = (face, views.of(cube)) {
            out.write(ViewCommand {
                view,
                op: ViewOp::Snap(face),
            });
        }
    }
}

/// Hovering lights up what a click would pick.
pub(crate) fn on_cube_move(
    ev: On<PointerMove>,
    pointer: CubePointer,
    mut cubes: Query<(&mut CubePointerAt, &mut CubeHover)>,
) {
    let cube = ev.event_target();
    if let Some((_, uv, region)) = pointer.region(cube, ev.pointer.position) {
        if let Ok((mut at, mut hover)) = cubes.get_mut(cube) {
            at.set_if_neq(CubePointerAt(Some(uv)));
            hover.set_if_neq(CubeHover(region));
        }
    }
}

/// Leaving a cube lights nothing on it.
pub(crate) fn on_cube_out(
    ev: On<PointerOut>,
    mut cubes: Query<(&mut CubePointerAt, &mut CubeHover), With<ViewCube>>,
) {
    if let Ok((mut at, mut hover)) = cubes.get_mut(ev.event_target()) {
        at.set_if_neq(CubePointerAt(None));
        hover.set_if_neq(CubeHover(None));
    }
}

/// The hover follows the view as it turns (a key, a snap, a menu) under a
/// pointer that stays put.
pub(crate) fn follow_hover(
    views: ViewCameras,
    mut cubes: Query<(Entity, &CubePointerAt, &mut CubeHover)>,
) {
    for (cube, at, mut hover) in &mut cubes {
        let (Some(uv), Some((_, _, t, _))) = (at.0, views.of(cube)) else {
            continue;
        };
        hover.set_if_neq(CubeHover(pick::pick(pick::gz_rotation(t.rotation), uv)));
    }
}

/// A press on the cube, and a drag from it, start from nothing (a drag
/// released off the cube ends with no click to clear it). Pressed, its
/// tooltip hides until the pointer leaves, as a button's does.
pub(crate) fn on_cube_press(
    ev: On<PointerPress>,
    cubes: Query<(), With<ViewCube>>,
    mut drag: ResMut<CubeDrag>,
    mut commands: Commands,
) {
    if cubes.contains(ev.event_target()) {
        drag.0 = 0.0;
        commands.entity(ev.event_target()).insert(Pressed);
    }
}

/// A release on the cube, or the end of a drag from it, ends its press.
pub(crate) fn on_cube_release(
    ev: On<PointerRelease>,
    cubes: Query<(), (With<ViewCube>, With<Pressed>)>,
    mut commands: Commands,
) {
    if cubes.contains(ev.event_target()) {
        commands.entity(ev.event_target()).remove::<Pressed>();
    }
}

/// See [`on_cube_release`].
pub(crate) fn on_cube_drag_end(
    ev: On<PointerDragEnd>,
    cubes: Query<(), (With<ViewCube>, With<Pressed>)>,
    mut commands: Commands,
) {
    if cubes.contains(ev.event_target()) {
        commands.entity(ev.event_target()).remove::<Pressed>();
    }
}

/// See [`on_cube_press`].
pub(crate) fn on_cube_drag_start(
    ev: On<PointerDragStart>,
    cubes: Query<(), With<ViewCube>>,
    mut drag: ResMut<CubeDrag>,
) {
    if cubes.contains(ev.event_target()) {
        drag.0 = 0.0;
    }
}

/// Dragging on the cube orbits the view (§3.4).
pub(crate) fn on_cube_drag(
    ev: On<PointerDrag>,
    cubes: Query<(), With<ViewCube>>,
    views: ViewCameras,
    mut drag: ResMut<CubeDrag>,
    mut out: MessageWriter<ViewCommand>,
) {
    if !cubes.contains(ev.event_target()) || ev.delta == Vec2::ZERO {
        return;
    }
    drag.0 += ev.delta.length();
    if drag.0 > CLICK_SLOP {
        if let Some((view, ..)) = views.of(ev.event_target()) {
            out.write(ViewCommand {
                view,
                op: ViewOp::Orbit(ev.delta),
            });
        }
    }
}

/// A click snaps the view to the face, edge or corner under it.
pub(crate) fn on_cube_click(
    ev: On<PointerClick>,
    pointer: CubePointer,
    mut drag: ResMut<CubeDrag>,
    mut out: MessageWriter<ViewCommand>,
) {
    let dragged = std::mem::take(&mut drag.0) > CLICK_SLOP;
    if dragged {
        return;
    }
    if let Some((view, _, Some(region))) = pointer.region(ev.event_target(), ev.pointer.position) {
        out.write(ViewCommand {
            view,
            op: region.op(),
        });
    }
}

/// A child filling the cube node, `down` pixels lower, which the pointer
/// passes through. Its image is hidden from AccessKit: the cube speaks
/// for it ([`cube_access`]).
fn layer(down: f32) -> impl Bundle {
    let mut access = AccessibilityNode(accesskit::Node::new(Role::Image));
    access.set_hidden();
    (
        access,
        Node {
            position_type: PositionType::Absolute,
            top: px(down),
            left: px(0.0),
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// Gives a new cube its drop shadow, its image (both shown once the
/// texture is) and its six face labels: UI text over the image. The
/// pointer passes through them all to the cube node.
pub(crate) fn spawn_parts(
    cubes: Query<Entity, Added<ViewCube>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for cube in &cubes {
        commands.spawn((CubeShadow, layer(elevation::CUBE.y), ChildOf(cube)));
        commands.spawn((CubeImage, layer(0.0), ChildOf(cube)));
        for n in CubeRegion::FACES {
            commands.spawn((
                CubeLabel(n),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(LABEL_BOX.x),
                    height: px(LABEL_BOX.y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    display: Display::None,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(cube),
                children![(
                    text(
                        label_type(&theme),
                        ColorRole::CubeLabel,
                        pick::face_label(n)
                    ),
                    Pickable::IGNORE,
                )],
            ));
        }
    }
}

/// How squarely a face must face the camera for its label to show.
const LABEL_FACING: f32 = 0.3;

/// Keeps each label on the middle of its face, shown while the face faces
/// its view's camera.
pub(crate) fn place_labels(
    views: ViewCameras,
    theme: Res<Theme>,
    mut labels: Query<(Entity, &CubeLabel, &mut Node)>,
) {
    let size = theme.s.view_cube;
    for (e, label, mut node) in &mut labels {
        let Some((_, _, t, _)) = views.of(e) else {
            continue;
        };
        let rot = pick::gz_rotation(t.rotation);
        let (at, facing) = pick::label_at(rot, label.0.as_vec3(), size);
        let display = if facing > LABEL_FACING {
            Display::Flex
        } else {
            Display::None
        };
        let (left, top) = (px(at.x - LABEL_BOX.x / 2.0), px(at.y - LABEL_BOX.y / 2.0));
        if node.display != display || node.left != left || node.top != top {
            node.display = display;
            node.left = left;
            node.top = top;
        }
    }
}
