//! The Scene panel's view and header count, written into the shell's
//! [`PanelStatus`].

use bevy::prelude::*;
use gz_bevy::spawn::GzIndex;

use super::{NEW_WORLD, OPEN_WORLD, SCENE};
use crate::panels::{PanelAction, PanelView};
use crate::shell::{CurrentPhase, PanelStatus, Phase, SetPanelStatus};

/// Ready once the world is loaded; skeleton rows while the first entity
/// snapshot arrives; the error with Reconnect once it's lost; else "No
/// world loaded" with the world actions (disabled until the World menu,
/// M8).
pub(super) fn view(phase: Phase) -> PanelView {
    match phase {
        Phase::Live => PanelView::Ready,
        Phase::Loading => PanelView::Loading,
        Phase::Lost { .. } => {
            PanelView::error("Lost connection to server").action(PanelAction::reconnect())
        }
        Phase::Idle | Phase::Connecting => PanelView::empty("No world loaded")
            .action(PanelAction::new(OPEN_WORLD, "Open world…").disabled())
            .action(PanelAction::new(NEW_WORLD, "New world").disabled()),
    }
}

/// The header's count: the world's entities once it's loaded.
fn count(phase: Phase, index: Option<&GzIndex>) -> Option<String> {
    match phase {
        Phase::Live => index.map(|i| {
            let n = i.order.len();
            format!("{n} {}", if n == 1 { "entity" } else { "entities" })
        }),
        _ => None,
    }
}

/// Writes the view as the connection changes.
pub(super) fn scene_view(phase: Res<CurrentPhase>, mut status: ResMut<PanelStatus>) {
    if phase.is_changed() {
        status.set_view(SCENE, view(phase.0));
    }
}

/// Writes the count as the connection or the world changes.
pub(super) fn entity_count(
    phase: Res<CurrentPhase>,
    index: Option<Res<GzIndex>>,
    mut status: ResMut<PanelStatus>,
) {
    if phase.is_changed() || index.as_ref().is_some_and(Res::is_changed) {
        status.set_count(SCENE, count(phase.0, index.as_deref()));
    }
}
