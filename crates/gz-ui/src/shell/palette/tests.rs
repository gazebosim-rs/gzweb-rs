//! The command palette: ranking, and the palette through the shell.

use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::ButtonState;
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use gz_bevy::spawn::{GzEntity, GzIndex, GzWorldRoot};
use gz_bevy::view::controls::RunBinding;
use gz_bevy::view::Selection;
use gz_core::scene::NodeKind;

use super::model::*;
use super::view::{PaletteBackdrop, PaletteField, PaletteRoot, PaletteRow};
use super::{PaletteOpen, PaletteState};
use crate::assets::icons::IconId;
use crate::panels::{CONSOLE, TOPICS};
use crate::shell::top_bar::search::SearchTrigger;
use crate::shell::{Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::theme::{MotionMode, Theme};
use crate::widgets::text_field::{set_field_text, TextFieldInput};
use crate::widgets::TextSubmit;

fn item(group: Group, name: &str) -> Item {
    Item {
        group,
        icon: IconId::Search,
        name: name.into(),
        hint: String::new(),
        shortcut: None,
        keywords: vec![],
        run: Run::Binding(String::new()),
        disabled: None,
    }
}

/// The whole name beats its start, which beats a word's start, which
/// beats letters in order; camel case starts words; no match is none.
#[test]
fn queries_score_names() {
    let s = |q, t| score(q, t);
    assert!(s("grid", "grid") > s("grid", "grid lines"));
    assert!(s("grid", "grid lines") > s("grid", "toggle grid"));
    assert!(s("grid", "toggle grid") > s("grid", "togglegrid"));
    assert!(s("tv", "TopicViewer") > s("tv", "tinyvessel"));
    assert!(s("viewer", "TopicViewer").is_some());
    assert_eq!(s("xyz", "grid"), None);
    assert_eq!(s("  ", "anything"), Some(0));
    // A keyword finds its item, a little below its own name.
    let mut topics = item(Group::Panels, "Topics");
    topics.keywords = vec!["TopicViewer"];
    assert!(item_score("TopicViewer", &topics).is_some());
    assert!(item_score("topics", &topics) > item_score("TopicViewer", &topics));
}

/// Results come by group, best first, capped; with no query the world's
/// entities and topics stay out; ↑ and ↓ skip unavailable results and
/// wrap.
#[test]
fn results_rank_by_group_and_step() {
    let mut items = vec![
        item(Group::Panels, "Plot"),
        item(Group::Commands, "Toggle grid"),
        item(Group::Entities, "grid_marker"),
    ];
    items.extend((0..20).map(|i| item(Group::Topics, &format!("/grid/{i}"))));
    let names = |r: &[Item]| r.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
    let all = rank(items.clone(), "");
    assert_eq!(names(&all), ["Toggle grid", "Plot"]);
    let grid = rank(items.clone(), "grid");
    assert_eq!(grid[0].name, "Toggle grid");
    // "plt" is only scattered in "Plot"; with "Plot" whole, nothing else.
    let mut both = items.clone();
    both.push(item(Group::Commands, "Split it"));
    assert_eq!(names(&rank(both.clone(), "plot")), ["Plot"]);
    assert!(rank(both, "plt").len() >= 2);
    assert_eq!(grid[1].group, Group::Entities);
    assert_eq!(
        grid.iter().filter(|i| i.group == Group::Topics).count(),
        Group::Topics.cap()
    );
    let mut list = vec![
        item(Group::Commands, "a"),
        item(Group::Commands, "b"),
        item(Group::Commands, "c"),
    ];
    list[1].disabled = Some("no");
    assert_eq!(step(&list, None, 1), Some(0));
    assert_eq!(step(&list, Some(0), 1), Some(2));
    assert_eq!(step(&list, Some(2), 1), Some(0));
    assert_eq!(step(&list, Some(0), -1), Some(2));
    assert_eq!(step(&[], None, 1), None);
    assert_eq!(gazebo_names("scene"), ["EntityTree"]);
    assert!(gazebo_names("nope").is_empty());
}

fn open(app: &mut App) {
    app.world_mut().write_message(UiCommand::OpenPalette);
    frames(app, 3);
}

fn field_input(app: &mut App) -> Entity {
    let world = app.world_mut();
    let field = world
        .query_filtered::<Entity, With<PaletteField>>()
        .single(world)
        .unwrap();
    world
        .query_filtered::<(Entity, &ChildOf), With<TextFieldInput>>()
        .iter(world)
        .find(|(_, p)| p.parent() == field)
        .unwrap()
        .0
}

fn type_query(app: &mut App, q: &str) {
    let input = field_input(app);
    let mut text = app.world_mut().get_mut::<EditableText>(input).unwrap();
    set_field_text(&mut text, q);
    frames(app, 3);
}

fn results(app: &App) -> Vec<String> {
    app.world()
        .resource::<PaletteState>()
        .results
        .iter()
        .map(|i| i.name.clone())
        .collect()
}

fn enter(app: &mut App) {
    let field = find::<PaletteField>(app);
    app.world_mut().trigger(TextSubmit {
        entity: field,
        value: String::new(),
    });
    frames(app, 3);
}

fn is_open(app: &mut App) -> bool {
    let world = app.world_mut();
    world
        .query_filtered::<(), With<PaletteRoot>>()
        .iter(world)
        .next()
        .is_some()
}

/// The bindings run so far.
#[derive(Resource, Default)]
struct Ran(Vec<String>);

fn record(mut runs: MessageReader<RunBinding>, mut ran: ResMut<Ran>) {
    ran.0.extend(runs.read().map(|r| r.0.clone()));
}

/// ⌘K opens it with the keys in its field; a command runs as its key
/// would; a Gazebo plugin's name finds its panel; Esc closes it and
/// gives the keys back.
#[test]
fn the_palette_finds_and_runs_commands() {
    let mut app = shell_app((1440, 900), 1.0);
    app.init_resource::<Ran>().add_systems(Update, record);
    frames(&mut app, 2);
    go_live(&mut app, false);
    chord(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyK]);
    frames(&mut app, 2);
    assert!(is_open(&mut app));
    let input = field_input(&mut app);
    assert_eq!(focused(&app), Some(input));
    let all = results(&app);
    assert!(all.iter().any(|n| n == "Topics"));
    assert!(all.iter().any(|n| n == "Theme: light"));

    type_query(&mut app, "grid");
    let first = results(&app)[0].to_lowercase();
    assert!(first.contains("grid"), "{:?}", results(&app));
    enter(&mut app);
    assert!(!is_open(&mut app));
    // It runs as G would (gz-bevy's key_input toggles the grid for it).
    assert_eq!(app.world().resource::<Ran>().0, ["overlay.grid"]);

    open(&mut app);
    type_query(&mut app, "TopicViewer");
    assert_eq!(results(&app)[0], "Topics");
    enter(&mut app);
    assert_eq!(layout(&app).primary, Some(TOPICS));

    // Esc closes and gives focus back.
    open(&mut app);
    let input = field_input(&mut app);
    focused_key(&mut app, input, KeyCode::Escape);
    frames(&mut app, 2);
    assert!(!is_open(&mut app));
    assert_ne!(focused(&app), Some(input));
}

