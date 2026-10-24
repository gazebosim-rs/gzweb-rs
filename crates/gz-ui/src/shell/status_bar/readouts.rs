//! The status bar's readouts (spec §3.6): the connection's address and
//! dot, physics, frame rate, iterations, the latest system message's chip,
//! the tool hint, the selection and the ground point under the cursor.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::messages::{MessageLevel, SystemMessages};
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::{cursor_on_ground, Selection, ViewId, ViewOrigin};

use super::StatusItem;
use crate::format;
use crate::shell::connection::{dot_role, CurrentPhase};
use crate::shell::Tool;
use crate::theme::{Fill, Theme};
use crate::widgets::text::set_text;
use crate::widgets::{status_chip, StatusKind, Tooltip, TooltipSide};

/// The text of an item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusText(pub StatusItem);

/// The connection dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ConnectionDot;

/// The ground point under the cursor, Gazebo x, y (§3.6 item 9).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorReadout(pub Option<[f32; 2]>);

/// The hint while placing.
pub(crate) const PLACE_HINT: &str = "Click to place · Shift to place several · Esc to stop";

/// What the status bar's texts are read from.
#[derive(SystemParam)]
pub(crate) struct StatusSources<'w, 's> {
    status: Option<Res<'w, GzStatus>>,
    tool: Res<'w, Tool>,
    /// Placing replaces the tool's hint (D-102).
    placing: Option<Res<'w, gz_bevy::view::place::Placing>>,
    cursor: Res<'w, CursorReadout>,
    diagnostics: Option<Res<'w, DiagnosticsStore>>,
    selection: Option<Res<'w, Selection>>,
    names: Query<'w, 's, (Option<&'static GzEntity>, Option<&'static Name>)>,
}

impl StatusSources<'_, '_> {
    /// Whether anything but the frame rate changed since the system's last
    /// run.
    fn changed(&self) -> bool {
        self.tool.is_changed()
            || self.placing.as_ref().is_some_and(Res::is_changed)
            || self.cursor.is_changed()
            || self.status.as_ref().is_some_and(Res::is_changed)
            || self.selection.as_ref().is_some_and(Res::is_changed)
    }

    /// "sel: amr_01", "3 selected" or "sel: —".
    fn selection(&self) -> String {
        let Some(s) = self.selection.as_deref() else {
            return format::selection(0, None);
        };
        let first = s
            .entities
            .first()
            .map(|e| crate::shell::viewport::hud::systems::entity_name(&self.names, *e))
            .or_else(|| s.parts.first().map(|p| p.name.clone()));
        format::selection(s.len(), first.as_deref())
    }

    /// The frame rate, smoothed.
    fn fps(&self) -> Option<f64> {
        self.diagnostics
            .as_ref()
            .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS))
            .and_then(|d| d.smoothed())
    }

    /// The text `item` shows now, if it has one.
    pub(crate) fn text(&self, item: StatusItem) -> Option<String> {
        let status = self.status.as_deref();
        let sim = status.and_then(|s| s.sim);
        Some(match item {
            StatusItem::Connection => status.map_or_else(String::new, |s| format::address(&s.url)),
            StatusItem::Physics => format::physics(None, sim.map(|s| s.step_size)),
            StatusItem::Fps => format::fps(self.fps()),
            StatusItem::Iterations => format::iterations(sim.as_ref()),
            StatusItem::Hint if self.placing.as_ref().is_some_and(|p| p.0) => PLACE_HINT.into(),
            StatusItem::Hint => self.tool.hint().into(),
            StatusItem::Selection => self.selection(),
            StatusItem::Cursor => format::cursor(self.cursor.0),
            StatusItem::Message | StatusItem::Workspace => return None,
        })
    }
}

