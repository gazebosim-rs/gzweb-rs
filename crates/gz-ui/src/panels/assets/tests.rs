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
    assert!("parts".parse::<AssetsStart>().is_err());
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
    // Nor while a model is edited (§6.6).
    edit_a_model(&mut app);
    assert_eq!(note(&mut app).as_deref(), Some(EDITING));
    assert!(app.world().entity(t).contains::<InteractionDisabled>());
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
    app.world_mut().resource_mut::<PlaceHover>().0 = Some((Vec3::new(1.0, 0.0, -2.0), Vec3::Y));
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
    });
    frames(&mut app, 2);
    assert_eq!(armed(&app), Some(boxed.clone()), "Shift keeps it");
    app.world_mut().write_message(PlaceAt {
        point: [3.0, 2.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        keep: false,
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
