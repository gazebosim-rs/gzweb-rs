//! The Assets panel's body.

use crate::panels::PanelView;
use crate::shell::connection::Phase;

/// Fuel search comes in M7: the empty state says so in every phase.
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::empty("Search Fuel for models and worlds")
}
