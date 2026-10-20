//! The Plot panel (spec §3.3; M7): charts of fields from Topics or the
//! inspector. Its activity button counts the active charts.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Plot panel.
pub struct PlotPanelPlugin;

impl Plugin for PlotPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Plot, "Plot", IconId::ActivityPlot, 30)
                .binding("panel.plot")
                .view(view::state)
                .badge(view::badge),
        );
    }
}

#[cfg(test)]
mod tests;