/// An unavailable result (Step while running) isn't chosen and doesn't
/// run; ↓ chooses the next available one; a press outside closes; the
/// top bar's search opens it.
#[test]
fn rows_choose_click_and_close() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    // Opened before the world is live: once it is, the first available
    // result (Save, then) is chosen.
    open(&mut app);
    let s = app.world().resource::<PaletteState>().clone();
    assert_ne!(
        s.results[s.selected.unwrap()].name,
        "Save",
        "not while offline"
    );
    go_live(&mut app, false);
    frames(&mut app, 2);
    let s = app.world().resource::<PaletteState>().clone();
    assert_eq!(s.results[s.selected.unwrap()].name, "Save");
    type_query(&mut app, "step");
    let state = app.world().resource::<PaletteState>().clone();
    let step_i = state.results.iter().position(|i| i.name.contains("Step"));
    let step_item = &state.results[step_i.unwrap()];
    assert_eq!(step_item.disabled, Some("Not while running"));
    assert_ne!(state.selected, step_i, "an unavailable result isn't chosen");
    sent(&mut app);
    let row = {
        let world = app.world_mut();
        world
            .query::<(Entity, &PaletteRow)>()
            .iter(world)
            .find(|(_, r)| Some(r.0) == step_i)
            .unwrap()
            .0
    };
    app.world_mut().trigger(Activate { entity: row });
    frames(&mut app, 2);
    assert!(is_open(&mut app), "an unavailable result runs nothing");
    assert!(!sent(&mut app)
        .iter()
        .any(|c| matches!(c, gz_bevy::connection::GzCommand::Step(_))));
    // Nothing matches: Enter does nothing either.
    type_query(&mut app, "zzqqx");
    assert!(results(&app).is_empty());
    enter(&mut app);
    assert!(is_open(&mut app));
    type_query(&mut app, "");
    let first = app.world().resource::<PaletteState>().selected;
    let input = field_input(&mut app);
    focused_key(&mut app, input, KeyCode::ArrowDown);
    let next = app.world().resource::<PaletteState>().selected;
    assert_ne!(first, next);
    let s = app.world().resource::<PaletteState>().clone();
    assert!(s.results[next.unwrap()].disabled.is_none());

    let backdrop = find::<PaletteBackdrop>(&mut app);
    app.world_mut().trigger(PointerPress {
        entity: backdrop,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
        count: 1,
    });
    frames(&mut app, 2);
    assert!(!is_open(&mut app));
    assert!(!app.world().resource::<PaletteOpen>().0);

    let trigger = find::<SearchTrigger>(&mut app);
    app.world_mut().trigger(Activate { entity: trigger });
    frames(&mut app, 3);
    assert!(is_open(&mut app));
}

