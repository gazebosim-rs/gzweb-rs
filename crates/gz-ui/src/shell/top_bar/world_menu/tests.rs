//! The World menu, the Save as dialog, Snap and the status bar's note.

use std::time::Duration;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::spawn::{GzEntity, GzIndex, GzWorldRoot};
use gz_bevy::undo::{Change, Record, UndoStack};
use gz_bevy::view::gizmo::Snap;
use gz_bevy::world_save::{SaveWorld, WorldSaved};
use gz_core::scene::NodeKind;

use super::super::save_dialog::{CurrentPoses, DialogButton, DialogMode, DialogName, SaveDialog};
use super::super::tools::{snap_label, SnapField, SnapTrigger};
use super::*;
use crate::shell::connection::ConnectionView;
use crate::shell::status_bar::readouts::{MessageChip, StatusNote};
use crate::shell::viewport::connect_card::{ConnectButton, ConnectCard};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::text_field::{set_field_text, TextFieldInput};
use crate::widgets::NumberCommit;

/// The saves asked for so far.
#[derive(Resource, Default)]
struct Saves(Vec<SaveWorld>);

fn record(mut saves: MessageReader<SaveWorld>, mut got: ResMut<Saves>) {
    got.0.extend(saves.read().cloned());
}

/// The shell live on a world with a model, saves recorded.
fn live() -> (App, Entity) {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )))
    .init_resource::<Saves>()
    .add_systems(Update, record);
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let cart = world
        .spawn((
            GzEntity {
                name: "cart".into(),
                id: 1,
                kind: NodeKind::Model,
                path: "cart".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    go_live(&mut app, false);
    (app, cart)
}

fn item(app: &mut App, which: WorldItem) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &WorldItem)>()
        .iter(world)
        .find(|(_, i)| **i == which)
        .unwrap()
        .0
}

fn header(app: &mut App, which: WorldHeader) -> String {
    let world = app.world_mut();
    world
        .query::<(&WorldHeader, &Text)>()
        .iter(world)
        .find(|(h, _)| **h == which)
        .map(|(_, t)| t.0.clone())
        .unwrap()
}

fn saves(app: &mut App) -> Vec<SaveWorld> {
    std::mem::take(&mut app.world_mut().resource_mut::<Saves>().0)
}

fn activate(app: &mut App, e: Entity) {
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 2);
}

/// The header names the file and its state; what the server can't do is
/// unavailable; Undo names its edit; Save asks for the authored world;
/// a finished save says so in the status bar for 3 s and the header
/// remembers when.
#[test]
fn the_world_menu_follows_the_world_and_saves_it() {
    let (mut app, cart) = live();
    assert_eq!(header(&mut app, WorldHeader::Name), "warehouse_demo.sdf");
    assert_eq!(header(&mut app, WorldHeader::State), "saved");
    for never in [
        WorldItem::New,
        WorldItem::Open,
        WorldItem::Recent,
        WorldItem::Revert,
        WorldItem::Import,
    ] {
        let e = item(&mut app, never);
        assert!(
            app.world().get::<InteractionDisabled>(e).is_some(),
            "{never:?}"
        );
    }
    let undo = item(&mut app, WorldItem::Undo);
    assert!(app.world().get::<InteractionDisabled>(undo).is_some());

    app.world_mut().write_message(Record {
        verb: "move",
        named: true,
        changes: vec![Change::Pose {
            entity: cart,
            before: Transform::default(),
            after: Transform::from_xyz(1.0, 0.0, 0.0),
        }],
        scrub: None,
        open: false,
    });
    frames(&mut app, 2);
    assert_eq!(header(&mut app, WorldHeader::State), "unsaved changes");
    assert!(app.world().get::<InteractionDisabled>(undo).is_none());
    assert!(all_text(&mut app).contains("Undo move cart"));

    let save = item(&mut app, WorldItem::Save);
    activate(&mut app, save);
    assert_eq!(
        saves(&mut app),
        [SaveWorld {
            name: "warehouse_demo.sdf".into(),
            current_poses: false,
            mark_saved: true,
        }]
    );
    // The save finishes.
    app.world_mut().resource_mut::<UndoStack>().mark_saved();
    app.world_mut().write_message(WorldSaved {
        result: Ok("/tmp/out/warehouse_demo.sdf".into()),
        marked: true,
    });
    frames(&mut app, 2);
    let chip = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<&MessageChip>()
            .iter(world)
            .next()
            .map(|c| c.text.clone())
    };
    assert_eq!(chip(&mut app).as_deref(), Some("Saved warehouse_demo.sdf"));
    assert_eq!(header(&mut app, WorldHeader::State), "saved");
    frames(&mut app, 32);
    assert_eq!(chip(&mut app), None, "gone after 3 s");
    assert!(app.world().resource::<StatusNote>().0.is_none());
    // Dirty again: when it was last saved.
    app.world_mut().write_message(Record {
        verb: "move",
        named: true,
        changes: vec![Change::Pose {
            entity: cart,
            before: Transform::from_xyz(1.0, 0.0, 0.0),
            after: Transform::from_xyz(2.0, 0.0, 0.0),
        }],
        scrub: None,
        open: false,
    });
    frames(&mut app, 2);
    assert!(header(&mut app, WorldHeader::State).starts_with("unsaved changes · last saved "));

    // A failed save says nothing saved and leaves the time (it logs why,
    // through gz-bevy's log, which tests don't collect).
    let at = app.world().resource::<LastSaved>().0.clone();
    app.world_mut().write_message(WorldSaved {
        result: Err("The server didn't answer".into()),
        marked: false,
    });
    frames(&mut app, 2);
    assert_eq!(chip(&mut app), None);
    assert_eq!(app.world().resource::<LastSaved>().0, at);
    // Another world wasn't saved this session.
    assert!(at.is_some());
    app.world_mut().resource_mut::<GzStatus>().world = Some("other".into());
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<LastSaved>().0, None);
}

