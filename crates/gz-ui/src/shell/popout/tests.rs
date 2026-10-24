//! Pop-out windows: the layout's rules, and the windows through the shell.

use bevy::picking::events::{PointerDrag, PointerDragStart, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{EdgeDrag, HeaderDrag, PopOutHost, PopOutWindow, WindowAction};
use crate::panels::{PanelBody, PanelId, PLOT, SCENE, TOPICS};
use crate::shell::activity_bar::{ActivityFor, PopRing};
use crate::shell::layout::{Edges, ShellLayout, POPOUT_MIN, POPOUT_SIZE};
use crate::shell::sidebar::{PanelHeader, SlotAction};
use crate::shell::{ShownPanels, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::theme::visuals::Active;

/// Popping out takes a panel from its slot into a window on top, the next
/// one a step further; the sidebar closes with nothing left in it; a
/// popped-out panel opened again (a click, a flow) comes to the top; Dock
/// puts it back where it was; Close drops it.
#[test]
fn panels_pop_out_dock_and_close() {
    let mut l = ShellLayout::for_width(1440.0, Some(SCENE));
    l.open_in(TOPICS, Slot::Secondary);
    l.set_area(Vec2::new(800.0, 700.0));
    l.pop_out(TOPICS);
    assert_eq!((l.primary, l.secondary), (Some(SCENE), None));
    assert!(l.is_popped(TOPICS) && !l.is_active(TOPICS));
    assert_eq!(l.popouts[0].from, Slot::Secondary);
    assert_eq!(l.popouts[0].rect.size(), POPOUT_SIZE);
    l.pop_out(SCENE);
    assert_eq!(l.primary, None);
    assert!(!l.sidebar_open, "nothing left in the sidebar");
    let (a, b) = (l.popouts[0].rect.min, l.popouts[1].rect.min);
    assert_eq!(b - a, Vec2::splat(24.0));
    // Opening a popped-out panel raises it; so does its activity button.
    l.open_in(TOPICS, Slot::Primary);
    assert_eq!(l.popouts.last().unwrap().panel, TOPICS);
    assert_eq!(l.primary, None);
    l.activity_click(SCENE, false);
    assert_eq!(l.popouts.last().unwrap().panel, SCENE);
    l.pop_out(TOPICS);
    assert_eq!(l.popouts.len(), 2, "already out: raised");
    assert_eq!(l.popouts.last().unwrap().panel, TOPICS);
    // Dock: back to its slot, the sidebar open.
    l.dock(TOPICS);
    assert_eq!(l.primary, Some(TOPICS), "its secondary moves up alone");
    assert!(l.sidebar_open);
    l.close_popout(SCENE);
    assert!(l.popouts.is_empty());
    l.dock(SCENE);
    assert_eq!(l.primary, Some(TOPICS), "nothing to dock");
}

/// A window moves and resizes within the viewport area, never under
/// 320×240; a smaller area pulls the windows back in.
#[test]
fn windows_stay_in_the_area() {
    let mut l = ShellLayout::for_width(1440.0, Some(PLOT));
    l.set_area(Vec2::new(800.0, 600.0));
    l.pop_out(PLOT);
    let rect = |l: &ShellLayout| l.popouts[0].rect;
    l.move_popout(PLOT, Vec2::new(-50.0, 900.0));
    assert_eq!(rect(&l).min, Vec2::new(0.0, 600.0 - POPOUT_SIZE.y));
    l.move_popout(PLOT, Vec2::new(100.0, 50.0));
    let start = rect(&l);
    let corner = Edges {
        right: true,
        bottom: true,
        ..default()
    };
    l.resize_popout(PLOT, start, corner, Vec2::new(-500.0, -500.0));
    assert_eq!(rect(&l).size(), POPOUT_MIN);
    assert_eq!(rect(&l).min, start.min);
    l.resize_popout(PLOT, start, corner, Vec2::new(5000.0, 5000.0));
    assert_eq!(rect(&l).max, Vec2::new(800.0, 600.0));
    let left = Edges {
        left: true,
        ..default()
    };
    l.resize_popout(PLOT, start, left, Vec2::new(-30.0, 0.0));
    assert_eq!(rect(&l).min.x, 70.0);
    assert_eq!(rect(&l).max.x, start.max.x);
    l.resize_popout(PLOT, start, left, Vec2::new(-500.0, 0.0));
    assert_eq!(rect(&l).min.x, 0.0);
    let top = Edges {
        top: true,
        ..default()
    };
    l.resize_popout(PLOT, start, top, Vec2::new(0.0, 1000.0));
    assert_eq!(rect(&l).height(), POPOUT_MIN.y);
    l.set_area(Vec2::new(400.0, 300.0));
    assert!(rect(&l).max.x <= 400.0 && rect(&l).max.y <= 300.0);
    assert!(rect(&l).width() >= POPOUT_MIN.x);
    // Nothing happens to a panel that isn't out.
    l.move_popout(SCENE, Vec2::ZERO);
    l.resize_popout(SCENE, start, left, Vec2::ONE);
    l.raise(SCENE);
    assert_eq!(l.popouts.len(), 1);
}

fn window(app: &mut App, panel: PanelId) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &PopOutWindow)>()
        .iter(world)
        .find(|(_, w)| w.0 == panel)
        .map(|(e, _)| e)
}

