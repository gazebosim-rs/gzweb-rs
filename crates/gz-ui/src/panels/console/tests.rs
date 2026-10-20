use super::view::{badge, state};
use crate::panels::PanelView;
use crate::shell::connection::Phase;
use crate::shell::ActivityBadges;
use crate::widgets::BadgeKind;

#[test]
fn the_console_badge_counts_errors_then_warnings() {
    assert_eq!(state(Phase::Live), PanelView::empty("No messages"));
    let b = ActivityBadges {
        plot: 0,
        console_warnings: 2,
        console_errors: 1,
    };
    assert_eq!(badge(&b), Some((BadgeKind::Error, 3)));
    assert_eq!(
        badge(&ActivityBadges {
            console_warnings: 2,
            ..Default::default()
        }),
        Some((BadgeKind::Warning, 2))
    );
    assert_eq!(badge(&ActivityBadges::default()), None);
}
