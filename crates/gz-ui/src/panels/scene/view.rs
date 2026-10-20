//! The Scene panel's body and header count.

use bevy::prelude::*;
use gz_bevy::spawn::GzIndex;

use super::SCENE;
use crate::panels::{PanelAction, PanelView};
use crate::shell::PanelStatus;
use crate::shell::{CurrentPhase, Phase};

/// Ready once the world is loaded; skeleton rows while the first entity
/// snapshot arrives; the error with Reconnect once it's lost; else "No
/// world loaded" with the (disabled) world actions.
pub(super) fn state(phase: Phase) -> PanelView {
    match phase {
        Phase::Live => PanelView::Ready,
        // Skeleton rows while the first entity snapshot arrives.
        Phase::Loading => PanelView::Loading,
        Phase::Lost { .. } => PanelView::Error {
            message: "Lost connection to server",
            actions: vec![PanelAction::Reconnect],
        },
        Phase::Idle | Phase::Connecting => PanelView::Empty {
            message: "No world loaded",
            actions: vec![PanelAction::OpenWorld, PanelAction::NewWorld],
        },
    }
}

/// The header's count: the world's entities once it's loaded.
pub(super) fn count(phase: Phase, index: Option<&GzIndex>) -> Option<String> {
    match phase {
        Phase::Live => index.map(|i| {
            let n = i.order.len();
            format!("{n} {}", if n == 1 { "entity" } else { "entities" })
        }),
        _ => None,
    }
}

/// Writes the count into the shell's [`PanelStatus`] when it changes.
pub(super) fn entity_count(
    phase: Res<CurrentPhase>,
    index: Option<Res<GzIndex>>,
    mut status: ResMut<PanelStatus>,
) {
    if !phase.is_changed() && !index.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let want = count(phase.0, index.as_deref());
    if status.count(SCENE) != want.as_deref() {
        status.set_count(SCENE, want);
    }
}
