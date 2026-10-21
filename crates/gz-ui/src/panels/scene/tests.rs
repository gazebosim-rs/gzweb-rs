//! The Scene panel through the shell: its body follows the connection,
//! its header counts the entities and has its actions; the tree shows the
//! scene, follows spawns and deletions, filters, keeps its state across a
//! rebuilt body, recycles a bounded pool of rows over thousands of
//! entities, owns visibility, and takes the arrow keys.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{Activate, ValueChange};
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_bevy::overlays::extras::{SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzIndex, GzSensors};
use gz_core::scene::{NodeKind, Pose, SensorDesc};
use gz_core::sdf::WorldExtras;
use gz_core::transport::Status;

use super::model::{Chip, NodeKey, TreeScroll, TreeState};
use super::view::{ChipChoice, ChipRow, FilterButton, FilterField, NoMatches, TreeList, TreeSlot};
use super::{COLLAPSE_ALL, EXPAND_ALL, NEW_WORLD, OPEN_WORLD, SCENE};
use crate::panels::{ActionId, PanelBody, PanelView, PLOT, RECONNECT};
use crate::shell::sidebar::body::{ActionButton, BodyView, SkeletonRow};
use crate::shell::{Slot, UiCommand};
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Ring};
use crate::widgets::TreeRowSpec;

fn view(app: &mut App) -> PanelView {
    let world = app.world_mut();
    let (body, view) = world
        .query::<(&PanelBody, &BodyView)>()
        .single(world)
        .unwrap();
    assert_eq!(body.0, SCENE);
    view.0.clone()
}

/// The Scene panel's buttons: (action, enabled), by name.
fn actions(app: &mut App) -> Vec<(ActionId, bool)> {
    let world = app.world_mut();
    let mut out: Vec<(ActionId, bool)> = world
        .query::<(&ActionButton, Has<InteractionDisabled>)>()
        .iter(world)
        .filter(|(b, _)| b.panel == SCENE)
        .map(|(b, d)| (b.action, !d))
        .collect();
    out.sort();
    out
}

fn header_button(app: &mut App, id: ActionId) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ActionButton)>()
        .iter(world)
        .find(|(_, b)| b.panel == SCENE && b.action == id)
        .unwrap()
        .0
}

#[test]
fn the_scene_panel_follows_the_connection() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    // No world: the world actions, disabled until the World menu (M8);
    // Expand all and Collapse all, disabled without a tree.
    assert!(all_text(&mut app).contains("No world loaded"));
    assert_eq!(
        actions(&mut app),
        [
            (COLLAPSE_ALL, false),
            (EXPAND_ALL, false),
            (NEW_WORLD, false),
            (OPEN_WORLD, false)
        ]
    );
    // Skeleton rows while the first snapshot arrives.
    app.world_mut().resource_mut::<GzStatus>().status = Status::Connected;
    frames(&mut app, 2);
    assert_eq!(view(&mut app), PanelView::Loading);
    let world = app.world_mut();
    assert_eq!(world.query::<&SkeletonRow>().iter(world).count(), 3);
    // Live: its content and the entity count.
    go_live(&mut app, false);
    assert_eq!(view(&mut app), PanelView::Ready);
    assert!(all_text(&mut app).contains("0 entities"));
    // Lost: the error, whose Reconnect retries.
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Error("closed".into());
        s.reconnect_at = Some(1e9);
    }
    frames(&mut app, 2);
    assert!(matches!(view(&mut app), PanelView::Error { .. }));
    let text = all_text(&mut app);
    assert!(text.contains("Lost connection to server") && !text.contains("0 entities"));
    let reconnect = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActionButton)>()
            .iter(world)
            .find(|(_, b)| b.action == RECONNECT)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: reconnect });
    frames(&mut app, 1);
    assert_eq!(
        app.world().resource::<Sent>().1,
        [GzReconnect { url: None }]
    );
}

