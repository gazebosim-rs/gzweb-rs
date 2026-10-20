//! The Plot panel's body and badge.

use bevy::prelude::*;

use super::{PlotCharts, PLOT};
use crate::panels::{PanelAction, PanelView};
use crate::shell::PanelStatus;
use crate::shell::Phase;
use crate::widgets::BadgeKind;

/// Fields come from Topics or the inspector: the empty state offers Add
/// field (disabled until M7).
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::Empty {
        message: "Add a field from Topics or the inspector",
        actions: vec![PanelAction::AddField],
    }
}

/// The activity badge: the active charts, a neutral count.
pub(super) fn badge(charts: PlotCharts) -> Option<(BadgeKind, u32)> {
    (charts.0 > 0).then_some((BadgeKind::Count, charts.0))
}

/// Writes the badge into the shell's [`PanelStatus`] when it changes.
pub(super) fn chart_badge(charts: Res<PlotCharts>, mut status: ResMut<PanelStatus>) {
    let want = badge(*charts);
    if status.badge(PLOT) != want {
        status.set_badge(PLOT, want);
    }
}
