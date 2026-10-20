//! The status bar's readouts (spec §3.6): the connection's address and
//! dot, physics, frame rate, iterations, the tool hint, the selection and
//! the ground point under the cursor.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::{cursor_on_ground, ViewOrigin};

use super::StatusItem;
use crate::shell::connection::{dot_role, CurrentPhase};
use crate::shell::format;
use crate::shell::Tool;
use crate::theme::Fill;

/// The text of an item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusText(pub StatusItem);

/// The connection dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ConnectionDot;

/// The ground point under the cursor, Gazebo x, y (§3.6 item 9).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorReadout(pub Option<[f32; 2]>);

fn set(text: &mut Text, want: &str) {
    if text.0 != want {
        text.0 = want.to_string();
    }
}

/// Every item's text and the connection dot.
#[allow(clippy::too_many_arguments)]
pub fn status_texts(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    tool: Res<Tool>,
    cursor: Res<CursorReadout>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    mut texts: Query<(&StatusText, &mut Text)>,
    mut dots: Query<&mut Fill, With<ConnectionDot>>,
) {
    let status = status.as_deref();
    let sim = status.and_then(|s| s.sim);
    let fps = diagnostics
        .as_ref()
        .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS))
        .and_then(|d| d.smoothed());
    for (which, mut t) in &mut texts {
        let want = match which.0 {
            StatusItem::Connection => status.map_or_else(String::new, |s| format::address(&s.url)),
            StatusItem::Physics => format::physics(None, sim.map(|s| s.step_size)),
            StatusItem::Fps => format::fps(fps),
            StatusItem::Iterations => format::iterations(sim.as_ref()),
            StatusItem::Hint => tool.hint().into(),
            StatusItem::Selection => "sel: —".into(),
            StatusItem::Cursor => format::cursor(cursor.0),
            StatusItem::Message | StatusItem::Workspace => continue,
        };
        set(&mut t, &want);
    }
    let role = dot_role(phase.0, status);
    for mut fill in &mut dots {
        fill.set_if_neq(Fill(role));
    }
}

/// The ground point under the cursor while it is over the view.
pub fn cursor_readout(
    windows: Query<&Window, With<PrimaryWindow>>,
    routing: Res<ViewRouting>,
    cameras: Query<(&Camera, Option<&ViewOrigin>, &Projection, &Transform), With<MainCamera>>,
    mut readout: ResMut<CursorReadout>,
) {
    let at = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .filter(|_| routing.pointer)
        .and_then(|c| {
            let (camera, origin, projection, t) = cameras.single().ok()?;
            cursor_on_ground(camera, origin, projection, t, c)
        })
        .map(|v| [v.x, v.y]);
    readout.set_if_neq(CursorReadout(at));
}
