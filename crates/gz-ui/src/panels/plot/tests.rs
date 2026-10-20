use super::view::{badge, state};
use crate::panels::{PanelAction, PanelView};
use crate::shell::connection::Phase;
use crate::shell::ActivityBadges;
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
    assert_eq!(badge(&ActivityBadges::default()), None);
    let b = ActivityBadges {
        plot: 2,
        ..Default::default()
    };
    assert_eq!(badge(&b), Some((BadgeKind::Count, 2)));
}
