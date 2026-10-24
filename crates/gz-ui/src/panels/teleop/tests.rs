//! The Teleop panel: its rules, and the panel through the shell.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::Pressed;
use bevy::ui_widgets::ValueChange;
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::overlays::extras::{SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzIndex, GzWorldRoot};
use gz_bevy::topics::GzTopics;
use gz_bevy::view::{DriveArmed, Selection};
use gz_core::msgs;
use gz_core::scene::NodeKind;
use prost::Message as _;

use super::model::*;
use super::systems::{Targets, NO_PUBLISH, SELECT};
use super::view::{LimitField, PadKey, TeleopSlider};
use super::{STOP, TELEOP};
use crate::panels::{PanelActionActivated, PanelView};
use crate::shell::sidebar::body::BodyView;
use crate::shell::{PanelStatus, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::NumberCommit;

/// Keys give full speed each way and a pair cancels; a stick starts past
/// its dead zone; a target's label drops its model's own prefix.
#[test]
fn commands_from_keys_and_sticks() {
    let held = Held {
        forward: true,
        left: true,
        ..default()
    };
    assert_eq!(held.command(1.0, 1.5), (1.0, 1.5));
    let both = Held {
        forward: true,
        back: true,
        right: true,
        ..default()
    };
    assert_eq!(both.command(1.0, 1.5), (0.0, -1.5));
    assert!(!Held::default().any());
    assert_eq!(stick(0.05), 0.0);
    assert!((stick(1.0) - 1.0).abs() < 1e-6);
    assert!(stick(-0.5) < 0.0);
    let t = |model: &str, topic: &str| DriveTarget {
        model: model.into(),
        topic: topic.into(),
        joints: vec![],
    };
    assert_eq!(t("amr", "/model/amr/cmd_vel").label(), "amr · cmd_vel");
    assert_eq!(t("car", "/drive").label(), "car · /drive");
    assert_eq!(t("", "/cmd_vel").label(), "/cmd_vel");
    assert_eq!(topic_model("/model/amr/cmd_vel"), Some("amr"));
    assert_eq!(topic_model("/cmd_vel"), None);
    assert_eq!(speed_label(0.35, "m/s"), "0.35 m/s");
}

const SDF: &str = r#"<sdf version="1.10"><world name="w">
  <model name="amr">
    <link name="base"/><link name="mast"/>
    <joint name="lift" type="prismatic"><parent>base</parent><child>mast</child>
      <axis><xyz>0 0 1</xyz><limit><lower>0</lower><upper>0.5</upper></limit></axis></joint>
    <plugin filename="gz-sim-diff-drive-system" name="gz::sim::systems::DiffDrive"/>
    <plugin filename="gz-sim-joint-position-controller-system" name="j">
      <joint_name>lift</joint_name></plugin>
  </model>
  <model name="crate"/>
</world></sdf>"#;

/// The panel live and open on a world with a drive, on a server that
/// publishes (or not).
fn live_teleop(publishes: Option<bool>) -> (App, Entity) {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )))
    .init_resource::<SdfExtras>()
    .init_resource::<SdfWanted>();
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let amr = world
        .spawn((
            GzEntity {
                name: "amr".into(),
                id: 0,
                kind: NodeKind::Model,
                path: "amr".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: TELEOP,
        slot: Slot::Primary,
    });
    go_live(&mut app, false);
    app.world_mut().resource_mut::<GzStatus>().publishes = publishes;
    app.world_mut().resource_mut::<SdfExtras>().extras =
        Some(gz_core::sdf::WorldExtras::parse(SDF).unwrap());
    frames(&mut app, 3);
    sent(&mut app);
    (app, amr)
}

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    world
        .query::<(&BodyView, &crate::panels::PanelBody)>()
        .iter(world)
        .find(|(_, b)| b.0 == TELEOP)
        .unwrap()
        .0
         .0
        .clone()
}

fn twists(app: &mut App) -> Vec<(String, f64, f64)> {
    sent(app)
        .into_iter()
        .filter_map(|c| match c {
            GzCommand::Publish {
                topic,
                msg_type,
                payload,
            } if msg_type == "gz.msgs.Twist" => {
                let t = msgs::Twist::decode(payload.as_slice()).unwrap();
                Some((topic, t.linear.unwrap().x, t.angular.unwrap().z))
            }
            _ => None,
        })
        .collect()
}

fn state(app: &App) -> TeleopState {
    app.world().resource::<TeleopState>().clone()
}

/// Not connected; waiting for the probe; a server that doesn't publish
/// says why; nothing to drive asks for a robot; a drive is the target.
#[test]
fn the_panel_needs_a_publishing_server_and_a_drive() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: TELEOP,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::empty("Not connected"));
    let (mut app, _) = live_teleop(None);
    assert_eq!(view(&mut app), PanelView::Loading);
    let (mut app, _) = live_teleop(Some(false));
    assert_eq!(view(&mut app), PanelView::empty(NO_PUBLISH));
    let (mut app, _) = live_teleop(Some(true));
    assert_eq!(view(&mut app), PanelView::Ready);
    let targets = app.world().resource::<Targets>().clone();
    assert_eq!(targets.0.len(), 1);
    assert_eq!(targets.0[0].topic, "/model/amr/cmd_vel");
    assert_eq!(targets.0[0].joints[0].upper, 0.5);
    assert_eq!(state(&app).target.as_deref(), Some("/model/amr/cmd_vel"));
    assert!(app.world().resource::<SdfWanted>().by("teleop"));
    app.world_mut().resource_mut::<SdfExtras>().extras = None;
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::empty(SELECT));
    // A Twist topic the server lists is a target too.
    let mut topics = GzTopics::default();
    topics.set(vec![("/model/boat/cmd_vel".into(), "gz.msgs.Twist".into())]);
    *app.world_mut().resource_mut::<GzTopics>() = topics;
    frames(&mut app, 2);
    assert_eq!(state(&app).target.as_deref(), Some("/model/boat/cmd_vel"));
}

