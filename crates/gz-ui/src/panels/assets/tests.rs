//! The Assets panel: its rules, and the panel through the shell.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::{GzCommand, GzServiceReply, GzStatus};
use gz_bevy::fuel::{FuelSearch, FuelState, GzFuel};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::place::{PlaceAt, PlaceHover, Placing};
use gz_core::fuel::FuelModel;
use gz_core::insert::{Insertable, LightKind, Shape};
use gz_core::msgs;
use prost::Message as _;

use super::model::*;
use super::systems::{
    local_model, Ghost, EDITING, NOT_CONNECTED, NO_EDITS, SEARCH_PROMPT, UNREACHABLE,
};
use super::view::{
    AssetsNote, Dismiss, FuelButton, FuelInsert, FuelMessage, InsertRows, LocalInsert, RowBar,
    TabChoice, TabPane, Tile,
};
use super::{AssetsStart, ASSETS};
use crate::shell::status_bar::readouts::PLACE_HINT;
use crate::shell::{ShellLayout, Slot, Tool, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

/// Inserts go sent → accepted → gone once the scene holds a new one of
/// its name; a refusal or silence fails; a model that never shows fails
/// late; a failed one stays until dismissed.
#[test]
fn inserts_follow_the_server_and_the_scene() {
    let mut inserts = Inserts::default();
    let boxed = Insertable::Shape(Shape::Box);
    inserts.sent(boxed.clone(), 0.0, vec!["box".into()]);
    let b = inserts.sent(Insertable::Light(LightKind::Point), 0.0, vec![]);
    inserts.answered(true, 1.0);
    assert_eq!(inserts.list[0].stage, InsertStage::Accepted);
    assert_eq!(inserts.list[1].stage, InsertStage::Sent);
    inserts.settle(["box"].into_iter(), 2.0);
    assert_eq!(inserts.list.len(), 2, "the old box isn't it");
    inserts.settle(["box", "box_0"].into_iter(), 2.0);
    assert_eq!(inserts.list.len(), 1);
    assert_eq!(inserts.list[0].id, b);
    inserts.settle([].into_iter(), REPLY_SECS + 1.0);
    assert_eq!(
        inserts.list[0].stage,
        InsertStage::Failed("The server didn't answer".into())
    );
    assert!(inserts.list[0].failed());
    inserts.dismiss(b);
    assert!(inserts.list.is_empty());

    let model = Insertable::Model {
        uri: "https://fuel/x".into(),
        name: "x".into(),
    };
    inserts.sent(model, 0.0, vec![]);
    assert!(inserts.loading("https://fuel/x"));
    assert!(!inserts.loading("https://fuel/y"));
    // The light's late answer first, then the model's.
    inserts.answered(false, 0.0);
    inserts.answered(true, 0.0);
    assert_eq!(inserts.list[0].status(), "Loading on the server…");
    inserts.settle([].into_iter(), APPEAR_SECS + 1.0);
    assert_eq!(inserts.list[0].status(), "x didn't show up in the world");
    inserts.sent(boxed, 0.0, vec![]);
    inserts.answered(false, 0.0);
    assert_eq!(inserts.list[1].status(), "The server couldn't insert Box");
}

/// A reply that comes after its insert timed out is that insert's, not
/// the next one's.
#[test]
fn a_late_reply_is_not_the_next_inserts() {
    let mut inserts = Inserts::default();
    inserts.sent(Insertable::Shape(Shape::Box), 0.0, vec![]);
    inserts.settle([].into_iter(), REPLY_SECS + 1.0);
    assert!(inserts.list[0].failed());
    inserts.sent(Insertable::Shape(Shape::Cone), 20.0, vec![]);
    inserts.answered(false, 21.0);
    assert_eq!(
        inserts.list[1].stage,
        InsertStage::Sent,
        "the box's late no"
    );
    inserts.answered(true, 22.0);
    assert_eq!(inserts.list[1].stage, InsertStage::Accepted);
}

/// A local model is named after its URI's last part.
#[test]
fn a_local_model_is_named_after_its_uri() {
    let name = |u: &str| match local_model(u) {
        Some(Insertable::Model { name, .. }) => name,
        other => panic!("{other:?}"),
    };
    assert_eq!(name("model://warehouse_shelf"), "warehouse_shelf");
    assert_eq!(name("/home/u/models/My%20Robot/"), "my_robot");
    assert_eq!(name("/srv/world/thing.sdf"), "thing");
    assert_eq!(name("/home/u/models/shelf/model.sdf"), "shelf");
    assert_eq!(
        name("https://fuel.gazebosim.org/1.0/OpenRobotics/models/Euro%20pallet/3"),
        "euro_pallet"
    );
    assert_eq!(
        name("https://fuel.gazebosim.org/1.0/o/models/m/tip/files"),
        "m"
    );
    assert_eq!(local_model("  "), None);
}

/// The start's tab and query parse; a query wants the Fuel tab.
#[test]
fn a_start_names_a_tab() {
    assert!("fuel:pallet".parse::<AssetsStart>().is_ok());
    assert!("Lights".parse::<AssetsStart>().is_ok());
    assert!("shapes:x".parse::<AssetsStart>().is_err());
    assert!("parts".parse::<AssetsStart>().is_ok());
    assert!("parts:x".parse::<AssetsStart>().is_err());
}

// Through the shell.

fn open(app: &mut App) {
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: ASSETS,
        slot: Slot::Primary,
    });
    frames(app, 2);
}

