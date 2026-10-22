use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::joint_state::{JointState, JointStateWanted, JointStates};
use gz_bevy::overlays::Overlays;
use gz_bevy::view::{Part, PartKind, Selection};

use super::JointValue;
use crate::inspectors::common::{fixture, EntityAction, SelectPath};
use crate::inspectors::model::tests::action;
use crate::inspectors::{InspectKind, Inspected};
use crate::test_support::frames;
use crate::test_support::shell::*;

fn live(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut v: Vec<(usize, String)> = world
        .query::<(&JointValue, &Text)>()
        .iter(world)
        .map(|(j, t)| (j.index, t.0.clone()))
        .collect();
    v.sort();
    v.into_iter().map(|(_, t)| t).collect()
}

#[test]
fn a_joint_shows_its_definition_and_live_state() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Joint,
        owner: s.cart,
        name: "hinge".into(),
    }];
    frames(&mut app, 3);
    let i = app.world().resource::<Inspected>().clone();
    assert_eq!((i.kind, i.entity), (InspectKind::Joint, Some(s.cart)));
    let text = all_text(&mut app);
    for want in [
        "JOINT",
        "hinge",
        "cart › hinge",
        "revolute",
        "wheel",
        "0  1  0",
        "\u{2212}57.3°",
        "57.3°",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
    // The model's joint states are wanted; until one arrives, "—".
    assert_eq!(
        app.world().resource::<JointStateWanted>().0.as_deref(),
        Some("cart")
    );
    assert_eq!(live(&mut app), ["—", "—", "—"]);
    app.world_mut().resource_mut::<JointStates>().joints.insert(
        "hinge".into(),
        JointState {
            position: std::f64::consts::FRAC_PI_2,
            velocity: 0.0,
            effort: 1.5,
        },
    );
    frames(&mut app, 2);
    assert_eq!(live(&mut app), ["90.0°", "0.0°/s", "1.5 N·m"]);
    // Show axis turns the joints overlay on.
    let axis = action(&mut app, EntityAction::ShowJoints);
    app.world_mut().trigger(Activate { entity: axis });
    frames(&mut app, 2);
    assert!(app.world().resource::<Overlays>().joints);
    // Select child: the wheel link.
    let child = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SelectPath)>()
            .iter(world)
            .find(|(_, p)| p.0 == "cart/wheel")
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: child });
    frames(&mut app, 3);
    let sel = app.world().resource::<Selection>().clone();
    assert!(sel.parts.is_empty() && sel.entities.len() == 1, "{sel:?}");
    assert_eq!(app.world().resource::<JointStateWanted>().0, None);
}

#[test]
fn a_sliding_joint_reads_in_metres() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Joint,
        owner: s.cart,
        name: "slide".into(),
    }];
    frames(&mut app, 3);
    let text = all_text(&mut app);
    for want in ["prismatic", "0.5 m", "30 N", "2 m/s"] {
        assert!(text.contains(want), "{want} in {text}");
    }
    app.world_mut().resource_mut::<JointStates>().joints.insert(
        "slide".into(),
        JointState {
            position: 0.25,
            velocity: 0.1,
            effort: 3.0,
        },
    );
    frames(&mut app, 2);
    assert_eq!(live(&mut app), ["0.250 m", "0.1 m/s", "3 N"]);
}