/// Every item's text, when what it shows changes (the frame rate when its
/// whole number does).
pub(crate) fn status_texts(
    sources: StatusSources,
    mut shown_fps: Local<Option<Option<i64>>>,
    mut texts: Query<(&StatusText, &mut Text)>,
) {
    let whole = sources.fps().map(|f| f.round() as i64);
    let fps_changed = *shown_fps != Some(whole);
    *shown_fps = Some(whole);
    let changed = sources.changed();
    if !changed && !fps_changed {
        return;
    }
    for (which, mut t) in &mut texts {
        if !changed && which.0 != StatusItem::Fps {
            continue;
        }
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
    if !phase.is_changed() && !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let role = dot_role(phase.0, status.as_deref());
    for mut fill in &mut dots {
        fill.set_if_neq(Fill(role));
    }
}

/// The ground point under the cursor while it is over a view.
#[allow(clippy::type_complexity)]
pub(crate) fn cursor_readout(
    windows: Query<&Window, With<PrimaryWindow>>,
    routing: Res<ViewRouting>,
    cameras: Query<
        (
            &Camera,
            Option<&ViewOrigin>,
            &Projection,
            &Transform,
            Option<&ViewId>,
        ),
        With<MainCamera>,
    >,
    mut readout: ResMut<CursorReadout>,
) {
    let at = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .filter(|_| routing.pointer)
        .and_then(|c| {
            let (camera, origin, projection, t, _) = cameras
                .iter()
                .find(|v| v.4.copied() == routing.hovered)
                .or_else(|| cameras.iter().next())?;
            cursor_on_ground(camera, origin, projection, t, c)
        })
        .map(|v| [v.x, v.y]);
    readout.set_if_neq(CursorReadout(at));
}

/// The message chip in item 5's slot.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct MessageChip {
    pub kind: StatusKind,
    pub text: String,
    pub count: String,
}

impl MessageChip {
    /// The chip for the latest warning or error, if there is one (§3.6
    /// item 5: "only when one exists"): its text and how many of its kind
    /// there have been ("2 warnings").
    pub(crate) fn of(messages: &SystemMessages) -> Option<Self> {
        let m = messages.latest_problem()?;
        let (kind, n, one, many) = match m.level {
            MessageLevel::Error => (StatusKind::Error, messages.errors, "error", "errors"),
            _ => (
                StatusKind::Warning,
                messages.warnings,
                "warning",
                "warnings",
            ),
        };
        Some(Self {
            kind,
            text: m.text.clone(),
            count: format!("{n} {}", if n == 1 { one } else { many }),
        })
    }
}

/// How long a transient confirmation shows (§3.6: 3 s).
pub(crate) const NOTE_SECS: f64 = 3.0;

/// A transient confirmation ("Saved warehouse_demo.sdf", §3.6): it takes
/// item 5's slot until `until` (real seconds).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct StatusNote(pub Option<(String, f64)>);

impl StatusNote {
    /// Shows `text` for [`NOTE_SECS`] from `now`.
    pub(crate) fn show(&mut self, text: impl Into<String>, now: f64) {
        self.0 = Some((text.into(), now + NOTE_SECS));
    }
}

/// Item 5: a transient confirmation while one shows, else the latest
/// warning or error as a status chip, its whole text and source in the
/// tooltip (the chip clips it). Rebuilt when it changes; the message tray
/// it opens comes with M8d.
pub(crate) fn message_chip(
    messages: Option<Res<SystemMessages>>,
    time: Res<Time<Real>>,
    mut note: ResMut<StatusNote>,
    theme: Res<Theme>,
    slots: Query<(Entity, &StatusItem)>,
    chips: Query<(Entity, &MessageChip)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    if note.0.as_ref().is_some_and(|(_, until)| now >= *until) {
        note.0 = None;
    }
    let messages_changed = messages.as_ref().is_some_and(|m| m.is_changed());
    if !messages_changed && !note.is_changed() {
        return;
    }
    let shown = note.0.as_ref().map(|(text, _)| MessageChip {
        kind: StatusKind::Done,
        text: text.clone(),
        count: String::new(),
    });
    let want = shown.or_else(|| messages.as_deref().and_then(MessageChip::of));
    let have = chips.iter().next();
    if have.map(|c| c.1) == want.as_ref() {
        return;
    }
    if let Some((e, _)) = have {
        commands.entity(e).despawn();
    }
    let Some(want) = want else { return };
    let Some((slot, _)) = slots.iter().find(|(_, i)| **i == StatusItem::Message) else {
        return;
    };
    let source = match want.kind {
        StatusKind::Done => String::new(),
        _ => messages
            .as_deref()
            .and_then(SystemMessages::latest_problem)
            .map(|m| m.source.clone())
            .unwrap_or_default(),
    };
    let tip = Tooltip::new(want.text.clone())
        .detail(source)
        .side(TooltipSide::Above);
    let count = (!want.count.is_empty()).then(|| want.count.clone());
    commands.entity(slot).with_child((
        status_chip(&theme, want.kind, want.text.clone(), count),
        tip,
        want,
    ));
}
