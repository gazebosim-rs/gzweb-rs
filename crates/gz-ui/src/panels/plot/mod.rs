//! The Plot panel (spec §3.3, §3.3.1; DECISIONS.md D-100): charts of
//! numeric fields over sim time, stacked, with a time window, a legend
//! with live values and a crosshair readout.
//!
//! A field is added through the picker (Add field…, the header's +, or
//! Topics' Plot field, which opens the picker on its topic in the second
//! slot): a topic, then one of the numbers its newest message holds
//! ([`gz_core::fields`]). Each series holds its topic in
//! [`GzSubscriptions`](gz_bevy::topics::GzSubscriptions) at
//! [`model::PLOT_RATE`] and records while the panel is closed too; only
//! the view is gated. A sample's time is its message's header stamp, or
//! the simulation clock's; a world reset drops the samples.
//!
//! The panel contract (DECISIONS.md D-58): the charts are
//! [`model::PlotData`], what the user set [`model::PlotState`]; the body
//! is built from them ([`view::build_plot`]) and can be respawned at any
//! time.

mod model;
mod raster;
mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, HeaderAction, PanelAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems};

/// The Plot panel's id.
pub const PLOT: PanelId = PanelId::new("plot");
/// Add field…: its empty state's and footer's, which open the picker.
pub(crate) const ADD_FIELD: ActionId = ActionId::new("add_field");
/// Its header's +: the picker too.
pub(crate) const ADD: ActionId = ActionId::new("add");
/// Its header's Pause, a toggle: the charts stop moving.
pub(crate) const PAUSE: ActionId = ActionId::new("pause");
/// Its footer's Export CSV.
pub(crate) const EXPORT: ActionId = ActionId::new("export_csv");
/// Its footer's Pop out (M7e).
pub(crate) const POP_OUT: ActionId = ActionId::new("pop_out");

/// The active charts; the activity badge counts them.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlotCharts(pub u32);

/// Topics' Plot field: open the picker on this topic's fields, in the
/// second slot.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct PlotPick(pub String);

/// How the panel starts, for a capture (`gzweb --plot TOPIC[:FIELD,…]`):
/// the panel opens in the primary slot, each chart entry becomes a chart
/// of its topic's fields once the server lists the topic, and the picker
/// opens on `pick`'s fields; then the resource is removed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct PlotStart {
    /// A chart each: a topic and its fields' paths.
    pub charts: Vec<(String, Vec<String>)>,
    /// A topic to open the picker on.
    pub pick: Option<String>,
}

/// The Plot panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PlotBody;

/// Registers the Plot panel: its view, badge, series, charts and picker.
pub struct PlotPanelPlugin;

impl Plugin for PlotPanelPlugin {
    fn build(&self, app: &mut App) {
        gz_bevy::topics::TopicsPlugin::ensure(app);
        app.register_panel(
            PanelDef::new(PLOT, "Plot", IconId::ActivityPlot)
                .place(DATA_GROUP, 30)
                .binding("panel.plot")
                .view(systems::view(false, false))
                .header_action(HeaderAction::new(ADD, IconId::Plus, "Add field").disabled())
                .header_action(
                    HeaderAction::new(PAUSE, IconId::Pause, "Pause charts")
                        .toggle(IconId::Play, "Resume charts"),
                )
                .content::<PlotBody>()
                .footer_action(PanelAction::new(ADD_FIELD, "Add field…").disabled())
                .footer_action(PanelAction::new(EXPORT, "Export CSV").disabled())
                .footer_action(PanelAction::new(POP_OUT, "Pop out").disabled()),
        )
        .add_message::<PlotPick>()
        .init_resource::<PlotCharts>()
        .init_resource::<model::PlotData>()
        .init_resource::<model::PlotState>()
        .init_resource::<model::SimClock>()
        .init_resource::<systems::BuiltCharts>()
        .init_resource::<systems::BuiltPicker>()
        .add_systems(
            Update,
            (
                (
                    systems::track_clock,
                    systems::apply_start.after(super::topics::apply_topics_start),
                    systems::read_picks,
                    systems::record,
                    systems::mark_listed,
                    systems::plot_status,
                    systems::hold_series.before(gz_bevy::topics::sync_subscriptions),
                    systems::refresh_topics,
                )
                    .chain(),
                (
                    systems::sync_charts,
                    systems::legend_states,
                    systems::legend_hover,
                    systems::draw_charts,
                    systems::hover_from_cursor,
                    systems::hover_readout,
                    systems::chip_states,
                    systems::sync_picker,
                    systems::picker_filter,
                    systems::sync_picker_rows,
                    systems::picker_values,
                    systems::notice,
                )
                    .chain()
                    .after(systems::plot_status)
                    .run_if(panel_shown(PLOT)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_plot)
        .add_observer(systems::on_window_chip)
        .add_observer(systems::on_action)
        .add_observer(systems::on_remove)
        .add_observer(systems::on_pick_row)
        .add_observer(systems::on_picker_button)
        .add_observer(systems::on_target);
    }
}

#[cfg(test)]
mod tests;
