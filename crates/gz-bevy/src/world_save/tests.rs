//! Saving the world: the request, the authored poses, the file and the
//! saved state.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::scene::NodeKind;
use gz_core::transport::Status;
use prost::Message as _;

use super::*;
use crate::edit::EditPlugin;
use crate::undo::UndoPlugin;

const SDF: &str = r#"<sdf version="1.9"><world name="w">
  <model name="cart"><pose>9 9 0 0 0 0</pose></model>
  <model name="box"><pose>3 0 0 0 0 0</pose></model>
</world></sdf>"#;

fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, EditPlugin, UndoPlugin, WorldSavePlugin))
        .insert_resource(GzStatus {
            status: Status::Ready,
            world: Some("w".into()),
            ..GzStatus::new("ws://test")
        });
    let root = app
        .world_mut()
        .spawn((
            Transform::default(),
            GlobalTransform::default(),
            GzWorldRoot,
        ))
        .id();
    let cart = app
        .world_mut()
        .spawn((
            GzEntity {
                name: "cart".into(),
                id: 1,
                kind: NodeKind::Model,
                path: "cart".into(),
            },
            Transform::from_xyz(1.0, 2.0, 0.0),
            ChildOf(root),
        ))
        .id();
    app.update();
    (app, cart)
}

fn requests(app: &mut App) -> Vec<String> {
    app.world_mut()
        .resource_mut::<Messages<GzCommand>>()
        .drain()
        .filter_map(|c| match c {
            GzCommand::Request { service, .. } => Some(service),
            _ => None,
        })
        .collect()
}

fn reply(app: &mut App, data: &str) {
    app.world_mut().write_message(GzServiceReply {
        service: "/world/w/generate_world_sdf".into(),
        msg_type: "gz.msgs.StringMsg".into(),
        payload: msgs::StringMsg {
            data: data.into(),
            ..default()
        }
        .encode_to_vec(),
    });
    app.update();
}

fn saved(app: &mut App) -> Vec<Result<String, String>> {
    app.world_mut()
        .resource_mut::<Messages<WorldSaved>>()
        .drain()
        .map(|s| s.result)
        .collect()
}

/// Save asks for the SDFormat and writes it with each model where it was
/// loaded (not where physics took it), the rest as the server has it; it
/// marks the world saved. "Use current poses" keeps the server's.
#[test]
fn a_save_writes_the_authored_world() {
    let (mut app, cart) = app();
    // Physics moves the cart after load.
    app.world_mut()
        .get_mut::<Transform>(cart)
        .unwrap()
        .translation = Vec3::new(7.0, 7.0, 0.0);
    let name = format!("gz-bevy-world-save-{}.sdf", std::process::id());
    app.world_mut().write_message(SaveWorld {
        name: name.clone(),
        current_poses: false,
        mark_saved: true,
    });
    app.update();
    assert_eq!(requests(&mut app), ["/world/w/generate_world_sdf"]);
    reply(&mut app, SDF);
    let path = saved(&mut app).remove(0).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(r#"<model name="cart"><pose>1 2 0 0 0 0</pose>"#),
        "{text}"
    );
    assert!(
        text.contains(r#"<model name="box"><pose>3 0 0 0 0 0</pose>"#),
        "{text}"
    );
    std::fs::remove_file(&path).unwrap();
    assert!(!app.world().resource::<UndoStack>().is_dirty());

    // The user places the cart: that's where it saves, and the world is
    // dirty until then.
    app.world_mut().write_message(crate::undo::Record {
        verb: "move",
        named: true,
        changes: vec![crate::undo::Change::Pose {
            entity: cart,
            before: Transform::from_xyz(1.0, 2.0, 0.0),
            after: Transform::from_xyz(4.0, 5.0, 0.0),
        }],
        scrub: None,
        open: false,
    });
    app.update();
    assert!(app.world().resource::<UndoStack>().is_dirty());
    app.world_mut().write_message(SaveWorld {
        name: name.clone(),
        current_poses: false,
        mark_saved: true,
    });
    app.update();
    reply(&mut app, SDF);
    let path = saved(&mut app).remove(0).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(r#"<model name="cart"><pose>4 5 0 0 0 0</pose>"#),
        "{text}"
    );
    std::fs::remove_file(&path).unwrap();
    assert!(!app.world().resource::<UndoStack>().is_dirty());

    app.world_mut().write_message(SaveWorld {
        name,
        current_poses: true,
        mark_saved: false,
    });
    app.update();
    reply(&mut app, SDF);
    let path = saved(&mut app).remove(0).unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("9 9 0 0 0 0"));
    std::fs::remove_file(&path).unwrap();
}

/// No live world, a reply that isn't a world, or none in time: the save
/// says why; a reply nobody asked for is left alone.
#[test]
fn a_save_says_why_it_failed() {
    let (mut app, _) = app();
    reply(&mut app, SDF);
    assert!(saved(&mut app).is_empty(), "nobody asked");
    let save = SaveWorld {
        name: "unused.sdf".into(),
        current_poses: false,
        mark_saved: true,
    };
    app.world_mut().write_message(save.clone());
    app.update();
    reply(&mut app, "service_not_found");
    assert_eq!(
        saved(&mut app),
        [Err("The server didn't send the world".to_string())]
    );
    app.world_mut().write_message(save.clone());
    app.update();
    reply(&mut app, "<not xml");
    assert!(saved(&mut app)[0].is_err());
    // No reply in time.
    app.world_mut().write_message(save.clone());
    app.update();
    app.world_mut().resource_mut::<SaveJobs>().0[0].1 -= SAVE_TIMEOUT_SECS + 1.0;
    app.update();
    assert_eq!(
        saved(&mut app),
        [Err("The server didn't answer".to_string())]
    );
    app.world_mut().resource_mut::<GzStatus>().status = Status::Connecting;
    app.world_mut().write_message(save);
    app.update();
    assert_eq!(saved(&mut app), [Err("Not connected".to_string())]);
}
