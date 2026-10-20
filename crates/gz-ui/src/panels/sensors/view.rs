//! The Sensors panel's body.

use crate::panels::PanelView;
use crate::shell::Phase;

/// It shows the selection's sensors: none until there is one (M5).
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::empty("Select a model with sensors")
}
