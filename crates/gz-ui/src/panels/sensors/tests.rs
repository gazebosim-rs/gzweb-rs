use super::view::state;
use crate::panels::PanelView;
use crate::shell::connection::Phase::*;

#[test]
fn the_sensors_panel_says_what_it_will_show() {
    for phase in [Idle, Connecting, Loading, Live, Lost { since: 0.0 }] {
        assert_eq!(
            state(phase),
            PanelView::empty("Select a model with sensors"),
            "{phase:?}"
        );
    }
}
