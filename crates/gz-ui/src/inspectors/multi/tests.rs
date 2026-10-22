use bevy::prelude::*;
use gz_bevy::view::{Part, PartKind, Selection};

use super::{summary, SharedPose};
use crate::inspectors::common::fixture;
use crate::inspectors::{InspectKind, Inspected};
use crate::test_support::frames;
use crate::test_support::shell::*;

fn shared(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut v: Vec<(usize, String)> = world
        .query::<(&SharedPose, &Text)>()
        .iter(world)
        .map(|(p, t)| (p.0, t.0.clone()))
        .collect();
    v.sort();
    v.into_iter().map(|(_, t)| t).collect()
}

#[test]
fn several_selected_share_what_they_have_in_common() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    // The cart at (1, 2, 0.5) yaw 90°, the crate at the origin.
    app.world_mut().resource_mut::<Selection>().entities = vec![s.cart, s.krate];
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Sensor,
        owner: s.base,
        name: "lidar".into(),
    }];
    frames(&mut app, 3);
    assert_eq!(app.world().resource::<Inspected>().kind, InspectKind::Multi);
    let text = all_text(&mut app);
    assert!(
        text.contains("SELECTION") && text.contains("3 selected"),
        "{text}"
    );
    assert!(text.contains("2 models · 1 sensor"), "{text}");
    assert_eq!(shared(&mut app), ["—", "—", "—", "0.0°", "0.0°", "—"]);
    // Moved onto the cart's z: shared.
    app.world_mut()
        .get_mut::<Transform>(s.krate)
        .unwrap()
        .translation
        .z = 0.5;
    frames(&mut app, 3);
    assert_eq!(shared(&mut app)[2], "0.500 m");
    assert_eq!(summary(&["link", "link"]), "2 links");
}

#[test]
fn the_multi_body_follows_the_selection() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    app.world_mut().resource_mut::<Selection>().entities = vec![s.cart, s.krate];
    frames(&mut app, 3);
    assert!(all_text(&mut app).contains("2 selected"));
    app.world_mut()
        .resource_mut::<Selection>()
        .entities
        .push(s.sun);
    frames(&mut app, 3);
    let text = all_text(&mut app);
    assert!(text.contains("2 models · 1 light"), "{text}");
    assert!(!text.contains("2 selected"), "{text}");
}
