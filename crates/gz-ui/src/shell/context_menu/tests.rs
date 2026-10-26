//! The entity context menu, the edit keys and Delete's undo chip.

use std::time::Duration;

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonState;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, MenuAction, MenuEvent};
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::{Overlay, OverlayCommand};
use gz_bevy::spawn::{GzEntity, GzIndex, GzWorldRoot};
use gz_bevy::undo::{Change, ModelCopy, Record, UndoCommand, UndoStack};
use gz_bevy::view::align::{AlignEdge, AlignModels};
use gz_bevy::view::controls::ActiveScopes;
use gz_bevy::view::input::Scope;
use gz_bevy::view::model_edits::{EditModels, ModelEdit, ModelsEdited};
use gz_bevy::view::{EntityContext, Selection};
use gz_core::scene::NodeKind;

use super::*;
use crate::shell::status_bar::readouts::{MessageChip, UndoChip};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::widgets::menu::{MenuPopupOf, SubmenuRow};

/// What the menu and the keys asked for.
#[derive(Resource, Default)]
struct Asked {
    edits: Vec<ModelEdit>,
    aligns: Vec<AlignModels>,
    overlays: Vec<OverlayCommand>,
    undos: Vec<UndoCommand>,
}

fn collect(
    mut edits: MessageReader<EditModels>,
    mut aligns: MessageReader<AlignModels>,
    mut overlays: MessageReader<OverlayCommand>,
    mut undos: MessageReader<UndoCommand>,
    mut asked: ResMut<Asked>,
) {
    asked.edits.extend(edits.read().map(|e| e.0));
    asked.aligns.extend(aligns.read().copied());
    asked.overlays.extend(overlays.read().copied());
    asked.undos.extend(undos.read().copied());
}

/// The shell live on a world with two models (cart selected), on a server
/// that takes edits.
fn live() -> (App, Entity, Entity) {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )))
    .init_resource::<Asked>()
    .add_systems(Update, collect);
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let model = |world: &mut World, name: &str, id| {
        world
            .spawn((
                GzEntity {
                    name: name.into(),
                    id,
                    kind: NodeKind::Model,
                    path: name.into(),
                },
                Transform::default(),
                Visibility::default(),
                ChildOf(root),
            ))
            .id()
    };
    let cart = model(world, "cart", 1);
    let box_ = model(world, "box", 2);
    world.resource_mut::<Selection>().entities = vec![cart];
    world.resource_mut::<GzStatus>().edits = Some(true);
    go_live(&mut app, false);
    (app, cart, box_)
}

fn open(app: &mut App, entity: Entity) {
    app.world_mut()
        .write_message(EntityContext { entity, view: None });
    frames(app, 3);
}

fn item(app: &mut App, which: ContextItem) -> (Entity, bool) {
    let world = app.world_mut();
    world
        .query::<(Entity, &ContextItem, Has<InteractionDisabled>)>()
        .iter(world)
        .find(|(_, i, _)| **i == which)
        .map(|(e, _, d)| (e, d))
        .unwrap_or_else(|| panic!("no {which:?}"))
}

fn menus(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query::<&ContextMenu>().iter(world).count()
}

fn popups_shown(app: &mut App) -> usize {
    let world = app.world_mut();
    world
        .query_filtered::<&Visibility, With<MenuPopupOf>>()
        .iter(world)
        .filter(|v| **v == Visibility::Visible)
        .count()
}

fn activate(app: &mut App, e: Entity) {
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 3);
}

/// It opens for its subject, shows what can run now and says why the rest
/// can't; choosing an item asks for it and closes the menu.
#[test]
fn the_menu_opens_for_its_subject_and_runs_an_item() {
    let (mut app, cart, _) = live();
    open(&mut app, cart);
    assert_eq!(menus(&mut app), 1);
    assert_eq!(popups_shown(&mut app), 1);
    let text = all_text(&mut app);
    for label in [
        "Focus",
        "Follow",
        "Move camera to",
        "Hide",
        "Duplicate",
        "Align",
        "Show",
        "Inspect",
        "Edit model",
    ] {
        assert!(text.contains(label), "{label} in {text}");
    }
    let disabled = |app: &mut App, i| item(app, i).1;
    assert!(disabled(&mut app, ContextItem::Rename));
    assert!(disabled(&mut app, ContextItem::ApplyForce));
    assert!(
        disabled(&mut app, ContextItem::Edit(ModelEdit::Paste)),
        "nothing copied"
    );
    assert!(
        disabled(&mut app, ContextItem::Align(0, AlignEdge::Min)),
        "one model"
    );
    assert!(!disabled(&mut app, ContextItem::Edit(ModelEdit::Delete)));
    let (delete, _) = item(&mut app, ContextItem::Edit(ModelEdit::Delete));
    activate(&mut app, delete);
    assert_eq!(app.world().resource::<Asked>().edits, [ModelEdit::Delete]);
    // A choice closes the menu (Bevy's click sends this), and it goes.
    app.world_mut().trigger(MenuEvent {
        source: delete,
        action: MenuAction::CloseAll,
    });
    frames(&mut app, 3);
    assert_eq!(menus(&mut app), 0);
    // Opening again replaces one, never stacks.
    open(&mut app, cart);
    open(&mut app, cart);
    assert_eq!(menus(&mut app), 1);
}

