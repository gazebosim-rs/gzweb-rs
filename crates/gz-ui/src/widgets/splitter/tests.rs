//! Splitter maths, drags at any UI scale, and the keyboard.

use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;

use super::{SplitAxis, SplitterDrag, SplitterLine};
use crate::test_support::{frames, ui_app};
use crate::theme::{ColorRole, Fill, Theme, ThemeMode};
use crate::widgets::*;

#[test]
fn splitter_drag_maths_and_resizing() {
    let pane = Entity::PLACEHOLDER;
    let s = Splitter {
        axis: SplitAxis::Columns,
        pane,
        sign: 1.0,
        min: 240.0,
        max: 480.0,
    };
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

    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerDrag, PointerDragEnd, PointerDragStart};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    let mut app = ui_app(ThemeMode::Light);
    let row = app
        .world_mut()
        .spawn(Node {
            width: px(800),
            height: px(200),
            ..default()
        })
        .id();
    let pane = app
        .world_mut()
        .spawn((
            Node {
                width: px(300),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let bar = app
        .world_mut()
        .spawn((
            splitter(&Theme::default(), Splitter { pane, ..s }, "Resize"),
            ChildOf(row),
        ))
        .id();
    frames(&mut app, 2);
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    let w = app.world_mut();
    w.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    w.flush();
    assert_eq!(
        w.get::<SplitterDrag>(bar),
        Some(&SplitterDrag { start: 300.0 })
    );
    w.trigger(PointerDrag {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        distance: Vec2::new(40.0, 0.0),
        delta: Vec2::new(40.0, 0.0),
    });
    w.flush();
    assert_eq!(w.get::<Node>(pane).unwrap().width, px(340));
    frames(&mut app, 1);
    let line = app.world().get::<Children>(bar).unwrap()[0];
    assert!(app.world().get::<SplitterLine>(line).is_some());
    assert_eq!(app.world().get::<Fill>(line).unwrap().0, ColorRole::Accent);
    let w = app.world_mut();
    w.trigger(PointerDragEnd {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    w.flush();
    frames(&mut app, 1);
    assert!(app.world().get::<SplitterDrag>(bar).is_none());
    assert_eq!(app.world().get::<Fill>(line).unwrap().0, ColorRole::Clear);
    // Rows set heights.
    let rows_bar = app
        .world_mut()
        .spawn(splitter(
            &Theme::default(),
            Splitter { pane, ..rows },
            "Resize",
        ))
        .id();
    frames(&mut app, 1);
    let w = app.world_mut();
    w.entity_mut(rows_bar).insert(SplitterDrag { start: 100.0 });
    w.trigger(PointerDrag {
        entity: rows_bar,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        ),
        button: PointerButton::Primary,
        distance: Vec2::new(0.0, 150.0),
        delta: Vec2::ZERO,
    });
    w.flush();
    assert_eq!(w.get::<Node>(pane).unwrap().height, px(250));
}

/// A drag of the pointer in window pixels at UI scale 1.5 moves the pane
/// by as much on screen (2/3 as many UI pixels).
#[test]
fn splitter_drags_follow_the_pointer_at_any_ui_scale() {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerDrag, PointerDragStart};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    let mut app = ui_app(ThemeMode::Light);
    app.world_mut().resource_mut::<Theme>().scale = 1.5;
    let row = app
        .world_mut()
        .spawn(Node {
            width: px(800),
            height: px(200),
            ..default()
        })
        .id();
    let pane = app
        .world_mut()
        .spawn((
            Node {
                width: px(300),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let s = Splitter {
        axis: SplitAxis::Columns,
        pane,
        sign: 1.0,
        min: 100.0,
        max: 700.0,
    };
    let bar = app
        .world_mut()
        .spawn((splitter(&Theme::default(), s, "Resize"), ChildOf(row)))
        .id();
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<UiScale>().0, 1.5);
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    );
    let w = app.world_mut();
    w.trigger(PointerDragStart {
        entity: bar,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    w.flush();
    assert_eq!(
        w.get::<SplitterDrag>(bar),
        Some(&SplitterDrag { start: 300.0 })
    );
    w.trigger(PointerDrag {
        entity: bar,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::new(90.0, 0.0),
        delta: Vec2::new(90.0, 0.0),
    });
    w.flush();
    assert_eq!(w.get::<Node>(pane).unwrap().width, px(360));
}

#[test]
fn splitters_move_with_the_keyboard() {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::window::PrimaryWindow;
    let mut app = ui_app(ThemeMode::Light);
    let row = app
        .world_mut()
        .spawn(Node {
            width: px(800),
            height: px(200),
            ..default()
        })
        .id();
    let pane = app
        .world_mut()
        .spawn((
            Node {
                width: px(300),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let s = Splitter {
        axis: SplitAxis::Columns,
        pane,
        sign: -1.0,
        min: 250.0,
        max: 400.0,
    };
    let bar = app
        .world_mut()
        .spawn((
            splitter(&Theme::default(), s, "Resize the inspector"),
            ChildOf(row),
        ))
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
        app.world().get::<Node>(pane).unwrap().width
    };
    // The pane is after the splitter: Left widens it.
    assert_eq!(press(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft), px(308));
    assert_eq!(
        press(&mut app, KeyCode::ArrowRight, Key::ArrowRight),
        px(300)
    );
    // Off-axis keys do nothing.
    assert_eq!(press(&mut app, KeyCode::ArrowUp, Key::ArrowUp), px(300));
    assert_eq!(press(&mut app, KeyCode::Home, Key::Home), px(250));
    assert_eq!(press(&mut app, KeyCode::End, Key::End), px(400));
}
