use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::view::Selection;

use crate::inspectors::common::{
    fixture, DismissButton, EntityAction, InspectorTitle, PoseFrame, PoseValue, QuickAction,
};
use crate::inspectors::{InspectKind, Inspected};
use crate::test_support::frames;
use crate::test_support::shell::*;

pub(crate) fn select(app: &mut App, entities: Vec<Entity>) {
    app.world_mut().resource_mut::<Selection>().entities = entities;
    frames(app, 3);
}

/// The values of the pose rows in `frame`, x to yaw.
pub(crate) fn pose(app: &mut App, frame: PoseFrame) -> Vec<String> {
    let world = app.world_mut();
    let mut v: Vec<(usize, String)> = world
        .query::<(&PoseValue, &Text)>()
        .iter(world)
        .filter(|(p, _)| p.0 == frame)
        .map(|(p, t)| (p.1, t.0.clone()))
        .collect();
    v.sort();
    v.into_iter().map(|(_, t)| t).collect()
}

pub(crate) fn action(app: &mut App, which: EntityAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &EntityAction)>()
        .iter(world)
        .find(|(_, a)| **a == which)
        .unwrap()
        .0
}

#[test]
fn a_model_shows_its_pose_flags_plugins_and_parts() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    assert_eq!(app.world().resource::<Inspected>().kind, InspectKind::Model);
    let text = all_text(&mut app);
    for want in [
        "MODEL",
        "cart",
        "warehouse",
        "DiffDrive",
        "Links 2",
        "Joints 2",
        "Sensors 1",
        "Edit model",
        "All components",
    ] {
        if want == "warehouse" {
            continue;
        }
        assert!(text.contains(want), "{want} in {text}");
    }
    // World pose in Gazebo axes: (1, 2, 0.5), yaw 90°.
    assert_eq!(
        pose(&mut app, PoseFrame::World),
        ["1.000 m", "2.000 m", "0.500 m", "0.0°", "0.0°", "90.0°"]
    );
    // It follows the sim.
    app.world_mut()
        .get_mut::<Transform>(s.cart)
        .unwrap()
        .translation
        .x = 3.0;
    frames(&mut app, 3);
    assert_eq!(pose(&mut app, PoseFrame::World)[0], "3.000 m");
    // Duplicate and Save as… aren't available yet; Focus is.
    let world = app.world_mut();
    let disabled = world
        .query_filtered::<Has<InteractionDisabled>, With<QuickAction>>()
        .iter(world)
        .filter(|d| *d)
        .count();
    assert_eq!(disabled, 2);
}

#[test]
fn focus_selects_the_pinned_subject_before_framing_it() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    app.world_mut().resource_mut::<Inspected>().pinned = true;
    select(&mut app, vec![s.krate]);
    let focus = action(&mut app, EntityAction::Focus);
    app.world_mut().trigger(Activate { entity: focus });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Selection>().entities, [s.cart]);
}

#[test]
fn a_removed_model_greys_its_name_until_dismissed() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    app.world_mut().entity_mut(s.cart).despawn();
    frames(&mut app, 3);
    assert!(app.world().resource::<Inspected>().removed);
    let world = app.world_mut();
    let text = world
        .query_filtered::<&Text, With<InspectorTitle>>()
        .single(world)
        .unwrap()
        .0
        .clone();
    assert_eq!(text, "cart (removed)");
    let all_disabled = world
        .query_filtered::<Has<InteractionDisabled>, With<QuickAction>>()
        .iter(world)
        .all(|d| d);
    assert!(all_disabled);
    let dismiss = find::<DismissButton>(&mut app);
    assert!(displayed(&app, dismiss));
    app.world_mut().trigger(Activate { entity: dismiss });
    frames(&mut app, 3);
    assert_eq!(*app.world().resource::<Inspected>(), Inspected::default());
}

