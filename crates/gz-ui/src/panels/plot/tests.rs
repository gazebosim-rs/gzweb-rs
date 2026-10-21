//! The Plot panel through the shell.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::{PlotCharts, ADD_FIELD, PLOT};
use crate::shell::sidebar::body::ActionButton;
use crate::shell::{PanelStatus, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::BadgeKind;

/// Its empty state offers Add field… (disabled until M7), and its badge
/// counts the active charts.
#[test]
fn the_plot_panel_offers_a_field_and_counts_charts() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains("Add a field from Topics or the inspector"));
    let world = app.world_mut();
    let add: Vec<_> = world
        .query::<(&ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .map(|(b, d)| (b.panel, b.action, d))
        .collect();
    assert_eq!(add, [(PLOT, ADD_FIELD, true)]);
    let badge = |app: &App| app.world().resource::<PanelStatus>().badge(PLOT);
    assert_eq!(badge(&app), None);
    app.world_mut().resource_mut::<PlotCharts>().0 = 2;
    frames(&mut app, 1);
    assert_eq!(badge(&app), Some((BadgeKind::Count, 2)));
    app.world_mut().resource_mut::<PlotCharts>().0 = 0;
    frames(&mut app, 1);
    assert_eq!(badge(&app), None);
}