/// A long edit's name is cut so Undo and its key fit the menu.
#[test]
fn long_undo_labels_end_in_an_ellipsis() {
    assert_eq!(fit_label("Undo move cart"), "Undo move cart");
    let long = fit_label("Undo move warehouse_shelf_with_boxes_03");
    assert_eq!(long.chars().count(), 26);
    assert!(long.starts_with("Undo move warehouse_") && long.ends_with('…'));
}

/// Typing a letter in the World menu jumps to the next item starting
/// with it (its items sit in a column inside the popup).
#[test]
fn the_world_menu_takes_type_to_search() {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    let (mut app, _) = live();
    let save = item(&mut app, WorldItem::Save);
    let export = item(&mut app, WorldItem::ExportSdf);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(save, FocusCause::Navigated);
    let window = find::<Window>(&mut app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyE,
        logical_key: Key::Character("e".into()),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 1);
    assert_eq!(focused(&app), Some(export));
}

fn dialog_open(app: &App) -> Option<DialogMode> {
    app.world().resource::<SaveDialog>().0.as_ref().map(|d| d.0)
}

fn name_input(app: &mut App) -> Entity {
    let world = app.world_mut();
    let name = world
        .query_filtered::<Entity, With<DialogName>>()
        .single(world)
        .unwrap();
    world
        .query_filtered::<(Entity, &ChildOf), With<TextFieldInput>>()
        .iter(world)
        .find(|(_, p)| p.parent() == name)
        .unwrap()
        .0
}

fn dialog_button(app: &mut App, which: DialogButton) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &DialogButton)>()
        .iter(world)
        .find(|(_, b)| **b == which)
        .unwrap()
        .0
}

/// ⌘S saves; ⇧⌘S and Export open the dialog with the world's name (Export
/// with current poses); Save in it saves what it holds (`.sdf` added);
/// Cancel and Esc close it.
#[test]
fn save_as_and_export_ask_for_a_name() {
    let (mut app, _) = live();
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyS]);
    frames(&mut app, 1);
    assert_eq!(saves(&mut app).len(), 1);
    chord(
        &mut app,
        &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyS],
    );
    frames(&mut app, 3);
    assert_eq!(dialog_open(&app), Some(DialogMode::SaveAs));
    let input = name_input(&mut app);
    assert_eq!(focused(&app), Some(input));
    assert_eq!(
        app.world().get::<EditableText>(input).unwrap().value(),
        "warehouse_demo.sdf"
    );
    let poses = find::<CurrentPoses>(&mut app);
    assert!(app.world().get::<Checked>(poses).is_none());
    // A name that saves as nothing leaves the dialog as it is.
    set_field_text(
        &mut app.world_mut().get_mut::<EditableText>(input).unwrap(),
        ".sdf",
    );
    frames(&mut app, 1);
    let save = dialog_button(&mut app, DialogButton::Save);
    activate(&mut app, save);
    assert!(saves(&mut app).is_empty());
    let input = name_input(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(input).unwrap().value(),
        ".sdf"
    );
    // A click on the toggle turns it on and gives the keys back to the
    // name.
    let poses = find::<CurrentPoses>(&mut app);
    click(&mut app, poses);
    frames(&mut app, 2);
    assert!(app.world().get::<Checked>(poses).is_some());
    assert_eq!(focused(&app), Some(input));
    // A path saves as its last part.
    set_field_text(
        &mut app.world_mut().get_mut::<EditableText>(input).unwrap(),
        "../copy",
    );
    frames(&mut app, 1);
    let save = dialog_button(&mut app, DialogButton::Save);
    activate(&mut app, save);
    assert_eq!(
        saves(&mut app),
        [SaveWorld {
            name: "copy.sdf".into(),
            current_poses: true,
            mark_saved: true,
        }]
    );
    assert_eq!(dialog_open(&app), None);

    let export = item(&mut app, WorldItem::ExportSdf);
    activate(&mut app, export);
    frames(&mut app, 2);
    assert_eq!(dialog_open(&app), Some(DialogMode::Export));
    let poses = find::<CurrentPoses>(&mut app);
    assert!(app.world().get::<Checked>(poses).is_some());
    let name = find::<DialogName>(&mut app);
    app.world_mut().trigger(crate::widgets::TextSubmit {
        entity: name,
        value: "snap.sdf".into(),
    });
    frames(&mut app, 2);
    assert_eq!(
        saves(&mut app),
        [SaveWorld {
            name: "snap.sdf".into(),
            current_poses: true,
            mark_saved: false,
        }]
    );

    app.world_mut()
        .resource_mut::<SaveDialog>()
        .open(DialogMode::SaveAs, None);
    frames(&mut app, 3);
    let cancel = dialog_button(&mut app, DialogButton::Cancel);
    activate(&mut app, cancel);
    assert_eq!(dialog_open(&app), None);
    app.world_mut()
        .resource_mut::<SaveDialog>()
        .open(DialogMode::SaveAs, None);
    frames(&mut app, 3);
    let input = name_input(&mut app);
    focused_key(&mut app, input, KeyCode::Escape);
    frames(&mut app, 2);
    assert_eq!(dialog_open(&app), None);
    assert!(saves(&mut app).is_empty());
}