/// The menu opens at the pointer at any UI scale: the pointer is in window
/// pixels, the menu in UI pixels (D-118).
#[test]
fn the_menu_opens_at_the_pointer_at_any_ui_scale() {
    let (mut app, cart, _) = live();
    app.world_mut().resource_mut::<Theme>().set_scale(1.5);
    frames(&mut app, 2);
    let at = Vec2::new(600.0, 300.0);
    let world = app.world_mut();
    let mut window = world.query::<&mut Window>().single_mut(world).unwrap();
    window.set_cursor_position(Some(at));
    open(&mut app, cart);
    let holder = find::<ContextMenu>(&mut app);
    let (min, _) = rect(&app, holder);
    assert!((min * 1.5 - at).length() < 0.5, "{min}");
}

/// Without edits, the edits say why; Copy still runs.
#[test]
fn edits_wait_for_a_server_that_takes_them() {
    let (mut app, cart, _) = live();
    app.world_mut().resource_mut::<GzStatus>().edits = Some(false);
    open(&mut app, cart);
    let disabled = |app: &mut App, i| item(app, i).1;
    assert!(disabled(&mut app, ContextItem::Edit(ModelEdit::Delete)));
    assert!(disabled(&mut app, ContextItem::Edit(ModelEdit::Duplicate)));
    assert!(!disabled(&mut app, ContextItem::Edit(ModelEdit::Copy)));
}

/// Hide hides the selection as one edit; open again, it reads Show.
#[test]
fn hide_hides_the_selection() {
    let (mut app, cart, box_) = live();
    app.world_mut().resource_mut::<Selection>().entities = vec![cart, box_];
    open(&mut app, cart);
    let (hide, _) = item(&mut app, ContextItem::Hide);
    activate(&mut app, hide);
    for e in [cart, box_] {
        assert_eq!(app.world().get::<Visibility>(e), Some(&Visibility::Hidden));
    }
    let undo = app.world().resource::<UndoStack>();
    assert_eq!(undo.undo_label(), Some("hide 2 entities"));
    open(&mut app, cart);
    let (hide, _) = item(&mut app, ContextItem::Hide);
    let label = app
        .world()
        .get::<crate::widgets::menu::MenuLabel>(hide)
        .unwrap()
        .0
        .clone();
    assert_eq!(label, "Show");
}

fn row_named(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &crate::widgets::menu::MenuLabel), With<SubmenuRow>>()
        .iter(world)
        .find(|(_, l)| l.0 == name)
        .unwrap()
        .0
}

fn press_in(app: &mut App, key_code: KeyCode) {
    let window = find::<Window>(app);
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key: bevy::input::keyboard::Key::Unidentified(
                bevy::input::keyboard::NativeKey::Unidentified,
            ),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
    frames(app, 2);
}

/// ↓ moves past a submenu's row like any item; →, Enter or a click opens
/// it, ← closes it alone.
#[test]
fn the_arrows_move_past_a_submenu_row() {
    use bevy::input_focus::{FocusCause, InputFocus};
    let (mut app, cart, _) = live();
    open(&mut app, cart);
    let (delete, _) = item(&mut app, ContextItem::Edit(ModelEdit::Delete));
    let (align, show) = (row_named(&mut app, "Align"), row_named(&mut app, "Show"));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(delete, FocusCause::Navigated);
    press_in(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Some(align));
    press_in(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Some(show));
    assert_eq!(popups_shown(&mut app), 1, "nothing opened on the way");
    press_in(&mut app, KeyCode::ArrowRight);
    assert_eq!(popups_shown(&mut app), 2);
    press_in(&mut app, KeyCode::ArrowLeft);
    assert_eq!(popups_shown(&mut app), 1);
    assert_eq!(focused(&app), Some(show));
    assert_eq!(menus(&mut app), 1, "the menu stays");
    click(&mut app, show);
    frames(&mut app, 2);
    assert_eq!(popups_shown(&mut app), 2);
}

