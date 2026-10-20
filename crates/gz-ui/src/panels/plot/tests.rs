use super::view::{badge, state};
use super::PlotCharts;
use crate::panels::{PanelAction, PanelView};
use crate::shell::connection::Phase;
use crate::widgets::BadgeKind;

#[test]
fn the_plot_panel_offers_a_field_and_counts_charts() {
    assert_eq!(
        state(Phase::Live),
        PanelView::Empty {
            message: "Add a field from Topics or the inspector",
            actions: vec![PanelAction::AddField]
        }
    );
    assert_eq!(badge(PlotCharts(0)), None);
    assert_eq!(badge(PlotCharts(2)), Some((BadgeKind::Count, 2)));
}
