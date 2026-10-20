//! The Scene panel's body.

use gz_bevy::spawn::GzIndex;

use crate::panels::{PanelAction, PanelView};
use crate::shell::connection::Phase;

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
