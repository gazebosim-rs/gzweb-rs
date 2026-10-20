//! The sidebar's slots: the header buttons and the row splitter.

use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::shell::layout::MIN_SLOT_H;
use crate::shell::{Panel, Slot, UiCommand};
use crate::test_support::frames;
use crate::test_support::shell::*;

#[test]
fn slot_header_buttons_swap_and_close() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: Panel::Plot,
        slot: Slot::Secondary,
    });
    frames(&mut app, 2);
    let world = app.world_mut();
    let actions: Vec<(Entity, crate::shell::sidebar::SlotAction)> = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .map(|(e, a)| (e, *a))
        .collect();
    use crate::shell::sidebar::SlotAction::*;
    assert_eq!(actions.iter().filter(|a| a.1 == Swap).count(), 2);
    let first = |a| actions.iter().find(|x| x.1 == a).unwrap().0;
    app.world_mut().trigger(Activate {
        entity: first(Swap),
    });
    frames(&mut app, 2);
    let l = layout(&app);
    assert_eq!(
        (l.primary, l.secondary),
        (Some(Panel::Plot), Some(Panel::Scene))
    );
    let world = app.world_mut();
    let close = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Close)
        .unwrap()
        .0;
    world.trigger(Activate { entity: close });
    frames(&mut app, 2);
    assert_eq!(layout(&app).secondary, None);
    let world = app.world_mut();
    let collapse = world
        .query::<(Entity, &crate::shell::sidebar::SlotAction)>()
        .iter(world)
        .find(|(_, a)| **a == Collapse)
        .unwrap()
        .0;
    world.trigger(Activate { entity: collapse });
    frames(&mut app, 2);
    assert!(!layout(&app).sidebar_open);
}

#[test]
fn the_row_splitter_resizes_the_slots_within_their_minimum() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    app.world_mut().write_message(UiCommand::OpenPanel {
        panel: Panel::Plot,
        slot: Slot::Secondary,
    });
    frames(&mut app, 3);
    let bar = find::<crate::shell::sidebar::slot_splitter::SlotSplitter>(&mut app);
    assert!(displayed(&app, bar));
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    let world = app.world_mut();
    world.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    world.flush();
    world.trigger(PointerDrag {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::new(0.0, -1000.0),
        delta: Vec2::new(0.0, -1000.0),
    });
    world.trigger(PointerDragEnd {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    frames(&mut app, 2);
    let split = layout(&app).split;
    assert!((split - MIN_SLOT_H / (828.0 - 6.0)).abs() < 1e-4, "{split}");
    assert!(app
        .world()
        .get::<crate::shell::sidebar::slot_splitter::SlotDrag>(bar)
        .is_none());
}