/// The scene as gz-bevy spawns it: a root, models with links (a visual,
/// a light, sensors), scene lights.
struct World3 {
    root: Entity,
}

impl World3 {
    fn spawn(app: &mut App) -> Self {
        let world = app.world_mut();
        let root = world
            .spawn((Transform::default(), Visibility::default()))
            .id();
        {
            let mut index = world.resource_mut::<GzIndex>();
            index.root = Some(root);
            index.scene = Some(gz_core::scene::SceneDesc::from_msg(
                &Default::default(),
                Default::default(),
            ));
        }
        let me = World3 { root };
        let amr = me.add(app, root, "amr_01", "amr_01", NodeKind::Model);
        let base = me.add(app, amr, "base_link", "amr_01/base_link", NodeKind::Link);
        me.add(
            app,
            base,
            "visual",
            "amr_01/base_link/visual",
            NodeKind::Visual,
        );
        app.world_mut()
            .entity_mut(base)
            .insert(GzSensors(vec![sensor("gpu_lidar"), sensor("front_camera")]));
        me.add(app, amr, "wheel_left", "amr_01/wheel_left", NodeKind::Link);
        me.add(app, root, "crate_01", "crate_01", NodeKind::Model);
        me.add(app, root, "sun", "sun", NodeKind::Light);
        me
    }

    fn add(&self, app: &mut App, parent: Entity, name: &str, path: &str, kind: NodeKind) -> Entity {
        let world = app.world_mut();
        let e = world
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
            .id();
        world.resource_mut::<GzIndex>().order.push(e);
        e
    }
}

fn sensor(name: &str) -> SensorDesc {
    SensorDesc {
        name: name.into(),
        id: 0,
        ty: "camera".into(),
        pose: Pose::IDENTITY,
    }
}

/// The full UI, live, with [`World3`] in it and the Scene panel open.
fn live_tree() -> (App, World3) {
    let mut app = shell_app((1440, 900), 1.0);
    app.init_resource::<SdfExtras>()
        .init_resource::<SdfWanted>();
    frames(&mut app, 2);
    let w = World3::spawn(&mut app);
    go_live(&mut app, false);
    frames(&mut app, 2);
    (app, w)
}

/// The rows shown, top to bottom: indent, caret and name.
fn rows(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut shown: Vec<(usize, TreeRowSpec)> = world
        .query::<(&TreeSlot, &TreeRowSpec, &Node)>()
        .iter(world)
        .filter(|(_, _, n)| n.display != Display::None)
        .filter_map(|(s, spec, _)| s.0.map(|i| (i, spec.clone())))
        .collect();
    shown.sort_by_key(|s| s.0);
    shown
        .into_iter()
        .map(|(_, s)| {
            let caret = match s.expanded {
                Some(true) => "v ",
                Some(false) => "> ",
                None => "",
            };
            let hidden = if s.hidden { " (hidden)" } else { "" };
            format!(
                "{}{caret}{} {}{hidden}",
                "  ".repeat(s.depth as usize),
                s.kind.letter(),
                s.name
            )
        })
        .collect()
}

/// The pooled row showing `name`.
fn row(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &TreeSlot, &TreeRowSpec)>()
        .iter(world)
        .find(|(_, s, spec)| s.0.is_some() && spec.name == name)
        .unwrap_or_else(|| panic!("no row {name}"))
        .0
}

fn part(app: &mut App, row: Entity, which: crate::widgets::tree_row::TreeRowPart) -> Entity {
    let world = app.world_mut();
    descendants(world, row)
        .into_iter()
        .find(|e| world.get::<crate::widgets::tree_row::TreeRowPart>(*e) == Some(&which))
        .unwrap()
}

fn caret(app: &mut App, name: &str) {
    let r = row(app, name);
    let c = part(app, r, crate::widgets::tree_row::TreeRowPart::Caret);
    app.world_mut().trigger(Activate { entity: c });
    frames(app, 2);
}

