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

/// The inspector field a scrub runs on.
const FIELD: Entity = Entity::PLACEHOLDER;

/// A pose field's commit (an `open` scrub step, or its release).
fn moved(entity: Entity, before: f32, after: f32, open: bool) -> Record {
    Record {
        verb: "move",
        named: true,
        changes: vec![Change::Pose {
            entity,
            before: Transform::from_xyz(before, 0.0, 0.0),
            after: Transform::from_xyz(after, 0.0, 0.0),
        }],
        scrub: Some(FIELD),
        open,
    }
}

/// A gizmo drag's.
fn dragged(entity: Entity, before: f32, after: f32) -> Record {
    Record {
        scrub: None,
        ..moved(entity, before, after, false)
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
            scrub: None,
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
            scrub: None,
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
            scrub: None,
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

#[test]
fn a_scrub_cut_short_doesnt_swallow_the_next_move() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    // A scrub whose release never came, then a drag.
    run(&mut app, moved(cart, 1.0, 1.5, true));
    run(&mut app, dragged(cart, 1.5, 3.0));
    run(&mut app, UndoCommand::Undo);
    assert_eq!(x(&app, cart), 1.5);
    run(&mut app, UndoCommand::Undo);
    assert_eq!(x(&app, cart), 1.0);
    assert!(!stack(&app).is_dirty());
    // One that came back to where it began leaves nothing.
    run(&mut app, moved(cart, 1.0, 1.5, true));
    run(&mut app, moved(cart, 1.5, 1.0, true));
    run(&mut app, dragged(cart, 1.0, 2.0));
    run(&mut app, UndoCommand::Undo);
    assert_eq!((x(&app, cart), stack(&app).undo_label()), (1.0, None));
    assert!(!stack(&app).is_dirty());
}

fn hide(entity: Entity) -> Record {
    Record {
        verb: "hide",
        named: true,
        changes: vec![Change::Visibility {
            entity,
            before: Visibility::Inherited,
            after: Visibility::Hidden,
        }],
        scrub: None,
        open: false,
    }
}

#[test]
fn after_a_reset_undoing_the_hides_left_is_the_world_as_loaded() {
    let mut app = app(Some(true));
    let cart = spawn(&mut app, "cart");
    run(&mut app, moved(cart, 1.0, 2.0, false));
    run(&mut app, hide(cart));
    run(&mut app, GzCommand::Reset);
    assert!(stack(&app).is_dirty());
    run(&mut app, UndoCommand::Undo);
    assert!(!stack(&app).is_dirty());
}

#[test]
fn a_reconnect_forgets_the_hides_and_a_restart_the_moves() {
    use crate::connection::SimStats;
    let mut app = app(Some(true));
    app.insert_resource(GzIndex {
        scenes: 1,
        ..Default::default()
    });
    let cart = spawn(&mut app, "cart");
    app.update();
    run(&mut app, moved(cart, 1.0, 2.0, false));
    run(&mut app, hide(cart));
    app.world_mut().resource_mut::<Isolated>().0 = Some(Vec::new());
    // The same world's scene again: it comes with everything shown.
    app.world_mut().resource_mut::<GzIndex>().scenes = 2;
    app.update();
    assert_eq!(stack(&app).undo_label(), Some("move cart"));
    assert_eq!(app.world().resource::<Isolated>().0, None);
    // The sim's iterations going back: it was reset, or restarted.
    let stats = |iterations| SimStats {
        paused: false,
        sim_time: 0.0,
        real_time: 0.0,
        real_time_factor: 1.0,
        iterations,
        step_size: 0.001,
    };
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(500));
    app.update();
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(600));
    app.update();
    assert!(stack(&app).is_dirty());
    app.world_mut().resource_mut::<GzStatus>().sim = Some(stats(3));
    app.update();
    let s = stack(&app);
    assert!(s.undo_label().is_none() && s.redo_label().is_none() && !s.is_dirty());
    // Another server is another world: cleared.
    run(&mut app, moved(cart, 1.0, 2.0, false));
    app.world_mut().resource_mut::<GzStatus>().url = "ws://other".into();
    app.update();
    assert_eq!(*stack(&app), UndoStack::default());
}

