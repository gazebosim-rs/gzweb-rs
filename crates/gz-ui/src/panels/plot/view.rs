//! The Plot panel's body.

use crate::panels::{PanelAction, PanelView};
use crate::shell::activity_bar::ActivityBadges;
use crate::shell::connection::Phase;
use crate::widgets::BadgeKind;

/// Fields come from Topics or the inspector: the empty state offers Add
/// field (disabled until M7).
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::Empty {
        message: "Add a field from Topics or the inspector",
        actions: vec![PanelAction::AddField],
    }
}

/// The activity badge: the active charts, a neutral count.
pub(super) fn badge(badges: &ActivityBadges) -> Option<(BadgeKind, u32)> {
    (badges.plot > 0).then_some((BadgeKind::Count, badges.plot))
}