/// Hovering Align opens its submenu over the menu; with two models
/// selected its items align them; a choice in it closes both.
#[test]
fn align_opens_on_hover_and_aligns() {
    let (mut app, cart, box_) = live();
    app.world_mut().resource_mut::<Selection>().entities = vec![cart, box_];
    open(&mut app, cart);
    let row = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<SubmenuRow>>()
            .iter(world)
            .next()
            .unwrap()
    };
    app.world_mut().entity_mut(row).insert(Hovered(true));
    frames(&mut app, 3);
    assert_eq!(popups_shown(&mut app), 2, "the menu and its submenu");
    let (y_max, disabled) = item(&mut app, ContextItem::Align(1, AlignEdge::Max));
    assert!(!disabled);
    activate(&mut app, y_max);
    assert_eq!(
        app.world().resource::<Asked>().aligns,
        [AlignModels {
            axis: 1,
            edge: AlignEdge::Max
        }]
    );
    app.world_mut().trigger(MenuEvent {
        source: y_max,
        action: MenuAction::CloseAll,
    });
    frames(&mut app, 3);
    assert_eq!(menus(&mut app), 0);
}

/// Show › turns an overlay over.
#[test]
fn show_turns_an_overlay_over() {
    let (mut app, cart, _) = live();
    open(&mut app, cart);
    let (collisions, _) = item(&mut app, ContextItem::Show(Overlay::Collisions));
    activate(&mut app, collisions);
    let asked = &app.world().resource::<Asked>().overlays;
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].overlay, Overlay::Collisions);
    assert_eq!(asked[0].on, Some(true));
}

/// Ctrl+C, Ctrl+V, Ctrl+D and Delete ask for the edits.
#[test]
fn the_edit_keys_ask_for_edits() {
    let (mut app, ..) = live();
    app.insert_resource(ActiveScopes(vec![
        Scope::Global,
        Scope::Viewport,
        Scope::Minimal,
        Scope::Text,
    ]));
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyC]);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyV]);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyD]);
    key(&mut app, KeyCode::Delete, ButtonState::Pressed);
    app.update();
    key(&mut app, KeyCode::Delete, ButtonState::Released);
    frames(&mut app, 2);
    assert_eq!(
        app.world().resource::<Asked>().edits,
        [
            ModelEdit::Copy,
            ModelEdit::Paste,
            ModelEdit::Duplicate,
            ModelEdit::Delete
        ]
    );
}

/// A delete shows "Deleted cart · Undo" for 5 s; clicking it undoes. A
/// copy shows its note without an Undo.
#[test]
fn a_delete_shows_an_undo_chip() {
    let (mut app, ..) = live();
    let chip = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(Entity, &MessageChip, Has<UndoChip>)>()
            .iter(world)
            .next()
            .map(|(e, c, u)| (e, c.text.clone(), c.count.clone(), u))
    };
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
    app.world_mut().write_message(ModelsEdited {
        edit: ModelEdit::Delete,
        result: Ok("Deleted cart".into()),
    });
    frames(&mut app, 2);
    let (e, text, count, undoes) = chip(&mut app).expect("a chip");
    assert_eq!(
        (text.as_str(), count.as_str(), undoes),
        ("Deleted cart", "Undo", true)
    );
    frames(&mut app, 35);
    assert!(chip(&mut app).is_some(), "still there after 3.5 s");
    app.world_mut().trigger(Activate { entity: e });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Asked>().undos, [UndoCommand::Undo]);
    assert!(chip(&mut app).is_none());

    app.world_mut().write_message(ModelsEdited {
        edit: ModelEdit::Copy,
        result: Ok("Copied cart".into()),
    });
    frames(&mut app, 2);
    let (_, text, count, undoes) = chip(&mut app).expect("a chip");
    assert_eq!(
        (text.as_str(), count.as_str(), undoes),
        ("Copied cart", "", false)
    );
    // A failed one says why.
    app.world_mut().write_message(ModelsEdited {
        edit: ModelEdit::Delete,
        result: Err("Not deleted: undo couldn't bring cart back".into()),
    });
    frames(&mut app, 2);
    let (_, text, _, undoes) = chip(&mut app).expect("a chip");
    assert_eq!(
        (text.as_str(), undoes),
        ("Not deleted: undo couldn't bring cart back", false)
    );
}

/// Typing a letter jumps to a submenu's row too, not into the submenu.
#[test]
fn typing_jumps_to_a_submenu_row() {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input_focus::{FocusCause, InputFocus};
    let (mut app, cart, _) = live();
    open(&mut app, cart);
    let (focus_item, _) = item(&mut app, ContextItem::Focus);
    let row = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &crate::widgets::menu::MenuLabel), With<SubmenuRow>>()
            .iter(world)
            .find(|(_, l)| l.0 == "Align")
            .unwrap()
            .0
    };
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(focus_item, FocusCause::Navigated);
    let window = find::<Window>(&mut app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 1);
    assert_eq!(focused(&app), Some(row), "Align");
}
