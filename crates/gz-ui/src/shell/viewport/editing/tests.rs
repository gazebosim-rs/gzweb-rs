//! Editing a model through the UI: the ways in, the pill, the status bar,
//! Done and Esc.

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzCommand;
use gz_bevy::spawn::{GzEntity, GzIndex, GzWorldRoot};
use gz_bevy::view::editing::{EditCommand, EditingModel};
use gz_bevy::view::{EntityContext, Selection};
use gz_core::scene::NodeKind;

use super::{meta_text, EditingBar, EditingDone, EditingMeta, EditingName};
use crate::inspectors::common::EntityAction;
use crate::shell::context_menu::ContextItem;
use crate::test_support::frames;
use crate::test_support::shell::*;

/// A live world: the model `amr_01` (links `base_link` and `wheel_left`)
/// and `crate_01`; (app, amr, base_link, crate).
fn live_world(paused: bool) -> (App, Entity, Entity, Entity) {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let add = |world: &mut World, parent: Entity, name: &str, path: &str, kind| {
        world
            .spawn((
                GzEntity {
                    name: name.into(),
                    id: 0,
                    kind,
                    path: path.into(),
                },
                Transform::default(),
                Visibility::default(),
                ChildOf(parent),
            ))
            .id()
    };
    let amr = add(world, root, "amr_01", "amr_01", NodeKind::Model);
    let base = add(world, amr, "base_link", "amr_01/base_link", NodeKind::Link);
    add(
        world,
        amr,
        "wheel_left",
        "amr_01/wheel_left",
        NodeKind::Link,
    );
    let crate_ = add(world, root, "crate_01", "crate_01", NodeKind::Model);
    go_live(&mut app, paused);
    (app, amr, base, crate_)
}

fn editing(app: &App) -> Option<Entity> {
    app.world().resource::<EditingModel>().model()
}

fn shown_bars(app: &mut App) -> usize {
    let world = app.world_mut();
    world
        .query_filtered::<&Node, With<EditingBar>>()
        .iter(world)
        .filter(|n| n.display != Display::None)
        .count()
}

fn text_of<C: Component>(app: &mut App) -> String {
    let world = app.world_mut();
    world
        .query_filtered::<&Text, With<C>>()
        .iter(world)
        .next()
        .map(|t| t.0.clone())
        .unwrap_or_default()
}

fn activate(app: &mut App, entity: Entity) {
    app.world_mut().trigger(Activate { entity });
    frames(app, 3);
}

/// The pill shows on the active view while a model is edited: its name,
/// its links and joints and the paused sim; the status bar says so. Done
/// leaves, resuming a sim entering paused.
#[test]
fn the_pill_shows_while_editing_and_done_leaves() {
    let (mut app, amr, ..) = live_world(false);
    assert_eq!(shown_bars(&mut app), 0);
    app.world_mut().write_message(EditCommand::Enter(amr));
    frames(&mut app, 3);
    assert_eq!(editing(&app), Some(amr));
    assert!(sent(&mut app).contains(&GzCommand::Pause));
    go_live(&mut app, true);
    frames(&mut app, 2);
    assert_eq!(shown_bars(&mut app), 1);
    assert_eq!(text_of::<EditingName>(&mut app), "amr_01");
    assert_eq!(
        text_of::<EditingMeta>(&mut app),
        "· 2 links · 0 joints · sim paused"
    );
    assert!(all_text(&mut app).contains("sim paused while editing"));
    let done = find::<EditingDone>(&mut app);
    activate(&mut app, done);
    assert_eq!(editing(&app), None);
    assert!(sent(&mut app).contains(&GzCommand::Play));
    assert_eq!(shown_bars(&mut app), 0);
}

/// Esc in Navigate clears the model's selection first, then leaves.
#[test]
fn esc_clears_the_selection_then_leaves() {
    let (mut app, amr, base, _) = live_world(true);
    app.world_mut().write_message(EditCommand::Enter(amr));
    frames(&mut app, 3);
    app.world_mut().resource_mut::<Selection>().entities = vec![base];
    frames(&mut app, 1);
    key(&mut app, KeyCode::Escape, bevy::input::ButtonState::Pressed);
    key(
        &mut app,
        KeyCode::Escape,
        bevy::input::ButtonState::Released,
    );
    frames(&mut app, 2);
    assert!(app.world().resource::<Selection>().is_empty());
    assert_eq!(editing(&app), Some(amr));
    key(&mut app, KeyCode::Escape, bevy::input::ButtonState::Pressed);
    key(
        &mut app,
        KeyCode::Escape,
        bevy::input::ButtonState::Released,
    );
    frames(&mut app, 2);
    assert_eq!(editing(&app), None);
}

/// The context menu's Edit model and the Model inspector's Edit model row
/// edit the model.
#[test]
fn edit_model_in_the_menu_and_the_inspector_enters() {
    let (mut app, amr, _, crate_) = live_world(true);
    app.world_mut().resource_mut::<Selection>().entities = vec![crate_];
    app.world_mut().write_message(EntityContext {
        entity: crate_,
        view: None,
    });
    frames(&mut app, 3);
    let item = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ContextItem, Has<bevy::ui::InteractionDisabled>)>()
            .iter(world)
            .find(|(_, i, _)| **i == ContextItem::EditModel)
            .map(|(e, _, d)| (e, d))
            .unwrap()
    };
    assert!(!item.1, "available");
    activate(&mut app, item.0);
    assert_eq!(editing(&app), Some(crate_));
    app.world_mut().write_message(EditCommand::Done);
    frames(&mut app, 2);
    app.world_mut().resource_mut::<Selection>().entities = vec![amr];
    frames(&mut app, 4);
    let row = {
        let world = app.world_mut();
        world
            .query::<(Entity, &EntityAction)>()
            .iter(world)
            .find(|(_, a)| **a == EntityAction::EditModel)
            .map(|(e, _)| e)
            .expect("the inspector's Edit model row")
    };
    activate(&mut app, row);
    assert_eq!(editing(&app), Some(amr));
}

#[test]
fn the_meta_counts() {
    assert_eq!(meta_text(1, 1, false), "· 1 link · 1 joint");
    assert_eq!(meta_text(3, 2, true), "· 3 links · 2 joints · sim paused");
}
