//! The transform tools' gizmo in the full UI (spec §6.1): gz-bevy's
//! gizmo and pose edits, the handles in the theme's `axis_x`, `axis_y`
//! and `axis_z` (a hovered or dragged one in `text`), and the snapping
//! readout next to the pointer.

use bevy::prelude::*;
use gz_bevy::view::gizmo::{GizmoState, GizmoStyle};

use crate::assets::text;
use crate::shell::regions::pass_through;
use crate::theme::{radius, ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::layout::display;
use crate::widgets::rounded;
use crate::widgets::text::set_text;

/// The readout's offset from the pointer (logical pixels).
pub(crate) const READOUT_OFFSET: Vec2 = Vec2::new(14.0, 14.0);

/// The snapping readout.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct GizmoReadout;

pub(crate) fn build(app: &mut App) {
    app.add_plugins((gz_bevy::edit::EditPlugin, gz_bevy::undo::UndoPlugin));
    // Without Bevy's gizmos (unit tests) there is nothing to draw with.
    if app.world().contains_resource::<GizmoConfigStore>() {
        app.add_plugins(gz_bevy::view::gizmo::GizmoPlugin);
    } else {
        app.init_resource::<GizmoState>()
            .init_resource::<gz_bevy::view::gizmo::TransformTool>()
            .init_resource::<GizmoStyle>();
    }
    app.add_systems(Update, (gizmo_style, readout));
}

/// A hidden readout on the view's node: mono, on `tooltip`.
pub(crate) fn gizmo_readout() -> impl Bundle {
    (
        GizmoReadout,
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            padding: UiRect::axes(px(SPACE[3]), px(SPACE[1])),
            border_radius: rounded(radius::XS),
            ..default()
        },
        Fill(ColorRole::Tooltip),
        pass_through(),
        children![text(TypeStyle::MONO_SMALL, ColorRole::TooltipText, "")],
    )
}

/// The handles' colours from the theme.
fn gizmo_style(theme: Res<Theme>, style: Option<ResMut<GizmoStyle>>) {
    let Some(mut style) = style else { return };
    if !theme.is_changed() && !style.is_added() {
        return;
    }
    style.set_if_neq(GizmoStyle {
        axes: [
            theme.color(ColorRole::AxisX),
            theme.color(ColorRole::AxisY),
            theme.color(ColorRole::AxisZ),
        ],
        hover: theme.color(ColorRole::Text),
    });
}

/// The readout follows the gizmo's while snapping.
fn readout(
    state: Option<Res<GizmoState>>,
    mut boxes: Query<(&mut Node, &Children), With<GizmoReadout>>,
    mut texts: Query<&mut Text>,
) {
    let Some(state) = state.filter(|s| s.is_changed()) else {
        return;
    };
    for (mut node, children) in &mut boxes {
        let want = display(state.readout.is_some());
        if node.display != want {
            node.display = want;
        }
        if let Some((at, value)) = &state.readout {
            let at = *at + READOUT_OFFSET;
            node.left = px(at.x);
            node.top = px(at.y);
            for c in children {
                if let Ok(mut t) = texts.get_mut(*c) {
                    set_text(&mut t, value);
                }
            }
        }
    }
}
