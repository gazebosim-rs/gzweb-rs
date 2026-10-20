//! The Scene panel through the shell: its body follows the connection,
//! its header counts the entities and has its (disabled) actions.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_core::transport::Status;

use super::{COLLAPSE_ALL, EXPAND_ALL, NEW_WORLD, OPEN_WORLD, SCENE};
use crate::panels::{ActionId, PanelBody, PanelView, RECONNECT};
use crate::shell::sidebar::body::{ActionButton, BodyView, SkeletonRow};
use crate::test_support::frames;
use crate::test_support::shell::*;

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    let (body, view) = world
        .query::<(&PanelBody, &BodyView)>()
        .single(world)
        .unwrap();
    assert_eq!(body.0, SCENE);
    view.0.clone()
}

/// The Scene panel's buttons: (action, enabled), by name.
fn actions(app: &mut App) -> Vec<(ActionId, bool)> {
    let world = app.world_mut();
    let mut out: Vec<(ActionId, bool)> = world
        .query::<(&ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .filter(|(b, _)| b.panel == SCENE)
        .map(|(b, d)| (b.action, !d))
        .collect();
    out.sort();
    out
}

#[test]
fn the_scene_panel_follows_the_connection() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    // No world: the world actions, disabled until the World menu (M8);
    // Expand all and Collapse all, disabled until the tree (M4).
    assert!(all_text(&mut app).contains("No world loaded"));
    assert_eq!(
        actions(&mut app),
        [
            (COLLAPSE_ALL, false),
            (EXPAND_ALL, false),
            (NEW_WORLD, false),
            (OPEN_WORLD, false)
        ]
    );
    // Skeleton rows while the first snapshot arrives.
    app.world_mut().resource_mut::<GzStatus>().status = Status::Connected;
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::Loading);
    let world = app.world_mut();
    assert_eq!(world.query::<&SkeletonRow>().iter(world).count(), 3);
    // Live: its content and the entity count.
    go_live(&mut app, false);
    assert_eq!(view(&mut app), PanelView::Ready);
    assert!(all_text(&mut app).contains("0 entities"));
    // Lost: the error, whose Reconnect retries.
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Error("closed".into());
        s.reconnect_at = Some(1e9);
    }
    frames(&mut app, 2);
    assert!(matches!(view(&mut app), PanelView::Error { .. }));
    let text = all_text(&mut app);
    assert!(text.contains("Lost connection to server") && !text.contains("0 entities"));
    let reconnect = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActionButton)>()
            .iter(world)
            .find(|(_, b)| b.action == RECONNECT)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: reconnect });
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
}