/// The world's entities and the server's topics are found by name: an
/// entity's result selects it, a topic's echoes it; a shown panel's
/// actions are commands.
#[test]
fn entities_topics_and_panel_actions() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let amr = world
        .spawn((
            GzEntity {
                name: "amr_01".into(),
                id: 0,
                kind: NodeKind::Model,
                path: "amr_01".into(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    go_live(&mut app, false);
    let mut topics = gz_bevy::topics::GzTopics::default();
    topics.set(vec![("/amr_01/odom".into(), "gz.msgs.Odometry".into())]);
    *app.world_mut().resource_mut::<gz_bevy::topics::GzTopics>() = topics;
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: CONSOLE,
        slot: Slot::Primary,
    });
    frames(&mut app, 3);

    open(&mut app);
    let all = results(&app);
    assert!(
        all.iter().any(|n| n == "Console: Clear messages"),
        "{all:?}"
    );
    // A model that loads while it's open joins its results.
    type_query(&mut app, "crate");
    assert!(!results(&app).contains(&"crate_7".to_string()));
    app.world_mut().spawn((
        GzEntity {
            name: "crate_7".into(),
            id: 7,
            kind: NodeKind::Model,
            path: "crate_7".into(),
        },
        Transform::default(),
        Visibility::default(),
        ChildOf(root),
    ));
    frames(&mut app, 2);
    assert_eq!(results(&app)[0], "crate_7");
    let state = app.world().resource::<PaletteState>().clone();
    assert_eq!(state.results[0].hint, "model");
    type_query(&mut app, "");
    assert!(
        !all.iter().any(|n| n == "amr_01"),
        "no entities without a query"
    );
    type_query(&mut app, "amr");
    let state = app.world().resource::<PaletteState>().clone();
    let groups: Vec<Group> = state.results.iter().map(|i| i.group).collect();
    assert!(groups.contains(&Group::Entities) && groups.contains(&Group::Topics));
    enter(&mut app);
    assert_eq!(app.world().resource::<Selection>().entities, [amr]);

    open(&mut app);
    type_query(&mut app, "odom");
    assert_eq!(results(&app)[0], "/amr_01/odom");
    enter(&mut app);
    frames(&mut app, 2);
    assert_eq!(layout(&app).primary, Some(TOPICS));
}

/// An arrow key as a keyboard sends it (its logical key too, which the
/// text input reads), in focused `e`.
fn arrow(app: &mut App, e: Entity, key_code: KeyCode, key: Key) {
    let window = find::<Window>(app);
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(e, bevy::input_focus::FocusCause::Navigated);
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: key.clone(),
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }
    app.update();
}

