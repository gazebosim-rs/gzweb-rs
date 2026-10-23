//! Splitter maths, drags at any UI scale, the keyboard and double-clicks:
//! the splitter asks, and a test owner records what it asked for.

use std::time::Duration;

use bevy::a11y::AccessibilityNode;
use bevy::camera::NormalizedRenderTarget;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;

use super::{SplitAxis, SplitterDrag, SplitterLine};
use crate::test_support::{frames, ui_app};
use crate::theme::{ColorRole, Fill, Theme, ThemeMode};
use crate::widgets::*;

/// What the splitters asked for: sizes, and how many resets.
#[derive(Resource, Default)]
struct Asked(Vec<f32>, u32);

fn owner_app() -> App {
    let mut app = ui_app(ThemeMode::Light);
    app.init_resource::<Asked>()
        .add_observer(|ev: On<SplitterMoved>, mut asked: ResMut<Asked>| {
            asked.0.push(ev.size);
        })
        .add_observer(|_: On<SplitterReset>, mut asked: ResMut<Asked>| {
            asked.1 += 1;
        });
    app
}

fn pointer() -> Pointer {
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    )
}

fn drag_start(w: &mut World, bar: Entity) {
    w.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    w.flush();
}

fn drag(w: &mut World, bar: Entity, distance: Vec2) {
    w.trigger(PointerDrag {
        entity: bar,
        pointer: pointer(),
        button: PointerButton::Primary,
        distance,
        delta: distance,
    });
    w.flush();
}

fn drag_end(w: &mut World, bar: Entity) {
    w.trigger(PointerDragEnd {
        entity: bar,
        pointer: pointer(),
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    w.flush();
}

fn click(w: &mut World, bar: Entity, button: PointerButton, count: u8) {
    w.trigger(PointerClick {
        entity: bar,
        pointer: pointer(),
        button,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: Duration::from_millis(80),
        count,
    });
    w.flush();
}

fn asked(app: &mut App) -> Vec<f32> {
    std::mem::take(&mut app.world_mut().resource_mut::<Asked>().0)
}

#[test]
fn splitter_drag_maths_and_ranges() {
    let s = Splitter::new(SplitAxis::Columns, 1.0).with_range(300.0, 240.0, 480.0);
    assert_eq!(s.size_after(300.0, Vec2::new(50.0, 99.0)), 350.0);
    assert_eq!(s.size_after(300.0, Vec2::new(-100.0, 0.0)), 240.0);
    assert_eq!(s.size_after(300.0, Vec2::new(500.0, 0.0)), 480.0);
    let right = Splitter { sign: -1.0, ..s };
    assert_eq!(right.size_after(300.0, Vec2::new(50.0, 0.0)), 250.0);
    let rows = Splitter {
        axis: SplitAxis::Rows,
        ..s
    };
    assert_eq!(rows.size_after(300.0, Vec2::new(99.0, 20.0)), 320.0);
    // A range too small for its minimum is just the minimum.
    let tight = s.with_range(240.0, 240.0, 100.0);
    assert_eq!((tight.min, tight.max), (240.0, 240.0));
    assert_eq!(tight.size_after(240.0, Vec2::new(50.0, 0.0)), 240.0);
}

/// A drag asks for sizes from where it started, whatever the owner did
/// meanwhile; the line is `accent` while it drags.
#[test]
fn a_drag_asks_its_owner_for_sizes() {
    let mut app = owner_app();
    let s = Splitter::new(SplitAxis::Columns, 1.0).with_range(300.0, 240.0, 480.0);
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize"))
        .id();
    frames(&mut app, 2);
    let w = app.world_mut();
    drag_start(w, bar);
    assert_eq!(
        w.get::<SplitterDrag>(bar),
        Some(&SplitterDrag { start: 300.0 })
    );
    drag(w, bar, Vec2::new(40.0, 7.0));
    // The owner hasn't applied it: the next move still counts from 300,
    // and is asked for even though it is the size the splitter holds.
    drag(w, bar, Vec2::ZERO);
    drag(w, bar, Vec2::new(500.0, 0.0));
    assert_eq!(asked(&mut app), vec![340.0, 300.0, 480.0]);
    // Nothing applied it: the splitter is as its owner left it.
    assert_eq!(app.world().get::<Splitter>(bar).unwrap().size, 300.0);
    frames(&mut app, 1);
    let line = app.world().get::<Children>(bar).unwrap()[0];
    assert!(app.world().get::<SplitterLine>(line).is_some());
    assert_eq!(app.world().get::<Fill>(line).unwrap().0, ColorRole::Accent);
    drag_end(app.world_mut(), bar);
    frames(&mut app, 1);
    assert!(app.world().get::<SplitterDrag>(bar).is_none());
    assert_eq!(app.world().get::<Fill>(line).unwrap().0, ColorRole::Clear);
    // Rows drag along y.
    let rows = app
        .world_mut()
        .spawn(splitter(
            &Theme::default(),
            Splitter::new(SplitAxis::Rows, 1.0).with_range(100.0, 0.0, 400.0),
            "Resize",
        ))
        .id();
    frames(&mut app, 1);
    let w = app.world_mut();
    drag_start(w, rows);
    drag(w, rows, Vec2::new(90.0, 150.0));
    assert_eq!(asked(&mut app), vec![250.0]);
}

/// A drag of the pointer in window pixels at UI scale 1.5 moves the pane
/// by as much on screen (2/3 as many UI pixels).
#[test]
fn splitter_drags_follow_the_pointer_at_any_ui_scale() {
    let mut app = owner_app();
    app.world_mut().resource_mut::<Theme>().scale = 1.5;
    let s = Splitter::new(SplitAxis::Columns, 1.0).with_range(300.0, 100.0, 700.0);
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize"))
        .id();
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<UiScale>().0, 1.5);
    let w = app.world_mut();
    drag_start(w, bar);
    drag(w, bar, Vec2::new(90.0, 0.0));
    assert_eq!(asked(&mut app), vec![360.0]);
}

#[test]
fn splitters_move_with_the_keyboard() {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::window::PrimaryWindow;
    let mut app = owner_app();
    let s = Splitter::new(SplitAxis::Columns, -1.0).with_range(300.0, 250.0, 400.0);
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize the inspector"))
        .id();
    frames(&mut app, 2);
    let access = app.world().get::<AccessibilityNode>(bar).unwrap();
    assert_eq!(access.label(), Some("Resize the inspector"));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(bar, FocusCause::Navigated);
    let window = {
        let w = app.world_mut();
        w.query_filtered::<Entity, With<PrimaryWindow>>()
            .single(w)
            .unwrap()
    };
    let press = |app: &mut App, key: KeyCode, logical: Key| {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: logical,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        frames(app, 2);
        asked(app)
    };
    // The pane is after the splitter: Left widens it. Nothing applies
    // what it asks, and the next key steps from it all the same.
    assert_eq!(press(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft), [308.0]);
    assert_eq!(
        press(&mut app, KeyCode::ArrowRight, Key::ArrowRight),
        [300.0]
    );
    // Off-axis keys ask nothing.
    assert!(press(&mut app, KeyCode::ArrowUp, Key::ArrowUp).is_empty());
    assert_eq!(press(&mut app, KeyCode::Home, Key::Home), [250.0]);
    assert_eq!(press(&mut app, KeyCode::End, Key::End), [400.0]);
    // Keys step from the size the owner keeps.
    app.world_mut().get_mut::<Splitter>(bar).unwrap().size = 300.0;
    // Two presses in one frame step twice.
    for _ in 0..2 {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::ArrowRight,
            logical_key: Key::ArrowRight,
            state: ButtonState::Pressed,
            text: None,
            repeat: true,
            window,
        });
    }
    frames(&mut app, 2);
    assert_eq!(asked(&mut app), [292.0, 284.0]);
    // A chord is the shortcuts' (Ctrl+→ steps the sim).
    let ctrl = |app: &mut App, state| {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::ControlLeft,
            logical_key: Key::Control,
            state,
            text: None,
            repeat: false,
            window,
        });
    };
    ctrl(&mut app, ButtonState::Pressed);
    frames(&mut app, 1);
    assert!(press(&mut app, KeyCode::ArrowRight, Key::ArrowRight).is_empty());
    ctrl(&mut app, ButtonState::Released);
    frames(&mut app, 1);
    assert_eq!(
        press(&mut app, KeyCode::ArrowRight, Key::ArrowRight),
        [276.0]
    );
}

