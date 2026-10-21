//! The view cube's pointer handling (hover, click, drag) and its face
//! labels.

use bevy::picking::events::{PointerClick, PointerDrag, PointerDragStart, PointerMove, PointerOut};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use gz_bevy::view::{ViewCommand, ViewOp};

use super::pick::{self, CubeRegion};
use super::{CubeLabel, ViewCube};
use crate::assets::fonts::text;
use crate::shell::viewport::MainView;
use crate::theme::{ColorRole, FontFamily, Theme, TypeStyle};
use crate::widgets::layout::node_rect;

/// The region under the pointer, lit up.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CubeHover(pub Option<CubeRegion>);

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
    view: MainView<'w, 's>,
}

impl CubePointer<'_, '_> {
    /// The region at `at` on cube `e`, and the view to act on.
    fn region(&self, e: Entity, at: Vec2) -> Option<(Entity, Option<CubeRegion>)> {
        let (node, t) = self.cubes.get(e).ok()?;
        let (view, _, transform, _) = self.view.get()?;
        let scale = self.windows.single().map_or(1.0, Window::scale_factor);
        let uv = cube_uv(at, node_rect(node, t), scale);
        Some((view, pick::pick(pick::gz_rotation(transform.rotation), uv)))
    }
}

/// Hovering lights up what a click would pick.
pub(crate) fn on_cube_move(
    ev: On<PointerMove>,
    pointer: CubePointer,
    mut hover: ResMut<CubeHover>,
) {
    if let Some((_, region)) = pointer.region(ev.event_target(), ev.pointer.position) {
        hover.set_if_neq(CubeHover(region));
    }
}

/// Leaving the cube lights nothing.
pub(crate) fn on_cube_out(
    ev: On<PointerOut>,
    cubes: Query<(), With<ViewCube>>,
    mut hover: ResMut<CubeHover>,
) {
    if cubes.contains(ev.event_target()) {
        hover.set_if_neq(CubeHover(None));
    }
}

/// A drag on the cube starts from nothing.
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
    view: MainView,
    mut drag: ResMut<CubeDrag>,
    mut out: MessageWriter<ViewCommand>,
) {
    if !cubes.contains(ev.event_target()) || ev.delta == Vec2::ZERO {
        return;
    }
    drag.0 += ev.delta.length();
    if drag.0 > CLICK_SLOP {
        if let Some((view, ..)) = view.get() {
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
    if let Some((view, Some(region))) = pointer.region(ev.event_target(), ev.pointer.position) {
        out.write(ViewCommand {
            view,
            op: region.op(),
        });
    }
}

/// Gives a new cube its six face labels: UI text over the image, which
/// the pointer passes through.
pub(crate) fn spawn_labels(
    cubes: Query<Entity, Added<ViewCube>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for cube in &cubes {
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
/// the camera.
pub(crate) fn place_labels(
    view: MainView,
    theme: Res<Theme>,
    mut labels: Query<(&CubeLabel, &mut Node)>,
) {
    let Some((_, _, t, _)) = view.get() else {
        return;
    };
    let rot = pick::gz_rotation(t.rotation);
    let size = theme.s.view_cube;
    for (label, mut node) in &mut labels {
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
