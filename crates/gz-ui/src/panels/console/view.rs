//! The Console panel's body and badge.

use bevy::prelude::*;

use super::{ConsoleUnread, CONSOLE};
use crate::panels::PanelView;
use crate::shell::PanelStatus;
use crate::shell::Phase;
use crate::widgets::BadgeKind;

/// No messages yet.
pub(super) fn state(_: Phase) -> PanelView {
    PanelView::empty("No messages")
}

/// The activity badge: errors (with the warnings in the count) in
/// `danger`, else warnings in `warn_badge`.
pub(super) fn badge(unread: ConsoleUnread) -> Option<(BadgeKind, u32)> {
    if unread.errors > 0 {
        Some((BadgeKind::Error, unread.errors + unread.warnings))
    } else if unread.warnings > 0 {
        Some((BadgeKind::Warning, unread.warnings))
    } else {
        None
    }
}

/// Writes the badge into the shell's [`PanelStatus`] when it changes.
pub(super) fn unread_badge(unread: Res<ConsoleUnread>, mut status: ResMut<PanelStatus>) {
    let want = badge(*unread);
    if status.badge(CONSOLE) != want {
        status.set_badge(CONSOLE, want);
    }
}
