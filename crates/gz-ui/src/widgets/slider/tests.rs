//! Slider steps with the modifiers, and the fill, thumb and readout.

use bevy::prelude::*;
use bevy::ui_widgets::{SliderStep, SliderValue};

use super::{format_value, step_for, BaseStep, SliderPart, SliderReadout};
use crate::test_support::{frames, ui_app};
use crate::theme::{Theme, ThemeMode};
use crate::widgets::*;

#[test]
fn slider_steps_and_visuals() {
    assert_eq!(step_for(0.1, false, false), 0.1);
    assert!((step_for(0.1, true, false) - 1.0).abs() < 1e-6);
    assert!((step_for(0.1, false, true) - 0.01).abs() < 1e-6);
    assert_eq!(step_for(0.1, true, true), 0.1);
    assert_eq!(format_value(0.6, 2), "0.60");
    assert_eq!(format_value(3.0, 0), "3");
    assert_eq!(format_value(3.0, -1), "3");

    let mut app = ui_app(ThemeMode::Light);
    let mut spec = SliderProps::new("Linear", 0.0, 2.0, 0.5);
    spec.step = 0.1;
    let row = app
        .world_mut()
        .spawn(slider_row(&Theme::default(), spec))
        .id();
    frames(&mut app, 2);
    let w = app.world_mut();
    let slider = w
        .query_filtered::<Entity, With<BaseStep>>()
        .single(w)
        .unwrap();
    // Shift held: ×10 before the arrow key reaches the slider.
    w.resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    frames(&mut app, 1);
    assert!((app.world().get::<SliderStep>(slider).unwrap().0 - 1.0).abs() < 1e-6);
    let w = app.world_mut();
    w.resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    w.resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    frames(&mut app, 1);
    assert!((app.world().get::<SliderStep>(slider).unwrap().0 - 0.01).abs() < 1e-6);
    // A new value moves the fill and thumb and updates the read-out.
    app.world_mut().entity_mut(slider).insert(SliderValue(1.5));
    frames(&mut app, 1);
    let w = app.world_mut();
    let mut parts = w.query::<(&SliderPart, &Node)>();
    for (part, node) in parts.iter(w) {
        match part {
            SliderPart::Fill => assert_eq!(node.width, Val::Percent(75.0)),
            SliderPart::Thumb => assert_eq!(node.left, Val::Percent(75.0)),
        }
    }
    let mut readout = w.query_filtered::<&Text, With<SliderReadout>>();
    assert_eq!(readout.single(w).unwrap().0, "1.50");
    assert!(w.get::<Children>(row).is_some());
    assert_eq!(
        w.get::<Node>(row).unwrap().height,
        px(Theme::default().s.input_h)
    );
    // Up and Down step too (with the step the modifiers set: Alt, 0.01).
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::window::PrimaryWindow;
    w.resource_mut::<InputFocus>()
        .set(slider, FocusCause::Navigated);
    let window = w
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap();
    for (code, key, want) in [
        (KeyCode::ArrowUp, Key::ArrowUp, 1.51),
        (KeyCode::ArrowDown, Key::ArrowDown, 1.5),
        (KeyCode::ArrowDown, Key::ArrowDown, 1.49),
        (KeyCode::KeyA, Key::Character("a".into()), 1.49),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        frames(&mut app, 2);
        let v = app.world().get::<SliderValue>(slider).unwrap().0;
        assert!((v - want).abs() < 1e-5, "{code:?}: {v}");
    }
}