fn set_filter(app: &mut App, text: &str) {
    let field = find::<FilterField>(app);
    let input = descendants(app.world(), field)
        .into_iter()
        .find(|e| app.world().get::<EditableText>(*e).is_some())
        .unwrap();
    app.world_mut()
        .entity_mut(input)
        .insert(EditableText::new(text));
    frames(app, 2);
}

fn choose(app: &mut App, chip: Chip) {
    let group = find::<ChipRow>(app);
    let world = app.world_mut();
    let e = world
        .query::<(Entity, &ChipChoice)>()
        .iter(world)
        .find(|(_, c)| c.0 == chip)
        .unwrap()
        .0;
    world.trigger(ValueChange {
        source: group,
        value: e,
        is_final: true,
    });
    frames(app, 2);
}

#[test]
fn the_tree_shows_the_scene_and_counts_it() {
    let (mut app, _) = live_tree();
    // The world open, models closed; visuals aren't listed.
    assert_eq!(
        rows(&mut app),
        [
            "v W warehouse_demo",
            "  > M amr_01",
            "  M crate_01",
            "  L sun"
        ]
    );
    assert!(all_text(&mut app).contains("7 entities"));
    // Expand all and Collapse all work now.
    assert!(actions(&mut app)
        .iter()
        .filter(|a| a.0 == EXPAND_ALL || a.0 == COLLAPSE_ALL)
        .all(|a| a.1));
    caret(&mut app, "amr_01");
    assert_eq!(
        rows(&mut app),
        [
            "v W warehouse_demo",
            "  v M amr_01",
            "    > K base_link",
            "    K wheel_left",
            "  M crate_01",
            "  L sun"
        ]
    );
    // Joints come with the world's SDFormat, under their model.
    let sdf = r#"<sdf version="1.10"><world name="w"><model name="amr_01">
        <link name="base_link"/><link name="wheel_left"/>
        <joint name="wheel_left_joint" type="revolute"><parent>base_link</parent>
          <child>wheel_left</child><axis><xyz>0 1 0</xyz></axis></joint>
        </model></world></sdf>"#;
    app.world_mut().resource_mut::<SdfExtras>().extras = Some(WorldExtras::parse(sdf).unwrap());
    frames(&mut app, 2);
    let header = header_button(&mut app, EXPAND_ALL);
    app.world_mut().trigger(Activate { entity: header });
    frames(&mut app, 2);
    assert_eq!(
        rows(&mut app),
        [
            "v W warehouse_demo",
            "  v M amr_01",
            "    v K base_link",
            "      S gpu_lidar",
            "      S front_camera",
            "    K wheel_left",
            "    J wheel_left_joint",
            "  M crate_01",
            "  L sun"
        ]
    );
    assert!(all_text(&mut app).contains("8 entities"));
    let header = header_button(&mut app, COLLAPSE_ALL);
    app.world_mut().trigger(Activate { entity: header });
    frames(&mut app, 2);
    assert_eq!(rows(&mut app).len(), 4);
    // The tree asks for the SDFormat while it shows.
    assert!(app.world().resource::<SdfWanted>().0);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    assert!(!app.world().resource::<SdfWanted>().0);
}

