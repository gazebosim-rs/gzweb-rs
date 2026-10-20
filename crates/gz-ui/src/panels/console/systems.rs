//! The Console panel's badge, written into the shell's [`PanelStatus`].

use bevy::prelude::*;

use super::{ConsoleUnread, CONSOLE};
use crate::shell::{PanelStatus, SetPanelStatus};
use crate::widgets::BadgeKind;

/// The activity badge: errors (with the warnings in the count) in
/// `danger`, else warnings in `warn_badge`.
pub(super) fn unread_badge(unread: Res<ConsoleUnread>, mut status: ResMut<PanelStatus>) {
    if !unread.is_changed() {
        return;
    }
    let ConsoleUnread { warnings, errors } = *unread;
    let badge = if errors > 0 {
        Some((BadgeKind::Error, errors + warnings))
    } else if warnings > 0 {
        Some((BadgeKind::Warning, warnings))
    } else {
        None
    };
    status.set_badge(CONSOLE, badge);
}
