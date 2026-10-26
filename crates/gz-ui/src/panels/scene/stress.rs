//! The Scene panel at scale (UI_PLAN M10e: a 5,000-entity tree at 60 fps):
//! the full UI with the world `scripts/stress-world.sh` makes, built here
//! as gz-bevy spawns it.
//!
//! `a_5000_entity_tree_bounds_its_work_per_frame` checks what each frame
//! changes; `bench_a_5000_entity_tree` times the frames of each thing a
//! user does to the tree (ignored; run it optimized, see the WORKLOG's
//! M10e entry).

use std::time::Instant;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::extras::{SdfExtras, SdfWanted};
use gz_bevy::spawn::{GzEntity, GzIndex};
use gz_bevy::undo::UndoCommand;
use gz_core::scene::NodeKind;
use gz_core::sdf::WorldExtras;

use super::footer::FooterButton::{self, HideOthers, Isolate};
use super::systems::{TreeRows, TreeWork};
use super::view::{FilterField, TreeList, TreeSlot};
use super::{EXPAND_ALL, SCENE};
use crate::shell::sidebar::body::ActionButton;
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::widgets::TreeRowSpec;

/// The stress world's models: four tree entities each.
const MODELS: usize = 1_250;

/// The world's SDFormat as far as the tree reads it: each model's hinge.
fn sdf(models: usize) -> String {
    let mut out = String::from(r#"<sdf version="1.10"><world name="stress">"#);
    for m in 0..models {
        out.push_str(&format!(
            r#"<model name="unit_{m}"><link name="base"/><link name="arm"/>
            <joint name="hinge" type="revolute"><parent>base</parent><child>arm</child>
            <axis><xyz>1 0 0</xyz></axis></joint></model>"#
        ));
    }
    out.push_str("</world></sdf>");
    out
}

fn spawn(world: &mut World, parent: Entity, name: &str, path: &str, kind: NodeKind) -> Entity {
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

/// The full UI, live, with `models` models (a base and an arm link, a
/// visual on each, and the hinge between them) and a sun: `4 × models + 1`
/// tree entities.
pub(super) fn stress_tree(models: usize) -> App {
    let mut app = shell_app((1440, 900), 1.0);
    app.init_resource::<SdfExtras>()
        .init_resource::<SdfWanted>();
    frames(&mut app, 2);
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
    for m in 0..models {
        let name = format!("unit_{m}");
        let model = spawn(world, root, &name, &name, NodeKind::Model);
        for link in ["base", "arm"] {
            let path = format!("{name}/{link}");
            let l = spawn(world, model, link, &path, NodeKind::Link);
            spawn(
                world,
                l,
                "visual",
                &format!("{path}/visual"),
                NodeKind::Visual,
            );
        }
    }
    spawn(world, root, "sun", "sun", NodeKind::Light);
    world.resource_mut::<SdfExtras>().extras =
        Some(WorldExtras::parse(&sdf(models)).expect("the stress SDFormat parses"));
    go_live(&mut app, false);
    frames(&mut app, 2);
    app
}

pub(super) fn expand_all(app: &mut App) {
    let world = app.world_mut();
    let header = world
        .query::<(Entity, &ActionButton)>()
        .iter(world)
        .find(|(_, b)| b.panel == SCENE && b.action == EXPAND_ALL)
        .expect("Expand all")
        .0;
    world.trigger(Activate { entity: header });
}

pub(super) fn scroll_by(app: &mut App, dy: f32) {
    let list = find::<TreeList>(app);
    app.world_mut()
        .get_mut::<ScrollPosition>(list)
        .expect("the list scrolls")
        .y += dy;
}

pub(super) fn type_filter(app: &mut App, text: &str) {
    let field = find::<FilterField>(app);
    let input = descendants(app.world(), field)
        .into_iter()
        .find(|e| app.world().get::<EditableText>(*e).is_some())
        .expect("the filter's input");
    app.world_mut()
        .entity_mut(input)
        .insert(EditableText::new(text));
}

pub(super) fn models(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &GzEntity)>()
        .iter(world)
        .filter(|(_, g)| g.kind == NodeKind::Model)
        .map(|(e, _)| e)
        .collect()
}

/// What a frame did: the tree's models built and rows computed, and the
/// pooled rows it wrote.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
struct Work {
    models: u64,
    rows: u64,
    specs: usize,
}

fn count_specs(specs: Query<(), (Changed<TreeRowSpec>, With<TreeSlot>)>, mut work: ResMut<Work>) {
    work.specs = specs.iter().count();
}

/// Runs a frame after `step`; what it did.
fn work(app: &mut App, step: impl FnOnce(&mut App)) -> Work {
    let before = *app.world().resource::<TreeWork>();
    step(app);
    app.update();
    let after = *app.world().resource::<TreeWork>();
    Work {
        models: after.models - before.models,
        rows: after.rows - before.rows,
        specs: std::mem::take(&mut app.world_mut().resource_mut::<Work>().specs),
    }
}

/// A 5,000-entity tree does a screenful's work a frame, not the world's
/// (UI_PLAN M10e): the model is built when entities come, the rows only
/// when the expansion or the filter changes, and a frame writes at most the
/// pool's rows (none for a selection, a hover or a frame with nothing new).
/// Hide others, Isolate and their undo put every model back.
#[test]
fn a_5000_entity_tree_bounds_its_work_per_frame() {
    let mut app = stress_tree(MODELS);
    app.init_resource::<Work>().add_systems(Last, count_specs);
    frames(&mut app, 2);
    assert!(all_text(&mut app).contains("5001 entities"));
    let pool = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&TreeSlot>().iter(world).count()
    };
    let idle = Work::default();
    assert_eq!(work(&mut app, |_| {}), idle);
    let expanded = work(&mut app, expand_all);
    assert_eq!((expanded.models, expanded.rows), (0, 1), "{expanded:?}");
    frames(&mut app, 2);
    let n = pool(&mut app);
    assert!(n <= 900 / 28 + 3, "{n} pooled rows");
    assert_eq!(app.world().resource::<TreeRows>().0.len(), 4 * MODELS + 2);
    assert_eq!(work(&mut app, |_| {}), idle);
    // Scrolling recycles the pool.
    for _ in 0..3 {
        let w = work(&mut app, |app| scroll_by(app, 28.0));
        assert!(w.models + w.rows == 0 && w.specs <= n, "{w:?}");
    }
    assert_eq!(pool(&mut app), n);
    // A selection marks rows, a hover outlines; neither writes one.
    let all = models(&mut app);
    for selected in [vec![all[1]], all.clone(), vec![]] {
        let w = work(&mut app, |app| {
            app.world_mut()
                .resource_mut::<gz_bevy::view::Selection>()
                .entities = selected;
        });
        assert_eq!(w, idle, "{w:?}");
    }
    let row = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<TreeSlot>>()
            .iter(world)
            .next()
            .expect("a row")
    };
    assert_eq!(
        work(&mut app, |app| {
            app.world_mut().entity_mut(row).insert(Hovered(true));
        }),
        idle
    );
    // The server's stats and every link's pose, as each message brings
    // them, are nothing to the tree.
    let links: Vec<Entity> = {
        let world = app.world_mut();
        world
            .query::<(Entity, &GzEntity)>()
            .iter(world)
            .filter(|(_, g)| g.kind == NodeKind::Link)
            .map(|(e, _)| e)
            .collect()
    };
    for _ in 0..2 {
        let w = work(&mut app, |app| {
            let world = app.world_mut();
            if let Some(sim) = world.resource_mut::<GzStatus>().sim.as_mut() {
                sim.iterations += 1;
            }
            for l in &links {
                if let Some(mut t) = world.get_mut::<Transform>(*l) {
                    t.translation.x += 0.01;
                }
            }
        });
        assert_eq!(w, idle, "{w:?}");
    }
    // A filter is new rows, from the same model.
    let w = work(&mut app, |app| type_filter(app, "unit_12"));
    assert!(w.models == 0 && w.rows == 1 && w.specs <= n, "{w:?}");
    assert!(work(&mut app, |app| type_filter(app, "")).specs <= n);
    // Hide others, Isolate and undo: every model as it was.
    app.world_mut()
        .resource_mut::<gz_bevy::view::Selection>()
        .entities = vec![all[0]];
    frames(&mut app, 2);
    let shown = |app: &App| {
        all.iter()
            .filter(|e| app.world().get::<Visibility>(**e) == Some(&Visibility::Inherited))
            .count()
    };
    let undo = |app: &mut App| {
        app.world_mut().write_message(UndoCommand::Undo);
        frames(app, 2);
    };
    let w = work(&mut app, |app| footer(app, HideOthers));
    assert!(w.models + w.rows == 0 && w.specs <= n, "{w:?}");
    frames(&mut app, 1);
    assert_eq!(shown(&app), 1);
    undo(&mut app);
    assert_eq!(shown(&app), MODELS);
    footer(&mut app, Isolate);
    frames(&mut app, 2);
    assert_eq!(shown(&app), 1);
    assert_eq!(work(&mut app, |_| {}), idle);
    footer(&mut app, Isolate);
    frames(&mut app, 2);
    assert_eq!(shown(&app), MODELS);
    undo(&mut app);
    assert_eq!(shown(&app), 1);
    undo(&mut app);
    assert_eq!(shown(&app), MODELS);
}