/// The panel open on a live world whose server takes edits.
fn live_assets() -> App {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    frames(&mut app, 2);
    open(&mut app);
    go_live(&mut app, false);
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    frames(&mut app, 2);
    sent(&mut app);
    app
}

fn tile(app: &mut App, what: &Insertable) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Tile)>()
        .iter(world)
        .find(|(_, t)| t.0 == *what)
        .unwrap()
        .0
}

fn note(app: &mut App) -> Option<String> {
    let world = app.world_mut();
    let (text, node) = world
        .query_filtered::<(&Text, &Node), With<AssetsNote>>()
        .single(world)
        .unwrap();
    (node.display != Display::None).then(|| text.0.clone())
}

fn armed(app: &App) -> Option<Insertable> {
    app.world().resource::<AssetsState>().armed.clone()
}

fn activate(app: &mut App, e: Entity) {
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 2);
}

/// Not connected, or on a server without edits, the panel says why and
/// its tiles are inert; on one that takes edits, it says nothing.
#[test]
fn inserting_needs_a_live_world_that_takes_edits() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    open(&mut app);
    assert_eq!(note(&mut app).as_deref(), Some(NOT_CONNECTED));
    let boxed = Insertable::Shape(Shape::Box);
    let t = tile(&mut app, &boxed);
    assert!(app.world().entity(t).contains::<InteractionDisabled>());
    go_live(&mut app, false);
    assert_eq!(note(&mut app).as_deref(), Some(NO_EDITS));
    app.world_mut().resource_mut::<GzStatus>().edits = Some(true);
    frames(&mut app, 2);
    assert_eq!(note(&mut app), None);
    assert!(!app.world().entity(t).contains::<InteractionDisabled>());
    // Nor while a model is edited (§6.6), but a shape adds a link to it.
    edit_a_model(&mut app);
    app.world_mut().resource_mut::<AssetsState>().tab = AssetTab::Lights;
    frames(&mut app, 2);
    assert_eq!(note(&mut app).as_deref(), Some(EDITING));
    let light = tile(&mut app, &Insertable::Light(LightKind::Point));
    assert!(app.world().entity(light).contains::<InteractionDisabled>());
    assert!(!app.world().entity(t).contains::<InteractionDisabled>());
}

/// The tabs pick what shows.
#[test]
fn the_tabs_pick_the_content() {
    let mut app = live_assets();
    let shown = |app: &mut App| -> Vec<AssetTab> {
        let world = app.world_mut();
        world
            .query::<(&TabPane, &Node)>()
            .iter(world)
            .filter(|(_, n)| n.display != Display::None)
            .map(|(p, _)| p.0)
            .collect()
    };
    assert_eq!(shown(&mut app), [AssetTab::Shapes]);
    let fuel = {
        let world = app.world_mut();
        world
            .query::<(Entity, &TabChoice)>()
            .iter(world)
            .find(|(_, t)| t.0 == AssetTab::Fuel)
            .unwrap()
            .0
    };
    app.world_mut().trigger(ValueChange {
        source: fuel,
        value: fuel,
        is_final: true,
    });
    frames(&mut app, 2);
    assert_eq!(shown(&mut app), [AssetTab::Fuel]);
    assert!(all_text(&mut app).contains(SEARCH_PROMPT));
}

