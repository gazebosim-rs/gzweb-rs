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
    // The model's joint states are wanted, on its publisher's topic;
    // until one arrives, "—".
    assert_eq!(
        app.world().resource::<JointStateWanted>().clone(),
        JointStateWanted {
            model: Some("cart".into()),
            topic: Some("/cart/joints".into())
        }
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
    assert_eq!(app.world().resource::<JointStateWanted>().model, None);
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
    for want in ["prismatic", "1  0  0 (model)", "0.5 m", "30 N", "2 m/s"] {
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

#[test]
fn an_end_that_isnt_in_the_scene_cant_be_selected() {
    use bevy::ui::InteractionDisabled;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    // `slide`'s child, rail, is in the SDFormat but not the scene.
    app.world_mut().resource_mut::<Selection>().parts = vec![Part {
        kind: PartKind::Joint,
        owner: s.cart,
        name: "slide".into(),
    }];
    frames(&mut app, 3);
    let world = app.world_mut();
    let paths: Vec<String> = world
        .query::<&SelectPath>()
        .iter(world)
        .map(|p| p.0.clone())
        .collect();
    assert_eq!(paths, ["cart/base"], "the parent only");
    let disabled = world
        .query_filtered::<Has<InteractionDisabled>, (
            With<crate::inspectors::common::QuickAction>,
            // Reparent is off too, outside editing.
            Without<crate::panels::PartAction>,
        )>()
        .iter(world)
        .filter(|d| *d)
        .count();
    assert_eq!(disabled, 1, "Select child");
}

/// A joint with a JointPositionController has a command slider on a
/// server that publishes (once the probe says so), which sits at the
/// joint's position until moved and publishes its position; not
/// otherwise.
#[test]
fn a_controlled_joint_takes_a_command() {
    use gz_bevy::connection::{GzCommand, GzStatus};
    use prost::Message as _;
    let open = |publishes: Option<bool>| {
        let mut app = shell_app((1440, 900), 1.0);
        frames(&mut app, 2);
        let s = fixture::scene(&mut app);
        app.world_mut().resource_mut::<GzStatus>().publishes = publishes;
        app.world_mut().resource_mut::<Selection>().parts = vec![Part {
            kind: PartKind::Joint,
            owner: s.cart,
            name: "slide".into(),
        }];
        frames(&mut app, 3);
        app
    };
    let mut app = open(None);
    let world = app.world_mut();
    assert!(world
        .query::<&super::JointCommand>()
        .iter(world)
        .next()
        .is_none());
    let mut refused = open(Some(false));
    let world = refused.world_mut();
    assert!(world
        .query::<&super::JointCommand>()
        .iter(world)
        .next()
        .is_none());
    // The probe answers after the inspector opened.
    app.world_mut().resource_mut::<GzStatus>().publishes = Some(true);
    frames(&mut app, 3);
    let (row, topic) = {
        let world = app.world_mut();
        let (e, c) = world
            .query::<(Entity, &super::JointCommand)>()
            .single(world)
            .unwrap();
        (e, c.0.clone())
    };
    assert_eq!(topic, "/model/cart/joint/slide/0/cmd_pos");
    let slider = {
        let world = app.world_mut();
        let kids = world.get::<Children>(row).unwrap().to_vec();
        kids.into_iter()
            .find(|c| world.get::<bevy::ui_widgets::Slider>(*c).is_some())
            .unwrap()
    };
    let at = |app: &App| {
        app.world()
            .get::<bevy::ui_widgets::SliderValue>(slider)
            .unwrap()
            .0
    };
    let state = |position| JointState {
        position,
        velocity: 0.0,
        effort: 0.0,
    };
    let live = |app: &mut App, p: f64| {
        app.world_mut()
            .resource_mut::<JointStates>()
            .joints
            .insert("slide".into(), state(p));
        frames(app, 2);
    };
    live(&mut app, 0.1);
    assert!((at(&app) - 0.1).abs() < 1e-6, "follows the joint");
    sent(&mut app);
    app.world_mut().trigger(bevy::ui_widgets::ValueChange {
        source: slider,
        value: 0.3f32,
        is_final: true,
    });
    frames(&mut app, 1);
    live(&mut app, 0.05);
    assert!((at(&app) - 0.3).abs() < 1e-6, "moved: keeps the command");
    let positions: Vec<f64> = sent(&mut app)
        .into_iter()
        .filter_map(|c| match c {
            GzCommand::Publish {
                topic: t, payload, ..
            } if t == topic => Some(
                gz_core::msgs::Double::decode(payload.as_slice())
                    .unwrap()
                    .data,
            ),
            _ => None,
        })
        .collect();
    assert_eq!(positions.len(), 1);
    assert!((positions[0] - 0.3).abs() < 1e-6);
}
