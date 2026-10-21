//! The Console panel through the shell.

use bevy::prelude::*;

use super::{ConsoleUnread, CONSOLE};
use crate::shell::PanelStatus;
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::BadgeKind;

/// The badge counts errors (with the warnings) in `danger`, else the
/// warnings, else hides; writing the same badge again changes nothing.
#[test]
fn the_console_badge_counts_errors_then_warnings() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let badge = |app: &mut App, warnings, errors| {
        *app.world_mut().resource_mut::<ConsoleUnread>() = ConsoleUnread { warnings, errors };
        frames(app, 1);
        app.world().resource::<PanelStatus>().badge(CONSOLE)
    };
    assert_eq!(badge(&mut app, 2, 1), Some((BadgeKind::Error, 3)));
    assert_eq!(badge(&mut app, 2, 0), Some((BadgeKind::Warning, 2)));
    assert_eq!(badge(&mut app, 0, 0), None);
    let tick = app.world().resource_ref::<PanelStatus>().last_changed();
    badge(&mut app, 0, 0);
    assert_eq!(
        app.world().resource_ref::<PanelStatus>().last_changed(),
        tick,
        "an unchanged badge isn't written"
    );
}
