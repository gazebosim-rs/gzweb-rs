//! The Console panel's body.

use crate::panels::PanelView;
use crate::shell::activity_bar::ActivityBadges;
use crate::shell::connection::Phase;
use crate::widgets::BadgeKind;

/// No messages yet.
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::empty("No messages")
}

/// The activity badge: errors (with the warnings in the count) in
/// `danger`, else warnings in `warn_badge`.
pub(super) fn badge(badges: &ActivityBadges) -> Option<(BadgeKind, u32)> {
    if badges.console_errors > 0 {
        Some((
            BadgeKind::Error,
            badges.console_errors + badges.console_warnings,
        ))
    } else if badges.console_warnings > 0 {
        Some((BadgeKind::Warning, badges.console_warnings))
    } else {
        None
    }
}