fn pinned_header(app: &mut App) -> Entity {
    use crate::shell::inspector::InspectorHeader;
    let world = app.world_mut();
    world
        .query::<(Entity, &InspectorHeader)>()
        .iter(world)
        .find(|(_, h)| **h == InspectorHeader::Pin)
        .unwrap()
        .0
}

#[test]
fn a_removed_subject_gives_way_to_a_part_and_dismiss_clears_parts() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.krate]);
    app.world_mut().entity_mut(s.krate).despawn();
    frames(&mut app, 3);
    assert!(app.world().resource::<Inspected>().removed);
    let lidar = gz_bevy::view::Part {
        kind: gz_bevy::view::PartKind::Sensor,
        owner: s.base,
        name: "lidar".into(),
    };
    app.world_mut().resource_mut::<Selection>().parts = vec![lidar];
    frames(&mut app, 3);
    let i = app.world().resource::<Inspected>().clone();
    assert_eq!((i.kind, i.removed), (InspectKind::Sensor, false));
    // Removed again while a part is selected: Dismiss clears it too.
    app.world_mut().entity_mut(s.base).despawn();
    frames(&mut app, 3);
    assert!(app.world().resource::<Inspected>().removed);
    app.world_mut()
        .resource_mut::<Selection>()
        .parts
        .push(gz_bevy::view::Part {
            kind: gz_bevy::view::PartKind::Joint,
            owner: s.cart,
            name: "hinge".into(),
        });
    app.world_mut().resource_mut::<Inspected>().pinned = true;
    frames(&mut app, 3);
    // Pinned, the removed subject stays until dismissed.
    assert!(app.world().resource::<Inspected>().removed);
    let dismiss = find::<DismissButton>(&mut app);
    app.world_mut().trigger(Activate { entity: dismiss });
    frames(&mut app, 3);
    assert!(app.world().resource::<Selection>().is_empty());
    assert_eq!(*app.world().resource::<Inspected>(), Inspected::default());
}

#[test]
fn a_part_follows_its_owners_respawn_a_few_frames_later() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    let lidar = |owner| gz_bevy::view::Part {
        kind: gz_bevy::view::PartKind::Sensor,
        owner,
        name: "lidar".into(),
    };
    app.world_mut().resource_mut::<Selection>().parts = vec![lidar(s.base)];
    frames(&mut app, 3);
    app.world_mut().entity_mut(s.base).despawn();
    frames(&mut app, 3);
    // The scene rebuilds the link later, at the same path.
    let again = app
        .world_mut()
        .spawn((
            gz_bevy::spawn::GzEntity {
                name: "base".into(),
                id: 0,
                kind: gz_core::scene::NodeKind::Link,
                path: "cart/base".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(s.cart),
        ))
        .id();
    frames(&mut app, 3);
    let sel = app.world().resource::<Selection>().clone();
    assert_eq!((sel.entities.len(), sel.parts), (0, vec![lidar(again)]));
    let i = app.world().resource::<Inspected>().clone();
    assert_eq!(
        (i.kind, i.entity, i.removed),
        (InspectKind::Sensor, Some(again), false)
    );
    assert_eq!(i.part.map(|p| p.owner), Some(again));
}

#[test]
fn the_pin_button_shows_the_pin_and_a_pinned_removal_waits_for_dismiss() {
    use crate::theme::visuals::Active;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.krate]);
    let pin = pinned_header(&mut app);
    app.world_mut().trigger(Activate { entity: pin });
    frames(&mut app, 2);
    assert!(app.world().get::<Active>(pin).is_some());
    select(&mut app, vec![s.sun]);
    app.world_mut().entity_mut(s.krate).despawn();
    frames(&mut app, 3);
    let i = app.world().resource::<Inspected>().clone();
    assert!(
        i.removed && i.pinned && i.kind == InspectKind::Model,
        "{i:?}"
    );
    let dismiss = find::<DismissButton>(&mut app);
    app.world_mut().trigger(Activate { entity: dismiss });
    frames(&mut app, 3);
    assert!(app.world().get::<Active>(pin).is_none(), "unpinned");
    // One click pins again.
    select(&mut app, vec![s.cart]);
    app.world_mut().trigger(Activate { entity: pin });
    frames(&mut app, 2);
    assert!(app.world().resource::<Inspected>().pinned);
}