/// A tile arms its shape (the views place, the status bar says how, the
/// preview follows the pointer); a place sends a `create` there, lifted
/// to rest on it, and ends placing unless Shift kept it; the insert
/// shows until the scene holds it.
#[test]
fn a_tile_arms_and_a_place_inserts() {
    let mut app = live_assets();
    let boxed = Insertable::Shape(Shape::Box);
    let t = tile(&mut app, &boxed);
    activate(&mut app, t);
    assert_eq!(armed(&app), Some(boxed.clone()));
    assert!(app.world().resource::<Placing>().0);
    assert!(app
        .world()
        .entity(t)
        .contains::<crate::theme::visuals::Active>());
    let toggled = app
        .world()
        .get::<bevy::a11y::AccessibilityNode>(t)
        .unwrap()
        .toggled();
    assert_eq!(toggled, Some(accesskit::Toggled::True));
    assert!(all_text(&mut app).contains(PLACE_HINT));
    app.world_mut().resource_mut::<PlaceHover>().0 =
        Some((Vec3::new(1.0, 0.0, -2.0), Vec3::Y, None));
    frames(&mut app, 2);
    let world = app.world_mut();
    let (ghost, at, vis) = world
        .query::<(&Ghost, &Transform, &Visibility)>()
        .single(world)
        .unwrap();
    assert_eq!(ghost.0, boxed);
    assert_eq!(at.translation, Vec3::new(1.0, 0.5, -2.0));
    assert_eq!(*vis, Visibility::Inherited);

    app.world_mut().write_message(PlaceAt {
        point: [1.0, 2.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        keep: true,
        hit: None,
    });
    frames(&mut app, 2);
    assert_eq!(armed(&app), Some(boxed.clone()), "Shift keeps it");
    app.world_mut().write_message(PlaceAt {
        point: [3.0, 2.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        keep: false,
        hit: None,
    });
    frames(&mut app, 2);
    assert_eq!(armed(&app), None);
    assert!(!app.world().resource::<Placing>().0);
    let creates: Vec<msgs::EntityFactory> = sent(&mut app)
        .into_iter()
        .filter_map(|c| match c {
            GzCommand::Request {
                service, payload, ..
            } if service == "/world/warehouse_demo/create" => {
                Some(msgs::EntityFactory::decode(payload.as_slice()).unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(creates.len(), 2);
    let at = creates[1].pose.clone().unwrap().position.unwrap();
    assert_eq!((at.x, at.y, at.z), (3.0, 2.0, 0.5));
    assert_eq!(creates[1].name, "box");
    let world = app.world_mut();
    assert!(world.query::<&Ghost>().iter(world).next().is_none());
    assert!(all_text(&mut app).contains("Inserting…"));

    for _ in 0..2 {
        app.world_mut().write_message(GzServiceReply {
            service: "/world/warehouse_demo/create".into(),
            msg_type: "gz.msgs.Boolean".into(),
            payload: msgs::Boolean {
                data: true,
                ..Default::default()
            }
            .encode_to_vec(),
        });
    }
    frames(&mut app, 2);
    // One new box ends one of the two; the second waits for its own.
    let mut index = app.world_mut().resource_mut::<GzIndex>();
    index.by_name.insert("box".into(), vec![]);
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Inserts>().list.len(), 1);
    let mut index = app.world_mut().resource_mut::<GzIndex>();
    index.by_name.insert("box_0".into(), vec![]);
    frames(&mut app, 2);
    assert!(app.world().resource::<Inserts>().list.is_empty());
}

/// Esc stops placing (and only that); a tile clicked again disarms.
#[test]
fn esc_or_the_tile_again_disarms() {
    let mut app = live_assets();
    let sphere = Insertable::Shape(Shape::Sphere);
    let t = tile(&mut app, &sphere);
    activate(&mut app, t);
    app.world_mut().insert_resource(Tool::Translate);
    frames(&mut app, 1);
    key(&mut app, KeyCode::Escape, bevy::input::ButtonState::Pressed);
    frames(&mut app, 2);
    assert_eq!(armed(&app), None);
    assert_eq!(
        *app.world().resource::<Tool>(),
        Tool::Translate,
        "one step per Esc"
    );
    key(
        &mut app,
        KeyCode::Escape,
        bevy::input::ButtonState::Released,
    );
    activate(&mut app, t);
    assert!(armed(&app).is_some());
    activate(&mut app, t);
    assert_eq!(armed(&app), None);

    // An Esc that closes the command palette leaves placing armed.
    activate(&mut app, t);
    app.world_mut().write_message(UiCommand::OpenPalette);
    frames(&mut app, 3);
    let field = {
        let world = app.world_mut();
        let takers: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::widgets::focus::TakesEscape>>()
            .iter(world)
            .collect();
        let parents: Vec<(Entity, Entity)> = world
            .query_filtered::<(Entity, &ChildOf), With<crate::widgets::text_field::TextFieldInput>>(
            )
            .iter(world)
            .map(|(e, p)| (e, p.parent()))
            .collect();
        let under = |mut e: Entity| loop {
            if takers.contains(&e) {
                return true;
            }
            match world.get::<ChildOf>(e) {
                Some(p) => e = p.parent(),
                None => return false,
            }
        };
        parents.into_iter().find(|(_, p)| under(*p)).unwrap().0
    };
    focused_key(&mut app, field, KeyCode::Escape);
    frames(&mut app, 2);
    assert!(armed(&app).is_some(), "the palette took that Esc");
}

/// A refused insert shows in `danger_soft` until dismissed.
#[test]
fn a_refused_insert_can_be_dismissed() {
    let mut app = live_assets();
    let cone = Insertable::Shape(Shape::Cone);
    let t = tile(&mut app, &cone);
    activate(&mut app, t);
    app.world_mut().write_message(PlaceAt {
        point: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
        keep: false,
        hit: None,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(GzServiceReply {
        service: "/world/warehouse_demo/create".into(),
        msg_type: "gz.msgs.Boolean".into(),
        payload: msgs::Boolean::default().encode_to_vec(),
    });
    frames(&mut app, 3);
    assert!(all_text(&mut app).contains("The server couldn't insert Cone"));
    let dismiss = find::<Dismiss>(&mut app);
    activate(&mut app, dismiss);
    assert!(app.world().resource::<Inserts>().list.is_empty());
    let rows = find::<InsertRows>(&mut app);
    assert!(app
        .world()
        .get::<Children>(rows)
        .is_none_or(|c| c.is_empty()));
}

fn models(n: usize) -> Vec<FuelModel> {
    (0..n)
        .map(|i| FuelModel {
            name: format!("Pallet {i}"),
            owner: "OpenRobotics".into(),
            filesize: 2_100_000,
            thumbnail_url: None,
            ..Default::default()
        })
        .collect()
}

fn button(app: &mut App, which: FuelButton) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &FuelButton)>()
        .iter(world)
        .find(|(_, b)| **b == which)
        .unwrap()
        .0
}

/// Typing searches a moment later; results show as rows whose Insert
/// arms their model (a bar under the row while it's on its way); an
/// unreachable Fuel says so with Retry; Load more asks for the next page.
#[test]
fn fuel_searches_and_inserts() {
    let mut app = live_assets();
    app.world_mut().resource_mut::<AssetsState>().tab = AssetTab::Fuel;
    let reader = |app: &mut App| -> Vec<FuelSearch> {
        app.world_mut()
            .resource_mut::<Messages<FuelSearch>>()
            .drain()
            .collect()
    };
    {
        let mut s = app.world_mut().resource_mut::<AssetsState>();
        s.query = "pallet".into();
        s.typed_at = Some(0.0);
    }
    // Messages last two frames: read after each.
    let mut searches = vec![];
    for _ in 0..12 {
        app.update();
        searches.extend(reader(&mut app));
    }
    assert!(
        searches.contains(&FuelSearch {
            query: "pallet".into(),
            page: 1
        }),
        "{searches:?}"
    );
    *app.world_mut().resource_mut::<GzFuel>() = GzFuel {
        query: "pallet".into(),
        page: 1,
        models: models(3),
        more: true,
        state: FuelState::Idle,
    };
    frames(&mut app, 2);
    let text = all_text(&mut app);
    assert!(
        text.contains("Pallet 2") && text.contains("OpenRobotics · 2.1 MB"),
        "{text}"
    );
    let insert = {
        let world = app.world_mut();
        world
            .query::<(Entity, &FuelInsert)>()
            .iter(world)
            .find(|(_, f)| f.0.name == "Pallet 1")
            .unwrap()
            .0
    };
    let label = app
        .world()
        .get::<bevy::a11y::AccessibilityNode>(insert)
        .unwrap()
        .label()
        .map(str::to_string);
    assert_eq!(label.as_deref(), Some("Insert Pallet 1"));
    activate(&mut app, insert);
    let uri = "https://fuel.gazebosim.org/1.0/OpenRobotics/models/Pallet%201";
    assert_eq!(
        armed(&app),
        Some(Insertable::Model {
            uri: uri.into(),
            name: "pallet_1".into()
        })
    );
    app.world_mut().write_message(PlaceAt {
        point: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
        keep: false,
        hit: None,
    });
    frames(&mut app, 2);
    let world = app.world_mut();
    let bar = world
        .query::<(&RowBar, &Node)>()
        .iter(world)
        .find(|(b, _)| b.0 == uri)
        .map(|(_, n)| n.display);
    assert_eq!(bar, Some(Display::Flex));

    let more = button(&mut app, FuelButton::More);
    reader(&mut app);
    app.world_mut().trigger(Activate { entity: more });
    let searches = reader(&mut app);
    assert_eq!(
        searches,
        [FuelSearch {
            query: "pallet".into(),
            page: 2
        }]
    );
    frames(&mut app, 4);
    app.world_mut().resource_mut::<GzFuel>().state = FuelState::Failed {
        unreachable: true,
        message: "refused".into(),
    };
    frames(&mut app, 2);
    let world = app.world_mut();
    let msg = world
        .query_filtered::<&Text, With<FuelMessage>>()
        .single(world)
        .unwrap()
        .0
        .clone();
    assert_eq!(msg, UNREACHABLE);
    let retry = button(&mut app, FuelButton::Retry);
    app.world_mut().trigger(Activate { entity: retry });
    // Asks again for the page that failed (the second).
    assert_eq!(
        reader(&mut app),
        [FuelSearch {
            query: "pallet".into(),
            page: 2
        }]
    );
}

/// The Local tab's Insert arms its URI's model.
#[test]
fn a_local_uri_arms_its_model() {
    let mut app = live_assets();
    app.world_mut().resource_mut::<AssetsState>().uri = "model://shelf".into();
    frames(&mut app, 1);
    let insert = find::<LocalInsert>(&mut app);
    activate(&mut app, insert);
    assert_eq!(
        armed(&app),
        Some(Insertable::Model {
            uri: "model://shelf".into(),
            name: "shelf".into()
        })
    );
}

/// A start opens the panel on its tab and searches.
#[test]
fn a_start_opens_on_its_tab() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource("fuel:box".parse::<AssetsStart>().unwrap());
    frames(&mut app, 3);
    assert_eq!(app.world().resource::<ShellLayout>().primary, Some(ASSETS));
    let state = app.world().resource::<AssetsState>();
    assert_eq!((state.tab, state.query.as_str()), (AssetTab::Fuel, "box"));
    assert!(!app.world().contains_resource::<AssetsStart>());
}

// The Parts tab (§6.6).

/// The panel open, editing the model `edited` with links `a` (at the
/// origin) and `b` (2 m along X); (app, model, a, b).
fn editing_parts() -> (App, Entity, Entity, Entity) {
    use gz_bevy::overlays::extras::SdfExtras;
    let mut app = live_assets();
    // The server's SDFormat has come (with nothing it adds).
    app.init_resource::<SdfExtras>();
    {
        let mut x = app.world_mut().resource_mut::<SdfExtras>();
        x.extras = Some(gz_core::sdf::WorldExtras::default());
        x.generation += 1;
    }
    let model = edit_a_model(&mut app);
    let link = |app: &mut App, name: &str, x: f32| {
        app.world_mut()
            .spawn((
                gz_bevy::spawn::GzEntity {
                    name: name.into(),
                    id: 0,
                    kind: gz_core::scene::NodeKind::Link,
                    path: format!("edited/{name}"),
                },
                Transform::from_xyz(x, 0.0, 0.0),
                Visibility::default(),
                ChildOf(model),
            ))
            .id()
    };
    let (a, b) = (link(&mut app, "a", 0.0), link(&mut app, "b", 2.0));
    frames(&mut app, 2);
    (app, model, a, b)
}

fn part_tile(app: &mut App, tool: PartTool) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &super::parts::PartTile)>()
        .iter(world)
        .find(|(_, t)| t.0 == tool)
        .unwrap()
        .0
}

fn place(app: &mut App, point: [f64; 3], hit: Option<Entity>, keep: bool) {
    app.world_mut().write_message(PlaceAt {
        point,
        normal: [0.0, 0.0, 1.0],
        keep,
        hit,
    });
    frames(app, 3);
}

/// The entity `path` names, and its parent.
fn at_path(app: &mut App, path: &str) -> Option<(Entity, Entity)> {
    let world = app.world_mut();
    world
        .query::<(Entity, &gz_bevy::spawn::GzEntity, &ChildOf)>()
        .iter(world)
        .find(|(_, g, _)| g.path == path)
        .map(|(e, _, p)| (e, p.parent()))
}

fn status_note(app: &App) -> Option<String> {
    app.world()
        .resource::<crate::shell::status_bar::readouts::StatusNote>()
        .0
        .as_ref()
        .map(|(t, _)| t.clone())
}

fn parts_state(app: &App) -> PartsState {
    app.world().resource::<AssetsState>().parts.clone()
}

/// The Parts tab shows while a model is edited, and entering opens it;
/// leaving goes back to Shapes.
#[test]
fn the_parts_tab_is_for_editing() {
    let mut app = live_assets();
    let parts_tab = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&TabChoice, &Node)>()
            .iter(world)
            .find(|(t, _)| t.0 == AssetTab::Parts)
            .map(|(_, n)| n.display != Display::None)
            .unwrap()
    };
    assert!(!parts_tab(&mut app));
    edit_a_model(&mut app);
    assert!(parts_tab(&mut app));
    assert_eq!(app.world().resource::<AssetsState>().tab, AssetTab::Parts);
    assert_eq!(note(&mut app), None, "the tab has its own hint");
    app.world_mut()
        .write_message(gz_bevy::view::editing::EditCommand::Done);
    frames(&mut app, 3);
    assert!(!parts_tab(&mut app));
    assert_eq!(app.world().resource::<AssetsState>().tab, AssetTab::Shapes);
}