/// Spawns and deletions show at once; so does a hidden entity.
#[test]
fn the_tree_follows_spawns_deletions_and_visibility() {
    let (mut app, w) = live_tree();
    let ball = w.add(&mut app, w.root, "ball", "ball", NodeKind::Model);
    frames(&mut app, 2);
    assert_eq!(rows(&mut app)[4], "  M ball");
    assert!(all_text(&mut app).contains("8 entities"));
    app.world_mut().entity_mut(ball).despawn();
    frames(&mut app, 2);
    assert_eq!(rows(&mut app).len(), 4);
    assert!(all_text(&mut app).contains("7 entities"));
    // The eye asks; the panel hides the entity; the row follows.
    let crate_row = row(&mut app, "crate_01");
    let eye = part(
        &mut app,
        crate_row,
        crate::widgets::tree_row::TreeRowPart::Eye,
    );
    app.world_mut().trigger(Activate { entity: eye });
    frames(&mut app, 2);
    let world = app.world_mut();
    let crate_e = world
        .query::<(Entity, &GzEntity)>()
        .iter(world)
        .find(|(_, g)| g.name == "crate_01")
        .unwrap()
        .0;
    assert_eq!(world.get::<Visibility>(crate_e), Some(&Visibility::Hidden));
    assert_eq!(rows(&mut app)[2], "  M crate_01 (hidden)");
    // Hidden elsewhere (M5's context menu, say): the row follows too.
    app.world_mut()
        .entity_mut(crate_e)
        .insert(Visibility::Inherited);
    frames(&mut app, 2);
    assert_eq!(rows(&mut app)[2], "  M crate_01");
    // The world row has no eye.
    let world_row = row(&mut app, "warehouse_demo");
    assert!(!app.world().get::<TreeRowSpec>(world_row).unwrap().hideable);
}

#[test]
fn the_filter_and_the_chips_narrow_the_tree() {
    let (mut app, _) = live_tree();
    set_filter(&mut app, "LIDAR");
    assert_eq!(
        rows(&mut app),
        [
            "v W warehouse_demo",
            "  v M amr_01",
            "    v K base_link",
            "      S gpu_lidar"
        ]
    );
    // A path closed while filtering stays closed until the filter changes.
    caret(&mut app, "amr_01");
    assert_eq!(rows(&mut app).len(), 2);
    set_filter(&mut app, "");
    assert_eq!(rows(&mut app).len(), 4);
    // The chips: a radio set, shown by the filter button, which looks
    // active while a chip filters.
    let button = find::<FilterButton>(&mut app);
    assert!(app.world().get::<Active>(button).is_none());
    choose(&mut app, Chip::Lights);
    assert_eq!(rows(&mut app), ["v W warehouse_demo", "  L sun"]);
    assert!(app.world().get::<Active>(button).is_some());
    let world = app.world_mut();
    let checked: Vec<Chip> = world
        .query::<(&ChipChoice, Has<Checked>)>()
        .iter(world)
        .filter(|c| c.1)
        .map(|c| c.0 .0)
        .collect();
    assert_eq!(checked, [Chip::Lights]);
    set_filter(&mut app, "nothing");
    assert!(rows(&mut app).is_empty());
    let none = find::<NoMatches>(&mut app);
    assert!(displayed(&app, none));
    choose(&mut app, Chip::All);
    set_filter(&mut app, "");
    assert!(!displayed(&app, none));
    let chips = find::<ChipRow>(&mut app);
    assert!(displayed(&app, chips));
    app.world_mut().trigger(Activate { entity: button });
    frames(&mut app, 2);
    assert!(!displayed(&app, chips));
}

/// The body is a disposable view: its expansion, filter, chips and scroll
/// live in the panel's resources and come back with a rebuilt body.
#[test]
fn a_rebuilt_body_keeps_the_trees_state() {
    let (mut app, _) = live_tree();
    caret(&mut app, "amr_01");
    set_filter(&mut app, "a");
    choose(&mut app, Chip::Models);
    let before = rows(&mut app);
    let body = find::<super::SceneTree>(&mut app);
    // Lost and back: a new body.
    {
        let mut s = app.world_mut().resource_mut::<GzStatus>();
        s.status = Status::Error("closed".into());
    }
    frames(&mut app, 2);
    go_live(&mut app, false);
    frames(&mut app, 2);
    assert_ne!(find::<super::SceneTree>(&mut app), body);
    assert_eq!(rows(&mut app), before);
    let field = find::<FilterField>(&mut app);
    let text = descendants(app.world(), field)
        .into_iter()
        .find_map(|e| app.world().get::<EditableText>(e))
        .map(|t| t.value().to_string());
    assert_eq!(text.as_deref(), Some("a"));
    let state = app.world().resource::<TreeState>();
    assert_eq!(state.chip, Chip::Models);
    assert!(state.expanded.contains(&NodeKey::Entity("amr_01".into())));
}