/// Runs a frame after each `step`, `n` times; the frames' times in ms.
fn time(app: &mut App, n: usize, mut step: impl FnMut(&mut App, usize)) -> Vec<f64> {
    (0..n)
        .map(|i| {
            step(app, i);
            let start = Instant::now();
            app.update();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect()
}

fn report(what: &str, mut ms: Vec<f64>) {
    ms.sort_by(f64::total_cmp);
    let n = ms.len();
    println!(
        "{what:<28} median {:>7.2} ms  max {:>7.2} ms  ({n} frames)",
        ms[n / 2],
        ms[n - 1]
    );
}

/// The frames of what a user does to a 5,000-entity tree, each timed.
#[test]
#[ignore = "a measurement, not a check: run it optimized (WORKLOG M10e)"]
fn bench_a_5000_entity_tree() {
    let start = Instant::now();
    // `GZ_BENCH_MODELS` compares another size.
    let models_n = std::env::var("GZ_BENCH_MODELS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(MODELS);
    let mut app = stress_tree(models_n);
    println!("built in {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    report("idle, collapsed", time(&mut app, 60, |_, _| {}));
    report("expand all", time(&mut app, 1, |app, _| expand_all(app)));
    report("idle, expanded", time(&mut app, 60, |_, _| {}));
    report(
        "scroll 37 px a frame",
        time(&mut app, 120, |app, _| scroll_by(app, 37.0)),
    );
    let words = [
        "u", "un", "uni", "unit", "unit_", "unit_1", "unit_12", "unit_123", "",
    ];
    report(
        "type a filter",
        time(&mut app, words.len(), |app, i| type_filter(app, words[i])),
    );
    let all = models(&mut app);
    report(
        "select a model a frame",
        time(&mut app, 60, |app, i| {
            app.world_mut()
                .resource_mut::<gz_bevy::view::Selection>()
                .entities = vec![all[i * 17 % all.len()]];
        }),
    );
    report(
        "select every model",
        time(&mut app, 1, |app, _| {
            app.world_mut()
                .resource_mut::<gz_bevy::view::Selection>()
                .entities = all.clone();
        }),
    );
    report("idle, all selected", time(&mut app, 60, |_, _| {}));
    let slots: Vec<Entity> = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<TreeSlot>>()
            .iter(world)
            .collect()
    };
    report(
        "hover a row a frame",
        time(&mut app, 60, |app, i| {
            let a = slots[i % slots.len()];
            let b = slots[(i + 1) % slots.len()];
            app.world_mut().entity_mut(a).insert(Hovered(false));
            app.world_mut().entity_mut(b).insert(Hovered(true));
        }),
    );
    // The footer and undo, with one model selected.
    app.world_mut()
        .resource_mut::<gz_bevy::view::Selection>()
        .entities = vec![all[0]];
    frames(&mut app, 2);
    // Each one and its undo, five times over.
    let undo = |app: &mut App| {
        app.world_mut().write_message(UndoCommand::Undo);
    };
    let mut times: [Vec<f64>; 6] = Default::default();
    for _ in 0..5 {
        let mut one = |app: &mut App, k: usize, step: &dyn Fn(&mut App)| {
            times[k].extend(time(app, 1, |app, _| step(app)));
            frames(app, 2);
        };
        one(&mut app, 0, &|app| footer(app, HideOthers));
        one(&mut app, 1, &undo);
        one(&mut app, 2, &|app| footer(app, Isolate));
        one(&mut app, 3, &|app| footer(app, Isolate));
        one(&mut app, 4, &undo);
        one(&mut app, 5, &undo);
    }
    let names = [
        "hide others",
        "undo it",
        "isolate",
        "end isolate",
        "undo the end",
        "undo the isolate",
    ];
    for (name, ms) in names.into_iter().zip(times) {
        report(name, ms);
    }
}

pub(super) fn footer(app: &mut App, which: FooterButton) {
    let world = app.world_mut();
    let b = world
        .query::<(Entity, &FooterButton)>()
        .iter(world)
        .find(|(_, b)| **b == which)
        .expect("the footer's button")
        .0;
    world.trigger(Activate { entity: b });
}
