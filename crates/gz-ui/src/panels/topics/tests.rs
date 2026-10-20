//! The Topics panel through the shell.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use gz_bevy::connection::GzStatus;
use gz_core::transport::Status;

use super::{PUBLISH, SERVICES, TOPICS};
use crate::panels::PanelView;
use crate::shell::sidebar::body::{ActionButton, BodyView};
use crate::shell::{Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world.query::<&BodyView>().single(world).unwrap().0.clone()
}

/// "Not connected", skeleton rows while loading, "No topics yet" once
/// live; Publish and Services in the header, disabled until M7.
#[test]
fn the_topics_panel_follows_the_connection() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: TOPICS,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::empty("Not connected"));
    let world = app.world_mut();
    let header: Vec<_> = world
        .query::<(&ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .map(|(b, d)| (b.panel, b.action, d))
        .collect();
    assert_eq!(header, [(TOPICS, PUBLISH, true), (TOPICS, SERVICES, true)]);
    app.world_mut().resource_mut::<GzStatus>().status = Status::Connected;
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::Loading);
    go_live(&mut app, false);
    assert_eq!(view(&mut app), PanelView::empty("No topics yet"));
}
