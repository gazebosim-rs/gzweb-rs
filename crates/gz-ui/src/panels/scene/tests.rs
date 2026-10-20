use gz_bevy::spawn::GzIndex;

use super::view::{count, state};
use crate::panels::{PanelAction, PanelView};
use crate::shell::connection::Phase::*;

#[test]
fn the_scene_panel_follows_the_connection() {
    let lost = Lost { since: 0.0 };
    assert_eq!(state(Live), PanelView::Ready);
    assert!(matches!(
        state(Connecting),
        PanelView::Empty {
            message: "No world loaded",
            ..
        }
    ));
    assert_eq!(state(Loading), PanelView::Loading);
    assert_eq!(
        state(lost),
        PanelView::Error {
            message: "Lost connection to server",
            actions: vec![PanelAction::Reconnect]
        }
    );
    assert_eq!(
        state(Idle),
        PanelView::Empty {
            message: "No world loaded",
            actions: vec![PanelAction::OpenWorld, PanelAction::NewWorld]
        }
    );
    // The header counts the entities once the world is loaded.
    assert_eq!(count(Idle, None), None);
    assert_eq!(count(Live, None), None);
    let index = GzIndex::default();
    assert_eq!(count(Live, Some(&index)).as_deref(), Some("0 entities"));
    assert_eq!(count(Loading, Some(&index)), None);
}