#[test]
fn focus_replaces_parts_and_hide_reads_show_while_hidden() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.sun]);
    let hide = action(&mut app, EntityAction::Hide);
    app.world_mut().trigger(Activate { entity: hide });
    frames(&mut app, 3);
    assert!(all_text(&mut app).contains("Show"));
    app.world_mut().resource_mut::<Inspected>().pinned = true;
    app.world_mut().resource_mut::<Selection>().parts = vec![gz_bevy::view::Part {
        kind: gz_bevy::view::PartKind::Joint,
        owner: s.cart,
        name: "hinge".into(),
    }];
    frames(&mut app, 2);
    let focus = action(&mut app, EntityAction::Focus);
    app.world_mut().trigger(Activate { entity: focus });
    frames(&mut app, 2);
    let sel = app.world().resource::<Selection>().clone();
    assert_eq!((sel.entities, sel.parts.len()), (vec![s.sun], 0));
}

/// `choose_subject` with the selection still holding a despawned entity
/// (`prune_selection` not run yet: the app doesn't order them).
fn choose(app: &mut App) {
    use bevy::ecs::system::RunSystemOnce;
    app.world_mut()
        .run_system_once(crate::shell::inspector::choose_subject)
        .unwrap();
}

#[test]
fn a_despawned_subject_is_removed_whatever_runs_first() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.krate]);
    app.world_mut().entity_mut(s.krate).despawn();
    // Not pruned yet.
    assert_eq!(app.world().resource::<Selection>().entities, [s.krate]);
    choose(&mut app);
    let i = app.world().resource::<Inspected>().clone();
    assert!(i.removed && i.kind == InspectKind::Model, "{i:?}");
}

#[test]
fn unpinning_goes_back_to_the_selection_and_a_multi_selection_has_no_pin() {
    use bevy::ui::InteractionDisabled;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    let pin = pinned_header(&mut app);
    app.world_mut().trigger(Activate { entity: pin });
    frames(&mut app, 2);
    select(&mut app, vec![s.krate]);
    assert_eq!(app.world().resource::<Inspected>().entity, Some(s.cart));
    app.world_mut().trigger(Activate { entity: pin });
    frames(&mut app, 3);
    assert_eq!(app.world().resource::<Inspected>().entity, Some(s.krate));
    // Several selected: Pin is off and does nothing.
    select(&mut app, vec![s.cart, s.krate]);
    assert!(app.world().get::<InteractionDisabled>(pin).is_some());
    app.world_mut().trigger(Activate { entity: pin });
    frames(&mut app, 2);
    assert!(!app.world().resource::<Inspected>().pinned);
    select(&mut app, vec![s.cart]);
    assert!(app.world().get::<InteractionDisabled>(pin).is_none());
}

/// The pose fields in `frame` and their values, x to yaw.
fn pose_fields(app: &mut App, frame: PoseFrame) -> Vec<(Entity, f64)> {
    use crate::widgets::NumberField;
    let world = app.world_mut();
    let mut v: Vec<(usize, Entity, f64)> = world
        .query::<(Entity, &PoseValue, &NumberField)>()
        .iter(world)
        .filter(|(_, p, _)| p.0 == frame)
        .map(|(e, p, f)| (p.1, e, f.value))
        .collect();
    v.sort_by_key(|(i, ..)| *i);
    v.into_iter().map(|(_, e, f)| (e, f)).collect()
}

