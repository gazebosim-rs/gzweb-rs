use super::view::state;
use crate::panels::PanelView;
use crate::shell::connection::Phase::*;

#[test]
fn the_topics_panel_follows_the_connection() {
    assert!(matches!(
        state(Live),
        PanelView::Empty {
            message: "No topics yet",
            ..
        }
    ));
    assert!(matches!(
        state(Lost { since: 0.0 }),
        PanelView::Empty {
            message: "Not connected",
            ..
        }
    ));
    assert_eq!(state(Idle), PanelView::empty("Not connected"));
    assert_eq!(state(Loading), PanelView::Loading);
}