/// Link arms placing; a click adds a link there, resting on the surface,
/// and selects it; Shift keeps it armed. A shape from the Shapes tab adds
/// a link of its shape.
#[test]
fn a_click_adds_a_link() {
    let (mut app, model, ..) = editing_parts();
    let t = part_tile(&mut app, PartTool::Link);
    activate(&mut app, t);
    assert!(app.world().resource::<Placing>().0);
    assert!(app
        .world()
        .entity(t)
        .contains::<crate::theme::visuals::Active>());
    place(&mut app, [1.0, 2.0, 0.0], None, true);
    let (link, parent) = at_path(&mut app, "edited/link_1").expect("a link");
    assert_eq!(parent, model);
    let at = app.world().get::<Transform>(link).unwrap().translation;
    assert!(at.distance(Vec3::new(1.0, 2.0, 0.25)) < 1e-4, "{at}");
    assert_eq!(
        app.world().resource::<gz_bevy::view::Selection>().entities,
        [link]
    );
    assert_eq!(parts_state(&app).armed, Some(PartTool::Link), "Shift");
    place(&mut app, [3.0, 0.0, 0.0], None, false);
    assert!(at_path(&mut app, "edited/link_2").is_some());
    assert_eq!(parts_state(&app).armed, None);
    // A sphere from the Shapes tab.
    app.world_mut().resource_mut::<AssetsState>().tab = AssetTab::Shapes;
    frames(&mut app, 2);
    let sphere = tile(&mut app, &Insertable::Shape(Shape::Sphere));
    activate(&mut app, sphere);
    place(&mut app, [0.0, 3.0, 0.0], None, false);
    let (l3, _) = at_path(&mut app, "edited/link_3").expect("a sphere link");
    let visual = app.world().get::<Children>(l3).unwrap()[0];
    let shape = &app
        .world()
        .get::<gz_bevy::spawn::GzVisual>(visual)
        .unwrap()
        .0
        .geometry;
    assert!(matches!(shape, gz_core::scene::GeometryDesc::Sphere { .. }));
    let creates = sent(&mut app)
        .into_iter()
        .filter(|c| matches!(c, GzCommand::Request { service, .. } if service.ends_with("/create")))
        .count();
    assert_eq!(creates, 0, "nothing goes into the world");
}

