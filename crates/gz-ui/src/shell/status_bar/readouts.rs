//! The status bar's readouts (spec §3.6): the connection's address and
//! dot, physics, frame rate, iterations, the tool hint, the selection and
//! the ground point under the cursor.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::system::SystemParam;
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
use crate::widgets::text::set_text;

/// The text of an item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusText(pub StatusItem);

/// The connection dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ConnectionDot;

/// The ground point under the cursor, Gazebo x, y (§3.6 item 9).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorReadout(pub Option<[f32; 2]>);

/// What the status bar's texts are read from.
#[derive(SystemParam)]
pub(crate) struct StatusSources<'w> {
    status: Option<Res<'w, GzStatus>>,
    tool: Res<'w, Tool>,
    cursor: Res<'w, CursorReadout>,
    diagnostics: Option<Res<'w, DiagnosticsStore>>,
}

impl StatusSources<'_> {
    /// The text `item` shows now, if it has one.
    pub(crate) fn text(&self, item: StatusItem) -> Option<String> {
        let status = self.status.as_deref();
        let sim = status.and_then(|s| s.sim);
        Some(match item {
            StatusItem::Connection => status.map_or_else(String::new, |s| format::address(&s.url)),
            StatusItem::Physics => format::physics(None, sim.map(|s| s.step_size)),
            StatusItem::Fps => format::fps(
                self.diagnostics
                    .as_ref()
                    .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS))
                    .and_then(|d| d.smoothed()),
            ),
            StatusItem::Iterations => format::iterations(sim.as_ref()),
            StatusItem::Hint => self.tool.hint().into(),
            StatusItem::Selection => "sel: —".into(),
            StatusItem::Cursor => format::cursor(self.cursor.0),
            StatusItem::Message | StatusItem::Workspace => return None,
        })
    }
}

/// Every item's text.
pub(crate) fn status_texts(sources: StatusSources, mut texts: Query<(&StatusText, &mut Text)>) {
    for (which, mut t) in &mut texts {
        if let Some(want) = sources.text(which.0) {
            set_text(&mut t, &want);
        }
    }
}

/// The connection dot (§3.6 item 1).
pub(crate) fn connection_dot(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    mut dots: Query<&mut Fill, With<ConnectionDot>>,
) {
    let role = dot_role(phase.0, status.as_deref());
    for mut fill in &mut dots {
        fill.set_if_neq(Fill(role));
    }
}

/// The ground point under the cursor while it is over the view.
pub(crate) fn cursor_readout(
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
