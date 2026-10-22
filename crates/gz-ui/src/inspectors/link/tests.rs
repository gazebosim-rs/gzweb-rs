use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::overlays::Overlays;
use gz_bevy::view::Selection;

use crate::inspectors::common::{fixture, EntityAction, PoseFrame};
use crate::inspectors::model::tests::{action, pose, select};
use crate::test_support::frames;
use crate::test_support::shell::*;

#[test]
fn a_link_shows_its_pose_in_the_model_and_its_definition() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.base]);
    assert_eq!(
        pose(&mut app, PoseFrame::Parent),
        ["0.000 m", "0.000 m", "0.100 m", "0.0°", "0.0°", "0.0°"]
    );
    let text = all_text(&mut app);
    for want in [
        "LINK",
        "cart › base",
        "2.5 kg",
        "0.1",
        "0.3",
        "body",
        "visual · box",
        "hull",
        "collision · box",
        "Sensors 1",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
}

#[test]
fn the_link_actions_select_its_model_and_turn_on_overlays() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.base]);
    let inertia = action(&mut app, EntityAction::ShowInertia);
    app.world_mut().trigger(Activate { entity: inertia });
    let collision = action(&mut app, EntityAction::ShowCollision);
    app.world_mut().trigger(Activate { entity: collision });
    frames(&mut app, 2);
    let o = app.world().resource::<Overlays>();
    assert!(o.inertia && o.collisions);
    let model = action(&mut app, EntityAction::SelectModel);
    app.world_mut().trigger(Activate { entity: model });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Selection>().entities, [s.cart]);
}