/// A delete undone creates the model again from its SDFormat, at its pose
/// and under its name; redone, removes it again. A spawn undone removes
/// it. A Reset forgets both (gz-sim puts the world back as loaded).
#[test]
fn a_delete_or_a_spawn_is_undone_through_the_server() {
    let mut app = app(Some(true));
    let model = ModelCopy {
        name: "cart".into(),
        sdf: "<sdf version=\"1.9\"><model name=\"cart\"/></sdf>".into(),
        pose: Transform::from_xyz(1.0, 2.0, 0.0),
        loaded: None,
    };
    let requests = |app: &mut App| -> Vec<(String, Vec<u8>)> {
        app.world_mut()
            .resource_mut::<Messages<GzCommand>>()
            .drain()
            .filter_map(|c| match c {
                GzCommand::Request {
                    service, payload, ..
                } => Some((service, payload)),
                _ => None,
            })
            .collect()
    };
    app.world_mut().write_message(Record {
        verb: "delete",
        named: true,
        changes: vec![Change::Exists {
            model: model.clone(),
            after: false,
        }],
        scrub: None,
        open: false,
    });
    app.update();
    let stack = app.world().resource::<UndoStack>();
    assert_eq!(stack.undo_label(), Some("delete cart"));
    assert!(stack.is_dirty());
    requests(&mut app);
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    let sent = requests(&mut app);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "/world/w/create");
    let factory = msgs::EntityFactory::decode(sent[0].1.as_slice()).unwrap();
    assert_eq!(factory.name, "cart");
    assert!(!factory.allow_renaming);
    assert_eq!(factory.pose.unwrap().position.unwrap().y, 2.0);
    assert_eq!(
        factory.from,
        Some(msgs::entity_factory::From::Sdf(model.sdf.clone()))
    );
    app.world_mut().write_message(UndoCommand::Redo);
    app.update();
    let sent = requests(&mut app);
    assert_eq!(sent[0].0, "/world/w/remove");
    let entity = msgs::Entity::decode(sent[0].1.as_slice()).unwrap();
    assert_eq!(entity.name, "cart");
    assert_eq!(entity.r#type, msgs::entity::Type::Model as i32);
    // A spawn undone removes the copy.
    app.world_mut().write_message(Record {
        verb: "duplicate",
        named: true,
        changes: vec![Change::Exists {
            model: ModelCopy {
                name: "cart_1".into(),
                ..model
            },
            after: true,
        }],
        scrub: None,
        open: false,
    });
    app.update();
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    let sent = requests(&mut app);
    assert_eq!(sent[0].0, "/world/w/remove");
    assert_eq!(
        msgs::Entity::decode(sent[0].1.as_slice()).unwrap().name,
        "cart_1"
    );
    app.world_mut().write_message(GzCommand::Reset);
    app.update();
    let stack = app.world().resource::<UndoStack>();
    assert_eq!((stack.undo_label(), stack.redo_label()), (None, None));
}

/// A deleted model too large for the server to take whole isn't created
/// again: nothing is sent and the delete stays where it is.
#[test]
fn a_model_too_large_to_send_isnt_created_again() {
    let mut app = app(Some(true));
    let sdf = format!(
        "<sdf version=\"1.9\"><model name=\"cart\"><!--{}--></model></sdf>",
        "x".repeat(gz_core::transport::MAX_FRAME)
    );
    app.world_mut().write_message(Record {
        verb: "delete",
        named: true,
        changes: vec![Change::Exists {
            model: ModelCopy {
                name: "cart".into(),
                sdf,
                pose: Transform::IDENTITY,
                loaded: None,
            },
            after: false,
        }],
        scrub: None,
        open: false,
    });
    app.update();
    app.world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .clear();
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    let sent = app
        .world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .drain()
        .filter(|c| matches!(c, GzCommand::Request { .. }))
        .count();
    assert_eq!(sent, 0);
    let stack = app.world().resource::<UndoStack>();
    assert_eq!(stack.undo_label(), Some("delete cart"));
}

/// Without edits, a delete stays where it is.
#[test]
fn a_delete_waits_for_a_server_that_takes_edits() {
    let mut app = app(Some(false));
    app.world_mut().write_message(Record {
        verb: "delete",
        named: true,
        changes: vec![Change::Exists {
            model: ModelCopy {
                name: "cart".into(),
                sdf: String::new(),
                pose: Transform::IDENTITY,
                loaded: None,
            },
            after: false,
        }],
        scrub: None,
        open: false,
    });
    app.update();
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    let stack = app.world().resource::<UndoStack>();
    assert_eq!(stack.undo_label(), Some("delete cart"));
}

/// Undoing a delete brings the model's pose as loaded back for Save; a
/// move of it waits until the model is back, then undoes.
#[test]
fn a_move_waits_for_its_model_to_come_back() {
    use crate::world_save::LoadedPoses;
    let mut app = app(Some(true));
    app.init_resource::<LoadedPoses>();
    let cart = spawn(&mut app, "cart");
    run(&mut app, dragged(cart, 0.0, 4.0));
    app.world_mut().write_message(Record {
        verb: "delete",
        named: true,
        changes: vec![Change::Exists {
            model: ModelCopy {
                name: "cart".into(),
                sdf: String::new(),
                pose: Transform::from_xyz(4.0, 0.0, 0.0),
                loaded: Some(Transform::from_xyz(9.0, 0.0, 0.0)),
            },
            after: false,
        }],
        scrub: None,
        open: false,
    });
    app.update();
    app.world_mut().entity_mut(cart).despawn();
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    assert_eq!(
        app.world().resource::<LoadedPoses>().0.get("cart"),
        Some(&Transform::from_xyz(9.0, 0.0, 0.0))
    );
    assert_eq!(stack(&app).undo_label(), Some("move cart"));
    // Not back yet: the move stays.
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    assert_eq!(stack(&app).undo_label(), Some("move cart"));
    // Back: it undoes.
    spawn(&mut app, "cart");
    app.update();
    app.world_mut().write_message(UndoCommand::Undo);
    app.update();
    assert_eq!(stack(&app).undo_label(), None);
}