/// Thousands of entities: the content is as tall as every row, the pool
/// only as large as the view, and scrolling recycles it.
#[test]
fn the_tree_is_virtualized() {
    let (mut app, w) = live_tree();
    for i in 0..3_000 {
        let name = format!("box_{i:04}");
        w.add(&mut app, w.root, &name, &name, NodeKind::Model);
    }
    let start = std::time::Instant::now();
    frames(&mut app, 2);
    let took = start.elapsed();
    assert!(all_text(&mut app).contains("3007 entities"));
    let pool = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&TreeSlot>().iter(world).count()
    };
    let n = pool(&mut app);
    let list = find::<TreeList>(&mut app);
    let (_, size) = rect(&app, list);
    let row_h = 28.0;
    assert!(
        size.y < 900.0 && n as f32 <= size.y / row_h + 3.0,
        "{n} pooled rows"
    );
    // Scroll to the end: the same pool shows the last rows.
    app.world_mut().get_mut::<ScrollPosition>(list).unwrap().y = 3_004.0 * row_h;
    frames(&mut app, 3);
    assert_eq!(pool(&mut app), n);
    let shown = rows(&mut app);
    assert_eq!(shown.last().map(String::as_str), Some("  M box_2999"));
    assert!(shown.iter().any(|r| r == "  M box_2990"));
    assert!(app.world().resource::<TreeScroll>().0 > 0.0);
    // A generous bound: the frame that rebuilt a 3,000-model tree.
    assert!(took.as_secs_f32() < 2.0, "{took:?}");
}