/// Tab and a press that runs nothing leave the keys in the field; ↑ and
/// ↓ leave the caret at the end of the query; a re-rank scrolls back to
/// the top; a ↑/↓ choice survives a refresh by what it runs, not its name.
#[test]
fn the_field_keeps_the_keys_and_the_choice() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let world = app.world_mut();
    let root = world
        .spawn((Transform::default(), Visibility::default(), GzWorldRoot))
        .id();
    world.resource_mut::<GzIndex>().root = Some(root);
    let link = |world: &mut World, model: &str| {
        world
            .spawn((
                GzEntity {
                    name: "left_wheel".into(),
                    id: 0,
                    kind: NodeKind::Link,
                    path: format!("{model}/left_wheel"),
                },
                Transform::default(),
                Visibility::default(),
                ChildOf(root),
            ))
            .id()
    };
    link(world, "blue");
    let green = link(world, "green");
    go_live(&mut app, false);
    open(&mut app);
    let input = field_input(&mut app);

    focused_key(&mut app, input, KeyCode::Tab);
    frames(&mut app, 2);
    assert_eq!(focused(&app), Some(input), "Tab stays in the palette");
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    frames(&mut app, 1);
    assert_eq!(focused(&app), Some(input), "a press elsewhere in it");

    type_query(&mut app, "left_wh");
    arrow(&mut app, input, KeyCode::ArrowDown, Key::ArrowDown);
    let caret = app
        .world()
        .get::<EditableText>(input)
        .unwrap()
        .editor
        .raw_selection()
        .focus()
        .index();
    assert_eq!(caret, "left_wh".len(), "the caret stays at the end");
    let s = app.world().resource::<PaletteState>().clone();
    let chosen = &s.results[s.selected.unwrap()];
    assert_eq!(chosen.run, Run::Select(green), "{:?}", s.results);
    // A model loading re-ranks; the choice stays on green's wheel.
    app.world_mut().spawn((
        GzEntity {
            name: "left_wheel_2".into(),
            id: 9,
            kind: NodeKind::Link,
            path: "red/left_wheel_2".into(),
        },
        Transform::default(),
        Visibility::default(),
        ChildOf(root),
    ));
    frames(&mut app, 2);
    let s = app.world().resource::<PaletteState>().clone();
    assert_eq!(s.results[s.selected.unwrap()].run, Run::Select(green));
    // The overlays without keys are commands too.
    type_query(&mut app, "wireframe");
    assert_eq!(results(&app)[0], "Toggle wireframe");
}

/// The workspaces are commands: one switches, Save keeps the layout.
#[test]
fn the_palette_switches_workspaces() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    open(&mut app);
    type_query(&mut app, "workspace debug");
    assert_eq!(
        results(&app).first().map(String::as_str),
        Some("Workspace: Debug")
    );
    enter(&mut app);
    assert_eq!(
        app.world().resource::<crate::shell::Workspaces>().current,
        2
    );
    open(&mut app);
    type_query(&mut app, "save current layout");
    assert_eq!(
        results(&app).first().map(String::as_str),
        Some("Save current layout")
    );
    // Save as new opens the Workspaces panel naming one.
    open(&mut app);
    type_query(&mut app, "new workspace");
    assert_eq!(
        results(&app).first().map(String::as_str),
        Some("Save layout as new workspace")
    );
    enter(&mut app);
    frames(&mut app, 3);
    assert_eq!(layout(&app).primary, Some(crate::shell::WORKSPACES_PANEL));
    assert!(
        app.world()
            .resource::<crate::panels::workspaces::PanelState>()
            .naming
    );
}

/// Settings has the UI scales and the motion settings (D-118): one runs,
/// the theme takes it, and the one in effect says "current".
#[test]
fn the_palette_sets_the_ui_scale_and_motion() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let hint = |app: &App, name: &str| {
        app.world()
            .resource::<PaletteState>()
            .results
            .iter()
            .find(|i| i.name == name)
            .map(|i| i.hint.clone())
    };
    open(&mut app);
    type_query(&mut app, "ui scale");
    let all = results(&app);
    for pct in [80, 90, 100, 110, 125, 150, 175, 200] {
        assert!(
            all.contains(&format!("UI scale: {pct}%")),
            "{pct} in {all:?}"
        );
    }
    assert_eq!(hint(&app, "UI scale: 100%").as_deref(), Some("current"));
    type_query(&mut app, "150%");
    assert_eq!(
        results(&app).first().map(String::as_str),
        Some("UI scale: 150%")
    );
    enter(&mut app);
    assert_eq!(app.world().resource::<Theme>().scale, 1.5);
    assert_eq!(app.world().resource::<UiScale>().0, 1.5);
    open(&mut app);
    type_query(&mut app, "ui scale");
    assert_eq!(hint(&app, "UI scale: 150%").as_deref(), Some("current"));
    assert_eq!(hint(&app, "UI scale: 100%").as_deref(), Some(""));

    type_query(&mut app, "reduce motion");
    assert_eq!(
        results(&app).first().map(String::as_str),
        Some("Motion: reduce")
    );
    enter(&mut app);
    let theme = app.world().resource::<Theme>();
    assert!(theme.reduce_motion && theme.motion == MotionMode::Reduce);
    open(&mut app);
    type_query(&mut app, "motion");
    assert_eq!(hint(&app, "Motion: reduce").as_deref(), Some("current"));
    for name in ["Motion: full", "Motion: follow system"] {
        assert!(results(&app).iter().any(|n| n == name), "{name}");
    }
}