/// Visual goes on the link clicked; a click off the model's links says
/// so.
#[test]
fn a_visual_goes_on_the_link_clicked() {
    let (mut app, _, a, _) = editing_parts();
    let t = part_tile(&mut app, PartTool::Visual);
    activate(&mut app, t);
    place(&mut app, [0.0, 0.0, 0.0], None, true);
    assert_eq!(status_note(&app).as_deref(), Some(super::parts::NOT_A_LINK));
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    let (_, parent) = at_path(&mut app, "edited/a/visual_1").expect("a visual");
    assert_eq!(parent, a);
}

/// A joint takes the parent link, then the child; one that would give
/// the child a second parent says why. Collisions and sensors go on the
/// link; Plugin names one.
#[test]
fn joints_collisions_sensors_and_plugins() {
    use gz_bevy::overlays::extras::SdfExtras;
    let (mut app, _, a, b) = editing_parts();
    let t = part_tile(&mut app, PartTool::Joint);
    activate(&mut app, t);
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    assert_eq!(parts_state(&app).parent, Some(a));
    assert!(all_text(&mut app).contains(JOINT_CHILD_HINT));
    place(&mut app, [2.0, 0.0, 0.0], Some(b), false);
    let extras = |app: &App| {
        app.world()
            .resource::<SdfExtras>()
            .extras
            .clone()
            .unwrap_or_default()
    };
    let j = extras(&app).link("edited/b").unwrap().joints[0].clone();
    assert_eq!((j.parent.as_str(), j.child.as_str()), ("a", "b"));
    assert_eq!(j.name, "joint_1");
    assert_eq!(parts_state(&app).armed, None);
    // b has its parent now.
    activate(&mut app, t);
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    place(&mut app, [2.0, 0.0, 0.0], Some(b), false);
    assert_eq!(
        status_note(&app).as_deref(),
        Some("b has a parent joint already")
    );
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: bevy::input::keyboard::Key::Escape,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    frames(&mut app, 2);
    assert_eq!(parts_state(&app), PartsState::default(), "Esc");
    // A collision and an IMU on a.
    let c = part_tile(&mut app, PartTool::Collision);
    activate(&mut app, c);
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    app.world_mut().resource_mut::<AssetsState>().parts.sensor = gz_core::parts::SensorKind::Imu;
    let s = part_tile(&mut app, PartTool::Sensor);
    activate(&mut app, s);
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    let link = extras(&app).link("edited/a").cloned().unwrap();
    assert_eq!(link.collisions[0].name, "collision_1");
    assert_eq!(link.sensors[0].name, "imu_1");
    let sensors = app.world().get::<gz_bevy::spawn::GzSensors>(a).unwrap();
    assert_eq!(sensors.0[0].ty, "imu");
    // A plugin.
    let p = part_tile(&mut app, PartTool::Plugin);
    activate(&mut app, p);
    assert!(
        !app.world().resource::<Placing>().0,
        "a plugin isn't placed"
    );
    {
        let mut st = app.world_mut().resource_mut::<AssetsState>();
        st.parts.plugin_file = "gz-sim-joint-state-publisher-system".into();
        st.parts.plugin_name = "gz::sim::systems::JointStatePublisher".into();
    }
    let add = find::<super::parts::PluginAdd>(&mut app);
    activate(&mut app, add);
    frames(&mut app, 2);
    assert_eq!(
        extras(&app).model("edited").unwrap().plugins,
        ["gz::sim::systems::JointStatePublisher"]
    );
}