/// The tree is a tab stop in the sidebar; arrow keys move its cursor,
/// which shows the focus ring, and open and close rows.
#[test]
fn the_tree_takes_tab_and_the_arrow_keys() {
    let (mut app, _) = live_tree();
    let list = find::<TreeList>(&mut app);
    // Tab reaches the tree from the filter's neighbours.
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    let mut reached = false;
    for _ in 0..60 {
        key(&mut app, KeyCode::Tab, ButtonState::Pressed);
        app.update();
        key(&mut app, KeyCode::Tab, ButtonState::Released);
        app.update();
        if focused(&app) == Some(list) {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never reached the tree");
    let press = |app: &mut App, k: KeyCode| {
        key(app, k, ButtonState::Pressed);
        app.update();
        key(app, k, ButtonState::Released);
        frames(app, 2);
    };
    let cursor = |app: &App| app.world().resource::<TreeState>().cursor.clone();
    press(&mut app, KeyCode::ArrowDown);
    assert_eq!(cursor(&app), Some(NodeKey::World));
    press(&mut app, KeyCode::ArrowDown);
    assert_eq!(cursor(&app), Some(NodeKey::Entity("amr_01".into())));
    let amr = row(&mut app, "amr_01");
    assert_eq!(app.world().get::<Ring>(amr), Some(&Ring(ColorRole::Accent)));
    press(&mut app, KeyCode::ArrowRight);
    assert_eq!(rows(&mut app)[1], "  v M amr_01");
    press(&mut app, KeyCode::ArrowRight);
    assert_eq!(
        cursor(&app),
        Some(NodeKey::Entity("amr_01/base_link".into()))
    );
    press(&mut app, KeyCode::ArrowLeft);
    assert_eq!(cursor(&app), Some(NodeKey::Entity("amr_01".into())));
    press(&mut app, KeyCode::ArrowLeft);
    assert_eq!(rows(&mut app)[1], "  > M amr_01");
    press(&mut app, KeyCode::End);
    assert_eq!(cursor(&app), Some(NodeKey::Entity("sun".into())));
    press(&mut app, KeyCode::PageUp);
    assert_eq!(cursor(&app), Some(NodeKey::World));
    press(&mut app, KeyCode::ArrowUp);
    assert_eq!(cursor(&app), Some(NodeKey::World));
    // A click moves the cursor too.
    let sun = row(&mut app, "sun");
    app.world_mut().trigger(Activate { entity: sun });
    frames(&mut app, 1);
    assert_eq!(cursor(&app), Some(NodeKey::Entity("sun".into())));
}

/// `SceneTreeStart` (a capture's `--scene-filter`, `--scene-expand`)
/// applies once the first tree exists, to the state and the field.
#[test]
fn a_start_filter_and_expansion_apply_once() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(super::SceneTreeStart {
        filter: "cam".into(),
        expand_all: true,
    });
    frames(&mut app, 2);
    go_live(&mut app, false);
    frames(&mut app, 2);
    assert!(app.world().contains_resource::<super::SceneTreeStart>());
    World3::spawn(&mut app);
    frames(&mut app, 3);
    assert!(!app.world().contains_resource::<super::SceneTreeStart>());
    assert_eq!(
        rows(&mut app),
        [
            "v W warehouse_demo",
            "  v M amr_01",
            "    v K base_link",
            "      S front_camera"
        ]
    );
    let field = find::<FilterField>(&mut app);
    let text = descendants(app.world(), field)
        .into_iter()
        .find_map(|e| app.world().get::<EditableText>(e))
        .map(|t| t.value().to_string());
    assert_eq!(text.as_deref(), Some("cam"));
    set_filter(&mut app, "");
    assert_eq!(rows(&mut app).len(), 8);
}

/// `--scene-expand` on a world that starts with no models keeps the world
/// open and opens the models that come later.
#[test]
fn a_start_expansion_waits_for_models() {
    let mut app = shell_app((1440, 900), 1.0);
    app.insert_resource(super::SceneTreeStart {
        filter: String::new(),
        expand_all: true,
    });
    frames(&mut app, 2);
    let root = {
        let world = app.world_mut();
        let root = world
            .spawn((Transform::default(), Visibility::default()))
            .id();
        let mut index = world.resource_mut::<GzIndex>();
        index.root = Some(root);
        index.scene = Some(gz_core::scene::SceneDesc::from_msg(
            &Default::default(),
            Default::default(),
        ));
        root
    };
    go_live(&mut app, false);
    frames(&mut app, 2);
    assert_eq!(rows(&mut app), ["W warehouse_demo"]);
    assert!(app.world().contains_resource::<super::SceneTreeStart>());
    let w = World3 { root };
    let amr = w.add(&mut app, root, "amr_01", "amr_01", NodeKind::Model);
    w.add(
        &mut app,
        amr,
        "base_link",
        "amr_01/base_link",
        NodeKind::Link,
    );
    frames(&mut app, 2);
    assert_eq!(
        rows(&mut app),
        ["v W warehouse_demo", "  v M amr_01", "    K base_link"]
    );
    assert!(!app.world().contains_resource::<super::SceneTreeStart>());
}

/// The sim's stats change `GzStatus` several times a second; the tree is
/// rebuilt only when its world or its entities change. (A name changed in
/// place is no structural change: it shows only once something else
/// rebuilds the tree, which a stats message no longer does.)
#[test]
fn stats_messages_leave_the_tree_alone() {
    let (mut app, _) = live_tree();
    let crate_01 = {
        let world = app.world_mut();
        world
            .query::<(Entity, &GzEntity)>()
            .iter(world)
            .find(|(_, g)| g.name == "crate_01")
            .unwrap()
            .0
    };
    app.world_mut().get_mut::<GzEntity>(crate_01).unwrap().name = "renamed".into();
    for k in 0..3 {
        if let Some(sim) = app.world_mut().resource_mut::<GzStatus>().sim.as_mut() {
            sim.iterations += 1 + k;
        }
        app.world_mut().resource_mut::<GzStatus>().set_changed();
        app.update();
    }
    assert!(rows(&mut app).iter().any(|r| r.ends_with("crate_01")));
    app.world_mut().resource_mut::<GzStatus>().world = Some("other".into());
    frames(&mut app, 2);
    let shown = rows(&mut app);
    assert!(shown.iter().any(|r| r.ends_with("renamed")), "{shown:?}");
    assert!(shown[0].ends_with("other"), "{shown:?}");
}

/// Scrolled by any amount, the shown rows cover the list from its top to
/// its bottom: rows sit inside the list's padding, so the row above a
/// boundary still shows for the padding's height after it.
#[test]
fn the_pool_covers_the_list_at_any_scroll() {
    let (mut app, w) = live_tree();
    for i in 0..300 {
        let name = format!("box_{i:04}");
        w.add(&mut app, w.root, &name, &name, NodeKind::Model);
    }
    frames(&mut app, 2);
    let list = find::<TreeList>(&mut app);
    for y in [0.0, 2800.0, 2804.0, 2810.0, 2827.0, 4000.0] {
        app.world_mut().get_mut::<ScrollPosition>(list).unwrap().y = y;
        frames(&mut app, 3);
        let (top, size) = rect(&app, list);
        let world = app.world_mut();
        let shown: Vec<Entity> = world
            .query::<(Entity, &TreeSlot, &Node)>()
            .iter(world)
            .filter(|(_, s, n)| s.0.is_some() && n.display != Display::None)
            .map(|(e, ..)| e)
            .collect();
        let spans: Vec<(f32, f32)> = shown
            .into_iter()
            .map(|e| {
                let (at, s) = rect(&app, e);
                (at.y, at.y + s.y)
            })
            .collect();
        let first = spans.iter().map(|s| s.0).fold(f32::MAX, f32::min);
        let last = spans.iter().map(|s| s.1).fold(f32::MIN, f32::max);
        assert!(
            first <= top.y + 8.0 + 0.5,
            "{y}: first row at {first}, list at {}",
            top.y
        );
        if y > 8.0 {
            assert!(first <= top.y + 0.5, "{y}: a gap above the first row");
        }
        assert!(
            last >= top.y + size.y - 0.5,
            "{y}: a gap below the last row"
        );
    }
}

/// What a screen reader gets: the tree's items under the Tree (through the
/// content node: Bevy links an accessibility node to its parent's only),
/// and only the rows shown; pooled rows with nothing to show are hidden.
#[test]
fn the_accessibility_tree_has_only_the_rows_shown() {
    use bevy::a11y::AccessibilityNode;
    let (mut app, w) = live_tree();
    for i in 0..300 {
        let name = format!("box_{i:04}");
        w.add(&mut app, w.root, &name, &name, NodeKind::Model);
    }
    frames(&mut app, 2);
    let items = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&TreeSlot, &AccessibilityNode, &ChildOf)>()
            .iter(world)
            .map(|(s, a, p)| {
                (
                    s.0.is_some(),
                    a.is_hidden(),
                    a.label().map(str::to_owned),
                    p.parent(),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = items(&mut app);
    assert!(before.len() > 20);
    let content = before[0].3;
    let tree = app.world().get::<ChildOf>(content).unwrap().parent();
    assert!(app.world().get::<AccessibilityNode>(content).is_some());
    assert_eq!(
        app.world().get::<AccessibilityNode>(tree).map(|a| a.role()),
        Some(accesskit::Role::Tree)
    );
    assert!(before.iter().all(|(shown, hidden, ..)| shown != hidden));
    caret(&mut app, "warehouse_demo");
    frames(&mut app, 2);
    let after = items(&mut app);
    let exposed: Vec<_> = after.iter().filter(|i| !i.1).collect();
    assert_eq!(exposed.len(), 1, "{exposed:?}");
    assert_eq!(exposed[0].2.as_deref(), Some("warehouse_demo"));
    caret(&mut app, "warehouse_demo");
    frames(&mut app, 2);
    assert!(items(&mut app).iter().filter(|i| !i.1).count() > 20);
}