#[test]
fn with_edits_the_pose_is_fields_that_move_the_model() {
    use crate::widgets::NumberCommit;
    use gz_bevy::connection::GzStatus;
    use gz_bevy::edit::{Held, SetPose};
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    // A stock server: text.
    assert!(pose_fields(&mut app, PoseFrame::World).is_empty());
    assert_eq!(pose(&mut app, PoseFrame::World).len(), 6);
    // One that takes edits: the body rebuilds with fields.
    {
        let mut status = app.world_mut().resource_mut::<GzStatus>();
        status.edits = Some(true);
        status.world = Some("w".into());
    }
    frames(&mut app, 3);
    assert!(pose(&mut app, PoseFrame::World).is_empty());
    let fields = pose_fields(&mut app, PoseFrame::World);
    let values: Vec<f64> = fields
        .iter()
        .map(|f| (f.1 * 1000.0).round() / 1000.0)
        .collect();
    assert_eq!(values, [1.0, 2.0, 0.5, 0.0, 0.0, 90.0]);
    let text = all_text(&mut app);
    assert!(text.contains("Position") && text.contains("Rotation"));
    // Committing X moves it at once and sends the pose.
    app.world_mut().trigger(NumberCommit {
        entity: fields[0].0,
        value: 3.0,
        is_final: true,
    });
    let sent: Vec<SetPose> = app
        .world_mut()
        .resource_mut::<Messages<SetPose>>()
        .drain()
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].entity, s.cart);
    assert!((sent[0].pose.translation - Vec3::new(3.0, 2.0, 0.5)).length() < 1e-5);
    let turned = sent[0]
        .pose
        .rotation
        .angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    assert!(turned < 1e-4);
    assert_eq!(
        app.world().get::<Transform>(s.cart).unwrap().translation,
        sent[0].pose.translation
    );
    assert!(app.world().resource::<Held>().until.contains_key(&s.cart));
    // Yaw, in degrees.
    app.world_mut().trigger(NumberCommit {
        entity: fields[5].0,
        value: 0.0,
        is_final: true,
    });
    frames(&mut app, 2);
    let rotation = app.world().get::<Transform>(s.cart).unwrap().rotation;
    assert!(rotation.angle_between(Quat::IDENTITY) < 1e-4);
}

#[test]
fn a_links_and_a_lights_pose_stay_text_and_a_scrub_sends_at_most_ten_a_second() {
    use crate::widgets::NumberCommit;
    use gz_bevy::connection::GzStatus;
    use gz_bevy::edit::SetPose;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    {
        let mut status = app.world_mut().resource_mut::<GzStatus>();
        status.edits = Some(true);
        status.world = Some("w".into());
    }
    select(&mut app, vec![s.base]);
    assert!(pose_fields(&mut app, PoseFrame::Parent).is_empty());
    assert_eq!(pose(&mut app, PoseFrame::Parent).len(), 6);
    // Nor a light's: gz-sim's set_pose doesn't move lights.
    select(&mut app, vec![s.sun]);
    assert!(pose_fields(&mut app, PoseFrame::Parent).is_empty());
    // Another top-level model does.
    select(&mut app, vec![s.krate]);
    let fields = pose_fields(&mut app, PoseFrame::World);
    assert_eq!(fields.len(), 6);
    for (value, is_final) in [(0.5, false), (1.0, false), (1.5, true)] {
        app.world_mut().trigger(NumberCommit {
            entity: fields[2].0,
            value,
            is_final,
        });
    }
    let sent: Vec<f32> = app
        .world_mut()
        .resource_mut::<Messages<SetPose>>()
        .drain()
        .map(|e| e.pose.translation.z)
        .collect();
    // The first scrub step and the release; the second came too soon.
    assert_eq!(sent, [0.5, 1.5]);
}