/// A pad key arms and drives; armed, the keys drive too and the views'
/// fly keys and gizmo stand down; the command goes out about ten times a
/// second; X stops with a last zero.
#[test]
fn the_pad_and_keys_drive_and_x_stops() {
    let (mut app, _) = live_teleop(Some(true));
    let w = {
        let world = app.world_mut();
        world
            .query::<(Entity, &PadKey)>()
            .iter(world)
            .find(|(_, k)| **k == PadKey::Forward)
            .unwrap()
            .0
    };
    app.world_mut().entity_mut(w).insert(Pressed);
    frames(&mut app, 1);
    assert!(state(&app).armed);
    assert!(app.world().resource::<DriveArmed>().0);
    assert_eq!(
        app.world().resource::<PanelStatus>().count(TELEOP),
        Some("publishing")
    );
    frames(&mut app, 9);
    let sent = twists(&mut app);
    assert!(sent.len() >= 4 && sent.len() <= 7, "{sent:?}");
    assert!(sent
        .iter()
        .all(|(t, l, _)| t == "/model/amr/cmd_vel" && *l == 1.0));
    app.world_mut().entity_mut(w).remove::<Pressed>();
    key(&mut app, KeyCode::KeyA, bevy::input::ButtonState::Pressed);
    frames(&mut app, 2);
    let sent = twists(&mut app);
    assert_eq!(
        sent.first().map(|s| (s.1, s.2)),
        Some((0.0, 1.5)),
        "{sent:?}"
    );
    key(&mut app, KeyCode::KeyA, bevy::input::ButtonState::Released);
    key(&mut app, KeyCode::KeyX, bevy::input::ButtonState::Pressed);
    frames(&mut app, 2);
    assert!(!state(&app).armed);
    assert!(!app.world().resource::<DriveArmed>().0);
    let sent = twists(&mut app);
    assert_eq!(
        sent.last().map(|s| (s.1, s.2)),
        Some((0.0, 0.0)),
        "{sent:?}"
    );
    frames(&mut app, 4);
    assert!(twists(&mut app).is_empty(), "disarmed: quiet");
}

fn slider(app: &mut App, which: &TeleopSlider) -> Entity {
    let world = app.world_mut();
    let row = world
        .query::<(Entity, &TeleopSlider)>()
        .iter(world)
        .find(|(_, s)| *s == which)
        .unwrap()
        .0;
    let children = world.get::<Children>(row).unwrap().to_vec();
    children
        .into_iter()
        .find(|c| world.get::<bevy::ui_widgets::Slider>(*c).is_some())
        .unwrap()
}

/// In Sliders mode a slider arms and sets the command; Stop disarms; a
/// joint's slider publishes its position; a limit sets its range.
#[test]
fn sliders_drive_and_joints_move() {
    let (mut app, _) = live_teleop(Some(true));
    app.world_mut().resource_mut::<TeleopState>().mode = DriveMode::Sliders;
    frames(&mut app, 2);
    let lin = slider(&mut app, &TeleopSlider::Linear);
    app.world_mut().trigger(ValueChange {
        source: lin,
        value: 0.5f32,
        is_final: false,
    });
    frames(&mut app, 2);
    assert!(state(&app).armed);
    assert_eq!(twists(&mut app).last().map(|s| s.1), Some(0.5));
    let stop = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(PanelActionActivated {
        entity: stop,
        panel: TELEOP,
        action: STOP,
    });
    frames(&mut app, 2);
    assert!(!state(&app).armed);
    assert_eq!(twists(&mut app).last().map(|s| s.1), Some(0.0));

    let lift = slider(
        &mut app,
        &TeleopSlider::Joint("/model/amr/joint/lift/0/cmd_pos".into()),
    );
    app.world_mut().trigger(ValueChange {
        source: lift,
        value: 0.25f32,
        is_final: true,
    });
    frames(&mut app, 1);
    let doubles: Vec<f64> = sent(&mut app)
        .into_iter()
        .filter_map(|c| match c {
            GzCommand::Publish { topic, payload, .. }
                if topic == "/model/amr/joint/lift/0/cmd_pos" =>
            {
                Some(msgs::Double::decode(payload.as_slice()).unwrap().data)
            }
            _ => None,
        })
        .collect();
    assert_eq!(doubles, [0.25]);

    let field = {
        let world = app.world_mut();
        world
            .query::<(Entity, &LimitField)>()
            .iter(world)
            .find(|(_, f)| **f == LimitField::Linear)
            .unwrap()
            .0
    };
    app.world_mut().trigger(NumberCommit {
        entity: field,
        value: 0.3,
        is_final: true,
    });
    frames(&mut app, 2);
    assert_eq!(state(&app).max_lin, 0.3);
}

/// Hiding the panel stops driving; the selected model's drive is the
/// target.
#[test]
fn hiding_stops_and_the_selection_picks() {
    let (mut app, amr) = live_teleop(Some(true));
    app.world_mut().resource_mut::<TeleopState>().armed = true;
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::ToggleSidebar);
    frames(&mut app, 2);
    assert!(!state(&app).armed);
    *app.world_mut().resource_mut::<Selection>() = Selection {
        entities: vec![amr],
        parts: vec![],
    };
    app.world_mut().resource_mut::<TeleopState>().target = None;
    frames(&mut app, 2);
    assert_eq!(state(&app).target.as_deref(), Some("/model/amr/cmd_vel"));
}
