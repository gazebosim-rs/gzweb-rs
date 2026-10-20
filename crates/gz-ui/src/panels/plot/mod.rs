//! The Plot panel (spec §3.3; M7): charts of fields from Topics or the
//! inspector. Its activity button counts the active charts.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::icons::IconId;
use crate::shell::ShellSystems;

/// The Plot panel's id.
pub const PLOT: PanelId = PanelId::new("plot");

/// The active charts (M7 keeps it); the activity badge counts them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlotCharts(pub u32);

/// Registers the Plot panel and its badge.
pub struct PlotPanelPlugin;

impl Plugin for PlotPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(PLOT, "Plot", IconId::ActivityPlot, DATA_GROUP, 30)
                .binding("panel.plot")
                .view(view::state),
        )
        .init_resource::<PlotCharts>()
        .add_systems(Update, view::chart_badge.in_set(ShellSystems::PanelStatus));
    }
}

#[cfg(test)]
mod tests;