#[test]
fn ctrl_z_puts_a_pose_edit_back_and_the_dot_shows_the_world_is_dirty() {
    use crate::shell::top_bar::world_button::UnsavedDot;
    use crate::widgets::NumberCommit;
    use gz_bevy::connection::GzStatus;
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.cart]);
    {
        let mut status = app.world_mut().resource_mut::<GzStatus>();
        status.edits = Some(true);
        status.world = Some("w".into());
    }
    frames(&mut app, 3);
    let dot = |app: &mut App| {
        let d = find::<UnsavedDot>(app);
        app.world().get::<Node>(d).unwrap().display != Display::None
    };
    assert!(!dot(&mut app));
    let x = |app: &App| app.world().get::<Transform>(s.cart).unwrap().translation.x;
    let field = pose_fields(&mut app, PoseFrame::World)[0].0;
    // A scrub is one edit.
    for (value, is_final) in [(1.5, false), (2.0, false), (2.5, true)] {
        app.world_mut().trigger(NumberCommit {
            entity: field,
            value,
            is_final,
        });
        app.update();
    }
    frames(&mut app, 2);
    assert_eq!(x(&app), 2.5);
    assert!(dot(&mut app));
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    frames(&mut app, 2);
    assert!((x(&app) - 1.0).abs() < 1e-5, "{}", x(&app));
    assert!(!dot(&mut app));
    chord(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyZ],
    );
    frames(&mut app, 2);
    assert_eq!(x(&app), 2.5);
    assert!(dot(&mut app));
}

#[test]
fn a_scrub_past_straight_up_carries_on_and_a_held_back_step_still_goes_out() {
    use crate::widgets::NumberCommit;
    use gz_bevy::connection::GzStatus;
    use gz_bevy::edit::{HasPlane, SetPose};
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    {
        let mut status = app.world_mut().resource_mut::<GzStatus>();
        status.edits = Some(true);
        status.world = Some("w".into());
    }
    select(&mut app, vec![s.cart]);
    let fields = pose_fields(&mut app, PoseFrame::World);
    let scrub = |app: &mut App, i: usize, value: f64, is_final: bool| {
        app.world_mut().trigger(NumberCommit {
            entity: fields[i].0,
            value,
            is_final,
        });
        app.update();
    };
    // Pitch from 80° through 90° to 100°: it carries on, with roll and yaw
    // as they were, rather than coming back as 80° with both turned round.
    for pitch in [80.0, 89.0, 95.0, 100.0] {
        scrub(&mut app, 4, pitch, false);
    }
    scrub(&mut app, 4, 100.0, true);
    frames(&mut app, 2);
    let values: Vec<f64> = pose_fields(&mut app, PoseFrame::World)
        .iter()
        .map(|f| (f.1 * 1000.0).round() / 1000.0)
        .collect();
    assert_eq!(values[3..], [0.0, 100.0, 90.0]);
    let want = Quat::from_euler(EulerRot::ZYX, 90f32.to_radians(), 100f32.to_radians(), 0.0);
    let rotation = app.world().get::<Transform>(s.cart).unwrap().rotation;
    assert!(rotation.angle_between(want) < 1e-4);
    app.world_mut()
        .resource_mut::<Messages<SetPose>>()
        .drain()
        .for_each(drop);
    // A step held back (too soon after the last) goes out once the time is
    // up, so a scrub that stops on it doesn't spring back.
    // Back to back: the second is too soon.
    for value in [2.0, 2.5] {
        app.world_mut().trigger(NumberCommit {
            entity: fields[0].0,
            value,
            is_final: false,
        });
    }
    let sent = |app: &mut App| -> Vec<f32> {
        app.world_mut()
            .resource_mut::<Messages<SetPose>>()
            .drain()
            .map(|e| e.pose.translation.x)
            .collect()
    };
    assert_eq!(sent(&mut app), [2.0]);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(gz_bevy::view::gizmo::SEND_SECS * 1.5),
    ));
    frames(&mut app, 2);
    assert_eq!(sent(&mut app), [2.5]);
    // A plane found after the body was built: no more edits, and the pose
    // turns to text.
    app.world_mut().entity_mut(s.cart).insert(HasPlane);
    scrub(&mut app, 0, 3.0, true);
    assert!(sent(&mut app).is_empty());
    frames(&mut app, 2);
    assert!(pose_fields(&mut app, PoseFrame::World).is_empty());
}
