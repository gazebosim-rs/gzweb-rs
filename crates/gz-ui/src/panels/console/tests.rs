use super::view::{badge, state};
use super::ConsoleUnread;
use crate::panels::PanelView;
use crate::shell::connection::Phase;
use crate::widgets::BadgeKind;

#[test]
fn the_console_badge_counts_errors_then_warnings() {
    assert_eq!(state(Phase::Live), PanelView::empty("No messages"));
    let unread = |warnings, errors| ConsoleUnread { warnings, errors };
    assert_eq!(badge(unread(2, 1)), Some((BadgeKind::Error, 3)));
    assert_eq!(badge(unread(2, 0)), Some((BadgeKind::Warning, 2)));
    assert_eq!(badge(unread(0, 0)), None);
}
