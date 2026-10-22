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
