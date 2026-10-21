//! The Plot panel (spec §3.3; M7): charts of fields from Topics or the
//! inspector. Its empty state offers Add field… (disabled until M7); its
//! activity button counts the active charts.

mod systems;

use bevy::prelude::*;

use super::{ActionId, PanelAction, PanelDef, PanelId, PanelView, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::ShellSystems;

/// The Plot panel's id.
pub const PLOT: PanelId = PanelId::new("plot");
/// Its empty state's Add field… (M7).
pub(crate) const ADD_FIELD: ActionId = ActionId::new("add_field");

/// The active charts (M7 keeps it); the activity badge counts them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlotCharts(pub u32);

/// Registers the Plot panel and its badge.
pub struct PlotPanelPlugin;

impl Plugin for PlotPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(PLOT, "Plot", IconId::ActivityPlot)
                .place(DATA_GROUP, 30)
                .binding("panel.plot")
                .view(
                    PanelView::empty("Add a field from Topics or the inspector")
                        .action(PanelAction::new(ADD_FIELD, "Add field…").disabled()),
                ),
        )
        .init_resource::<PlotCharts>()
        .add_systems(Update, systems::chart_badge.in_set(ShellSystems::Panels));
    }
}

#[cfg(test)]
mod tests;