/// A primary double-click asks for a reset, unless a click of it ended a
/// drag (bevy_picking's click count runs on across drags); a right-button
/// one doesn't.
#[test]
fn a_primary_double_click_without_a_drag_asks_for_a_reset() {
    let mut app = owner_app();
    let s = Splitter::new(SplitAxis::Columns, 1.0).with_range(300.0, 100.0, 700.0);
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize"))
        .id();
    frames(&mut app, 2);
    let resets = |app: &App| app.world().resource::<Asked>().1;
    let w = app.world_mut();
    // Drag and release (the release's click comes before the drag end),
    // then grab again at once: the second click counts 2.
    drag_start(w, bar);
    click(w, bar, PointerButton::Primary, 1);
    drag_end(w, bar);
    drag_start(w, bar);
    click(w, bar, PointerButton::Primary, 2);
    drag_end(w, bar);
    // A plain click right after a drag isn't one either.
    click(w, bar, PointerButton::Primary, 3);
    click(w, bar, PointerButton::Secondary, 1);
    click(w, bar, PointerButton::Secondary, 2);
    assert_eq!(resets(&app), 0);
    let w = app.world_mut();
    click(w, bar, PointerButton::Primary, 1);
    click(w, bar, PointerButton::Primary, 2);
    assert_eq!(resets(&app), 1);
}

/// What an owner spawns: the splitter's behaviour on its own node.
#[test]
fn a_splitter_control_is_a_tab_stop_with_its_cursor() {
    use bevy::input_focus::tab_navigation::TabIndex;
    use bevy::picking::cursor::EntityCursor;
    use bevy::window::SystemCursorIcon;
    let mut app = owner_app();
    let bar = app
        .world_mut()
        .spawn((
            Node::default(),
            splitter_control(Splitter::new(SplitAxis::Rows, 1.0), "Resize slots"),
        ))
        .id();
    frames(&mut app, 1);
    let w = app.world();
    assert_eq!(w.get::<TabIndex>(bar), Some(&TabIndex(0)));
    assert_eq!(
        w.get::<EntityCursor>(bar),
        Some(&EntityCursor::System(SystemCursorIcon::RowResize))
    );
    assert_eq!(
        w.get::<AccessibilityNode>(bar).unwrap().label(),
        Some("Resize slots")
    );
}