/// Undo takes the last part back, and the World menu names it; the
/// pill's Discard, shown while there are parts, leaves without them and
/// says so.
#[test]
fn undo_takes_a_part_back_and_discard_drops_them() {
    use crate::shell::viewport::editing::EditingDiscard;
    let discard_shown = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Node, With<EditingDiscard>>()
            .iter(world)
            .any(|n| n.display != Display::None)
    };
    let (mut app, ..) = editing_parts();
    assert!(!discard_shown(&mut app), "no parts yet");
    let t = part_tile(&mut app, PartTool::Link);
    activate(&mut app, t);
    place(&mut app, [1.0, 0.0, 0.0], None, true);
    place(&mut app, [2.0, 0.0, 0.0], None, false);
    assert!(all_text(&mut app).contains("Undo add link_2"));
    app.world_mut()
        .write_message(gz_bevy::undo::UndoCommand::Undo);
    frames(&mut app, 3);
    assert!(at_path(&mut app, "edited/link_2").is_none());
    assert!(at_path(&mut app, "edited/link_1").is_some());
    assert!(discard_shown(&mut app));
    let discard = find::<EditingDiscard>(&mut app);
    activate(&mut app, discard);
    frames(&mut app, 3);
    assert!(at_path(&mut app, "edited/link_1").is_none());
    assert_eq!(
        app.world()
            .resource::<gz_bevy::view::editing::EditingModel>()
            .model(),
        None
    );
    assert_eq!(status_note(&app).as_deref(), Some("1 part discarded"));
}

