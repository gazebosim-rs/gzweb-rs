//! The Teleop panel's body.

use crate::panels::PanelView;
use crate::shell::connection::Phase;

/// It drives the selection: none until there is one (M5).
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::empty("Select a robot to drive")
}