/// The window's header.
fn header(app: &mut App, panel: PanelId) -> Entity {
    let world = app.world_mut();
    let host = world
        .query::<(Entity, &PopOutHost)>()
        .iter(world)
        .find(|(_, h)| h.0 == panel)
        .unwrap()
        .0;
    world
        .query::<(Entity, &PanelHeader, &ChildOf)>()
        .iter(world)
        .find(|(_, _, p)| p.parent() == host)
        .unwrap()
        .0
}

fn button<C: Component + PartialEq>(app: &mut App, under: Entity, which: C) -> Entity {
    let world = app.world_mut();
    let found = world
        .query::<(Entity, &C, &ChildOf)>()
        .iter(world)
        .find(|(_, c, p)| **c == which && p.parent() == under)
        .map(|(e, ..)| e);
    found.unwrap()
}

fn activate(app: &mut App, e: Entity) {
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 3);
}

fn drag(app: &mut App, e: Entity, distance: Vec2) {
    let world = app.world_mut();
    world.trigger(PointerDragStart {
        entity: e,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: e,
        pointer: pointer(),
        button: PointerButton::Primary,
        distance,
        delta: distance,
    });
    frames(app, 3);
}

/// The slot header's Pop out puts the panel in a window over the views,
/// with its header (grip, title, Dock, Close), its body and its footer;
/// it shows, and its activity button wears the ring, not the active look.
/// The header drags the window, an edge resizes it, a press raises it;
/// Dock returns it and Close drops it.
#[test]
fn a_window_hosts_its_panel() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: PLOT,
        slot: Slot::Primary,
    });
    frames(&mut app, 3);
    // The primary header's Pop out.
    let slot_header = {
        let world = app.world_mut();
        world
            .query::<(Entity, &PanelHeader)>()
            .iter(world)
            .find(|(_, h)| h.panel == PLOT)
            .unwrap()
            .0
    };
    let pop = button(&mut app, slot_header, SlotAction::PopOut);
    activate(&mut app, pop);
    let l = layout(&app);
    assert!(l.is_popped(PLOT) && l.primary != Some(PLOT));
    assert!(l.area.x > 0.0, "the area's size is known");
    let w = window(&mut app, PLOT).expect("a window");
    let (min, size) = rect(&app, w);
    assert_eq!(size, POPOUT_SIZE);
    assert!(app.world().resource::<ShownPanels>().is_shown(PLOT));
    let h = header(&mut app, PLOT);
    assert!(app.world().get::<HeaderDrag>(h).is_some());
    let dock = button(&mut app, h, WindowAction::Dock);
    button(&mut app, h, WindowAction::Close);
    assert!(all_text(&mut app).contains("Pop out"), "the footer");
    let body = {
        let world = app.world_mut();
        world
            .query::<(&PanelBody, &ChildOf)>()
            .iter(world)
            .find(|(b, _)| b.0 == PLOT)
            .map(|(_, p)| p.parent())
    };
    assert!(app.world().get::<PopOutHost>(body.unwrap()).is_some());
    let plot_button = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ActivityFor)>()
            .iter(world)
            .find(|(_, a)| a.0 == PLOT)
            .unwrap()
            .0
    };
    assert!(app.world().get::<Active>(plot_button).is_none());
    let ring = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &ChildOf), With<PopRing>>()
            .iter(world)
            .find(|(_, p)| p.parent() == plot_button)
            .unwrap()
            .0
    };
    assert!(displayed(&app, ring));

    // The header moves the window; an edge resizes it.
    drag(&mut app, h, Vec2::new(100.0, 40.0));
    let moved = layout(&app).popouts[0].rect;
    assert_eq!(moved.min, l.popouts[0].rect.min + Vec2::new(100.0, 40.0));
    let right = {
        let world = app.world_mut();
        world
            .query::<(Entity, &EdgeDrag, &ChildOf)>()
            .iter(world)
            .find(|(_, e, p)| {
                e.edges
                    == Edges {
                        right: true,
                        ..default()
                    }
                    && p.parent() == w
            })
            .unwrap()
            .0
    };
    drag(&mut app, right, Vec2::new(60.0, 0.0));
    assert_eq!(layout(&app).popouts[0].rect.width(), POPOUT_SIZE.x + 60.0);
    assert_eq!(rect(&app, w).1.x, POPOUT_SIZE.x + 60.0);
    assert!(rect(&app, w).0.x > min.x);

    // A second window; a press in the first brings it back on top.
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: TOPICS,
        slot: Slot::Primary,
    });
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::PopOut(TOPICS));
    frames(&mut app, 3);
    assert_eq!(layout(&app).popouts.last().unwrap().panel, TOPICS);
    app.world_mut().trigger(PointerPress {
        entity: w,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
        count: 1,
    });
    frames(&mut app, 3);
    assert_eq!(layout(&app).popouts.last().unwrap().panel, PLOT);
    let z = |app: &App, e| app.world().get::<ZIndex>(e).map(|z| z.0);
    let topics = window(&mut app, TOPICS).unwrap();
    assert!(z(&app, w) > z(&app, topics));

    // Dock: back in the primary slot it came from; the window goes.
    activate(&mut app, dock);
    let l = layout(&app);
    assert_eq!(l.primary, Some(PLOT));
    assert!(window(&mut app, PLOT).is_none());
    assert!(app.world().get::<Active>(plot_button).is_some());
    assert!(!displayed(&app, ring));
    let th = header(&mut app, TOPICS);
    let close = button(&mut app, th, WindowAction::Close);
    activate(&mut app, close);
    assert!(window(&mut app, TOPICS).is_none());
    assert!(!app.world().resource::<ShownPanels>().is_shown(TOPICS));
}