/// Esc, pressed and released.
fn press_esc(app: &mut App) {
    use bevy::input::ButtonState;
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: bevy::input::keyboard::Key::Escape,
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        frames(app, 2);
    }
}

/// The server's SDFormat for the edited model, as `inner` (in its
/// `<model>`) says.
fn server_sdf(app: &mut App, inner: &str) {
    use gz_bevy::overlays::extras::SdfExtras;
    let sdf = format!(
        "<sdf version=\"1.9\"><world name=\"w\"><model name=\"edited\">{inner}</model>\
         </world></sdf>"
    );
    let mut x = app.world_mut().resource_mut::<SdfExtras>();
    x.extras = Some(gz_core::sdf::WorldExtras::parse(&sdf).unwrap());
    x.generation += 1;
    frames(app, 2);
}

/// A joint's parent is forgotten once undo takes it back, or another
/// model is edited: the next click is the parent again.
#[test]
fn a_joints_parent_goes_with_its_link_or_model() {
    let (mut app, model, a, _) = editing_parts();
    let link = part_tile(&mut app, PartTool::Link);
    activate(&mut app, link);
    place(&mut app, [4.0, 0.0, 0.0], None, false);
    let (l1, _) = at_path(&mut app, "edited/link_1").unwrap();
    let joint = part_tile(&mut app, PartTool::Joint);
    activate(&mut app, joint);
    place(&mut app, [4.0, 0.0, 0.0], Some(l1), false);
    assert_eq!(parts_state(&app).parent, Some(l1));
    app.world_mut()
        .write_message(gz_bevy::undo::UndoCommand::Undo);
    frames(&mut app, 3);
    assert_eq!(parts_state(&app).parent, None);
    assert!(!all_text(&mut app).contains(JOINT_CHILD_HINT));
    place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
    assert_eq!(parts_state(&app).parent, Some(a), "the parent again");
    // Another model, edited: a isn't its link.
    let root = app.world().get::<ChildOf>(model).unwrap().parent();
    let other = app
        .world_mut()
        .spawn((
            gz_bevy::spawn::GzEntity {
                name: "other".into(),
                id: 0,
                kind: gz_core::scene::NodeKind::Model,
                path: "other".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    app.world_mut()
        .write_message(gz_bevy::view::editing::EditCommand::Enter(other));
    frames(&mut app, 3);
    assert_eq!(parts_state(&app).parent, None);
}

/// A link fixed to the world, or a nested model's link with its parent,
/// can't take a second parent; before the server's SDFormat comes, a
/// joint or a collision waits for it.
#[test]
fn joints_count_the_world_and_nested_models() {
    use gz_bevy::overlays::extras::SdfExtras;
    let (mut app, model, a, b) = editing_parts();
    let inner = app
        .world_mut()
        .spawn((
            gz_bevy::spawn::GzEntity {
                name: "inner".into(),
                id: 0,
                kind: gz_core::scene::NodeKind::Model,
                path: "edited/inner".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(model),
        ))
        .id();
    let d = app
        .world_mut()
        .spawn((
            gz_bevy::spawn::GzEntity {
                name: "d".into(),
                id: 0,
                kind: gz_core::scene::NodeKind::Link,
                path: "edited/inner/d".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(inner),
        ))
        .id();
    server_sdf(
        &mut app,
        "<link name=\"a\"/><link name=\"b\"/><joint name=\"fix\" type=\"fixed\">\
         <parent>world</parent><child>b</child></joint><model name=\"inner\">\
         <link name=\"c\"/><link name=\"d\"/><joint name=\"j\" type=\"fixed\">\
         <parent>c</parent><child>d</child></joint></model>",
    );
    let joint = part_tile(&mut app, PartTool::Joint);
    for child in [b, d] {
        activate(&mut app, joint);
        place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
        place(&mut app, [0.0, 0.0, 0.0], Some(child), false);
        let name = app
            .world()
            .get::<gz_bevy::spawn::GzEntity>(child)
            .unwrap()
            .name
            .clone();
        assert_eq!(
            status_note(&app),
            Some(format!("{name} has a parent joint already"))
        );
        press_esc(&mut app);
    }
    // No SDFormat yet.
    app.world_mut().resource_mut::<SdfExtras>().extras = None;
    frames(&mut app, 2);
    for tool in [PartTool::Joint, PartTool::Collision] {
        let t = part_tile(&mut app, tool);
        activate(&mut app, t);
        place(&mut app, [0.0, 0.0, 0.0], Some(a), false);
        assert_eq!(status_note(&app).as_deref(), Some(super::parts::NO_SDF));
        assert_eq!(parts_state(&app).parent, None, "{tool:?}");
        press_esc(&mut app);
    }
}

/// Esc with Plugin armed disarms it and leaves editing alone; an armed
/// tile is pressed to AccessKit.
#[test]
fn esc_disarms_a_plugin_and_armed_tiles_are_pressed() {
    let (mut app, model, ..) = editing_parts();
    let p = part_tile(&mut app, PartTool::Plugin);
    activate(&mut app, p);
    frames(&mut app, 2);
    let toggled = |app: &App, e: Entity| {
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(e)
            .unwrap()
            .toggled()
    };
    assert_eq!(toggled(&app, p), Some(accesskit::Toggled::True));
    press_esc(&mut app);
    assert_eq!(parts_state(&app).armed, None);
    assert_eq!(toggled(&app, p), None);
    let editing = app
        .world()
        .resource::<gz_bevy::view::editing::EditingModel>()
        .model();
    assert_eq!(editing, Some(model), "still editing");
}

/// The preview of a link stands upright in a turned model.
#[test]
fn the_preview_stands_in_the_models_frame() {
    let (mut app, model, ..) = editing_parts();
    app.world_mut()
        .get_mut::<Transform>(model)
        .unwrap()
        .rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let t = part_tile(&mut app, PartTool::Link);
    activate(&mut app, t);
    app.world_mut().resource_mut::<PlaceHover>().0 = Some((Vec3::ZERO, Vec3::Y, None));
    frames(&mut app, 3);
    let world = app.world_mut();
    let ghost = world
        .query_filtered::<&Transform, With<super::parts::PartGhost>>()
        .single(world)
        .unwrap()
        .rotation;
    // Gazebo's yaw is a turn about Bevy's Y: its X goes to Bevy's -Z.
    assert!((ghost * Vec3::X).distance(Vec3::NEG_Z) < 1e-5, "{ghost:?}");
    assert!((ghost * Vec3::Y).distance(Vec3::Y) < 1e-5, "{ghost:?}");
}
