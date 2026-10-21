//! The Topics panel's view, written into the shell's [`PanelStatus`].

use bevy::prelude::*;

use super::TOPICS;
use crate::panels::PanelView;
use crate::shell::{CurrentPhase, PanelStatus, Phase, SetPanelStatus};

/// "No topics yet" once live, skeleton rows while loading, else "Not
/// connected" (§3.3.1). Ready once it lists topics (M7).
pub(super) fn view(phase: Phase) -> PanelView {
    match phase {
        Phase::Live => PanelView::empty("No topics yet"),
        Phase::Loading => PanelView::Loading,
        _ => PanelView::empty("Not connected"),
    }
}

/// Writes the view as the connection changes.
pub(super) fn topics_view(phase: Res<CurrentPhase>, mut status: ResMut<PanelStatus>) {
    if phase.is_changed() {
        status.set_view(TOPICS, view(phase.0));
    }
}