/// Connect to server… shows the connect card over the world, with
/// Cancel; Cancel hides it again.
#[test]
fn connect_shows_the_card_over_the_world() {
    let (mut app, _) = live();
    let card = find::<ConnectCard>(&mut app);
    frames(&mut app, 5);
    assert!(!displayed(&app, card));
    let connect = item(&mut app, WorldItem::Connect);
    assert!(
        all_text(&mut app).contains("localhost"),
        "the address as its hint"
    );
    activate(&mut app, connect);
    frames(&mut app, 2);
    assert!(app.world().resource::<ConnectionView>().switching);
    assert!(displayed(&app, card));
    let cancel = connect_button(&mut app, ConnectButton::Cancel);
    assert!(displayed(&app, cancel));
    activate(&mut app, cancel);
    frames(&mut app, 5);
    assert!(!app.world().resource::<ConnectionView>().switching);
    assert!(!displayed(&app, card));
}

/// The Snap popover's fields set the steps, and the trigger reads the
/// translate step.
#[test]
fn snap_sets_the_steps() {
    let (mut app, _) = live();
    assert_eq!(snap_label(&Snap::default()), "Snap 0.25");
    let field = |app: &mut App, which: SnapField| {
        let world = app.world_mut();
        world
            .query::<(Entity, &SnapField)>()
            .iter(world)
            .find(|(_, f)| **f == which)
            .unwrap()
            .0
    };
    let length = field(&mut app, SnapField::Length);
    app.world_mut().trigger(NumberCommit {
        entity: length,
        value: 0.5,
        is_final: true,
    });
    let angle = field(&mut app, SnapField::Angle);
    app.world_mut().trigger(NumberCommit {
        entity: angle,
        value: 45.0,
        is_final: true,
    });
    frames(&mut app, 2);
    let snap = *app.world().resource::<Snap>();
    assert_eq!(snap.length, 0.5);
    assert!((snap.angle - 45f32.to_radians()).abs() < 1e-6);
    let trigger = find::<SnapTrigger>(&mut app);
    let label = app
        .world()
        .get::<bevy::a11y::AccessibilityNode>(trigger)
        .and_then(|a| a.label().map(str::to_string));
    assert_eq!(label.as_deref(), Some("Snap 0.5"));
}

/// A captured image encodes as a PNG; a path's last part is its name.
#[test]
fn screenshots_encode_as_png() {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let image = Image::new_fill(
        Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let png = png_bytes(&image).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    // A window's capture is usually BGRA.
    let bgra = Image::new_fill(
        Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 255, 255],
        TextureFormat::Bgra8UnormSrgb,
        default(),
    );
    let png = png_bytes(&bgra).unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    assert_eq!(&buf[..4], &[255, 0, 0, 255], "red, as RGBA");
    assert_eq!(file_name("/a/b/w.sdf"), "w.sdf");
    assert_eq!(file_name("w.sdf"), "w.sdf");
    assert_eq!(default_name(None), "world.sdf");
}

/// A typed name saves as its last part with `.sdf`; one empty without
/// them saves as nothing.
#[test]
fn typed_names_save_as_a_file_name() {
    use super::super::save_dialog::file_name_of;
    assert_eq!(file_name_of(" copy ").as_deref(), Some("copy.sdf"));
    assert_eq!(file_name_of("copy.sdf").as_deref(), Some("copy.sdf"));
    assert_eq!(file_name_of("../a/b").as_deref(), Some("b.sdf"));
    assert_eq!(file_name_of("/abs/w.sdf").as_deref(), Some("w.sdf"));
    assert_eq!(file_name_of(r"C:\w\x").as_deref(), Some("x.sdf"));
    for none in ["", "  ", ".sdf", "a/", "..", "a/.sdf"] {
        assert_eq!(file_name_of(none), None, "{none:?}");
    }
}
