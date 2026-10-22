use super::*;
use crate::edit::EditPlugin;
use gz_core::msgs;
use gz_core::scene::NodeKind;
use gz_core::transport::Status;
use prost::Message as _;

fn app(edits: Option<bool>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(GzStatus {
            status: Status::Ready,
            world: Some("w".into()),
            edits,
            ..GzStatus::new("ws://test")
        })
        .add_plugins((EditPlugin, UndoPlugin));
    app.update();
    app
}

fn spawn(app: &mut App, name: &str) -> Entity {
    app.world_mut()
        .spawn((
            GzEntity {
                name: name.into(),
                id: 7,
                kind: NodeKind::Model,
                path: name.into(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id()
}

fn moved(entity: Entity, before: f32, after: f32, open: bool) -> Record {
    Record {
        verb: "move",
        named: true,
        changes: vec![Change::Pose {
            entity,
            before: Transform::from_xyz(before, 0.0, 0.0),
            after: Transform::from_xyz(after, 0.0, 0.0),
        }],
        open,
    }
}

fn run(app: &mut App, m: impl Message) {
    app.world_mut().write_message(m);
    app.update();
}

fn stack(app: &App) -> &UndoStack {
    app.world().resource::<UndoStack>()
}

fn x(app: &App, e: Entity) -> f32 {
    app.world().get::<Transform>(e).unwrap().translation.x
}

/// The x of each `set_pose` sent.
fn sent_x(app: &mut App) -> Vec<f64> {
    app.world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .drain()
        .filter_map(|c| match c {
            GzCommand::Request { payload, .. } => msgs::Pose::decode(payload.as_slice())
                .ok()
                .and_then(|p| p.position)
                .map(|p| p.x),
            _ => None,
        })
        .collect()
}

#[test]
fn a_move_is_undone_and_redone_through_set_pose() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    assert!(!stack(&app).is_dirty());
    run(&mut app, moved(cart, 1.0, 2.0, false));
    assert_eq!(stack(&app).undo_label(), Some("move cart"));
    assert!(stack(&app).is_dirty());
    run(&mut app, UndoCommand::Undo);
    app.update();
    assert_eq!(x(&app, cart), 1.0);
    assert_eq!(sent_x(&mut app), [1.0]);
    assert!(app.world().resource::<Held>().holds(cart, 0.0));
    // Back where it was loaded: not dirty.
    assert!(!stack(&app).is_dirty());
    assert_eq!(
        (stack(&app).undo_label(), stack(&app).redo_label()),
        (None, Some("move cart"))
    );
    run(&mut app, UndoCommand::Redo);
    app.update();
    assert_eq!(x(&app, cart), 2.0);
    assert_eq!(sent_x(&mut app), [2.0]);
    assert!(stack(&app).is_dirty());
    // Nothing more to redo; a new edit forgets what was undone.
    run(&mut app, UndoCommand::Redo);
    run(&mut app, UndoCommand::Undo);
    run(&mut app, moved(cart, 1.0, 5.0, false));
    assert_eq!(stack(&app).redo_label(), None);
}

#[test]
fn a_scrub_is_one_edit_from_where_it_began() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    run(&mut app, moved(cart, 1.0, 1.5, true));
    run(&mut app, moved(cart, 1.5, 2.0, true));
    run(&mut app, moved(cart, 2.0, 3.0, false));
    // Closed: the next is its own.
    run(&mut app, moved(cart, 3.0, 4.0, false));
    run(&mut app, UndoCommand::Undo);
    assert_eq!(x(&app, cart), 3.0);
    run(&mut app, UndoCommand::Undo);
    assert_eq!(x(&app, cart), 1.0);
    assert_eq!(stack(&app).undo_label(), None);
}

#[test]
fn without_edits_a_move_waits_and_a_hide_still_undoes() {
    let mut app = app(Some(false));
    let cart = spawn(&mut app, "cart");
    run(&mut app, moved(cart, 1.0, 2.0, false));
    run(
        &mut app,
        Record {
            verb: "hide",
            named: true,
            changes: vec![Change::Visibility {
                entity: cart,
                before: Visibility::Inherited,
                after: Visibility::Hidden,
            }],
            open: false,
        },
    );
    *app.world_mut().get_mut::<Visibility>(cart).unwrap() = Visibility::Hidden;
    run(&mut app, UndoCommand::Undo);
    assert_eq!(
        app.world().get::<Visibility>(cart),
        Some(&Visibility::Inherited)
    );
    // The move can't be undone here; it stays on top.
    run(&mut app, UndoCommand::Undo);
    assert_eq!(stack(&app).undo_label(), Some("move cart"));
    assert!(sent_x(&mut app).is_empty());
}

#[test]
fn an_edit_follows_its_path_to_a_respawn_and_names_many() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    let krate = spawn(&mut app, "crate");
    let mut r = moved(cart, 1.0, 2.0, false);
    r.changes.extend(moved(krate, 1.0, 2.0, false).changes);
    run(&mut app, r);
    assert_eq!(stack(&app).undo_label(), Some("move 2 entities"));
    app.world_mut().entity_mut(cart).despawn();
    let again = spawn(&mut app, "cart");
    run(&mut app, UndoCommand::Undo);
    assert_eq!((x(&app, again), x(&app, krate)), (1.0, 1.0));
    // One that names itself.
    assert_eq!(label("hide others", false, &["cart"]), "hide others");
}

#[test]
fn isolate_comes_back_with_its_saved_state() {
    let mut app = app(Some(true));
    let saved = vec![("cart".to_string(), Visibility::Inherited)];
    run(
        &mut app,
        Record {
            verb: "isolate",
            named: false,
            changes: vec![Change::Isolated {
                before: None,
                after: Some(saved.clone()),
            }],
            open: false,
        },
    );
    app.world_mut().resource_mut::<Isolated>().0 = Some(saved.clone());
    run(&mut app, UndoCommand::Undo);
    assert_eq!(app.world().resource::<Isolated>().0, None);
    run(&mut app, UndoCommand::Redo);
    assert_eq!(app.world().resource::<Isolated>().0, Some(saved));
}

#[test]
fn another_world_clears_and_a_reset_forgets_the_moves() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    run(&mut app, moved(cart, 1.0, 2.0, false));
    // Reconnecting to the same world keeps it.
    app.world_mut().resource_mut::<GzStatus>().world = None;
    app.update();
    app.world_mut().resource_mut::<GzStatus>().world = Some("w".into());
    app.update();
    assert!(stack(&app).is_dirty());
    // A Reset puts every pose back: the moves go, a hide stays.
    run(
        &mut app,
        Record {
            verb: "hide",
            named: true,
            changes: vec![Change::Visibility {
                entity: cart,
                before: Visibility::Inherited,
                after: Visibility::Hidden,
            }],
            open: false,
        },
    );
    run(&mut app, GzCommand::Reset);
    assert_eq!(stack(&app).undo_label(), Some("hide cart"));
    assert!(stack(&app).is_dirty());
    run(&mut app, UndoCommand::Undo);
    assert_eq!(stack(&app).undo_label(), None);
    // Another world: cleared.
    run(&mut app, moved(cart, 1.0, 2.0, false));
    app.world_mut().resource_mut::<Isolated>().0 = Some(Vec::new());
    app.world_mut().resource_mut::<GzStatus>().world = Some("v".into());
    app.update();
    assert_eq!(*stack(&app), UndoStack::default());
    assert_eq!(app.world().resource::<Isolated>().0, None);
}
