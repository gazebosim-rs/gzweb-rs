//! The Plot panel's badge, written into the shell's [`PanelStatus`].

use bevy::prelude::*;

use super::{PlotCharts, PLOT};
use crate::shell::{PanelStatus, SetPanelStatus};
use crate::widgets::BadgeKind;

/// The activity badge: the active charts, a neutral count.
pub(super) fn chart_badge(charts: Res<PlotCharts>, mut status: ResMut<PanelStatus>) {
    if charts.is_changed() {
        let n = charts.0;
        status.set_badge(PLOT, (n > 0).then_some((BadgeKind::Count, n)));
    }
}
