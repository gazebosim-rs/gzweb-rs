//! The Topics panel's body.

use crate::panels::PanelView;
use crate::shell::Phase;

/// "No topics yet" once live, skeleton rows while loading, else "Not
/// connected".
pub(super) fn state(phase: Phase) -> PanelView {
    match phase {
        Phase::Live => PanelView::empty("No topics yet"),
        Phase::Loading => PanelView::Loading,
        _ => PanelView::empty("Not connected"),
    }
}
