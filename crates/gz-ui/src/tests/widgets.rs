//! Tooltip timing, slider steps, splitter maths, toggles, tree rows,
//! chips and badges.

use bevy::a11y::AccessibilityNode;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, Pressed};
use bevy::ui_widgets::{Activate, SliderStep, SliderValue};

use super::app::{descendants, frames, ui_app};
use crate::fonts::Type;
use crate::icons::{Icon, IconId};
use crate::theme::{ColorRole, Fill, Ink, Opacity, Stroke, Theme, ThemeMode};
use crate::visuals::Active;
use crate::widgets::slider::{format_value, step_for, BaseStep, SliderPart, SliderReadout};
use crate::widgets::splitter::{SplitAxis, SplitterDrag, SplitterLine};
use crate::widgets::toggle::{knob_left, TogglePart};
use crate::widgets::tooltip::{
    TooltipArrow, TooltipCard, TooltipStep, TooltipTimer, CHAIN_SECS, DELAY_SECS,
};
use crate::widgets::tree_row::{name_type, TreeRowPart};
use crate::widgets::*;

#[test]
fn tooltip_waits_500ms_then_chains_within_300ms() {
    let a = Entity::from_raw_u32(1).unwrap();
    let b = Entity::from_raw_u32(2).unwrap();
    let mut t = TooltipTimer::default();
    assert_eq!(t.step(Some(a), false, 0.0), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, 0.49), TooltipStep::default());
    assert_eq!(
        t.step(Some(a), false, DELAY_SECS),
        TooltipStep {
            hide: None,
            show: Some(a)
        }
    );
    // Shown once.
    assert_eq!(t.step(Some(a), false, 0.6), TooltipStep::default());
    // Moving to b within 300 ms: b shows at once.
    assert_eq!(
        t.step(Some(b), false, 0.7),
        TooltipStep {
            hide: Some(a),
            show: Some(b)
        }
    );
    // Leaving, then coming back after more than 300 ms: the full delay.
    assert_eq!(
        t.step(None, false, 0.8),
        TooltipStep {
            hide: Some(b),
            show: None
        }
    );
    let back = 0.8 + CHAIN_SECS + 0.01;
    assert_eq!(t.step(Some(a), false, back), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, back + 0.4), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, back + DELAY_SECS).show, Some(a));
    // A press hides it and keeps it hidden until the pointer leaves.
    assert_eq!(t.step(Some(a), true, 3.0).hide, Some(a));
    assert_eq!(t.step(Some(a), false, 4.0), TooltipStep::default());
    t.step(None, false, 4.1);
    t.step(Some(a), false, 4.2);
    assert_eq!(t.step(Some(a), false, 4.2 + DELAY_SECS).show, Some(a));
}

#[test]
fn tooltip_text() {
    let t = Tooltip::new("Navigate").shortcut("Esc");
    assert_eq!(t.accessible_name(), "Navigate");
    assert_eq!(t.second_line().as_deref(), Some("Esc"));
    let d = Tooltip::new("Workspace: Build")
        .detail("Click to switch")
        .shortcut_opt(Some("Alt+1–4".into()))
        .side(TooltipSide::Above);
    assert_eq!(
        d.second_line().as_deref(),
        Some("Click to switch · Alt+1–4")
    );
    assert_eq!(d.side, TooltipSide::Above);
    assert_eq!(
        Tooltip::new("x").detail("y").second_line().as_deref(),
        Some("y")
    );
    assert_eq!(Tooltip::new("x").second_line(), None);
}

#[test]
fn tooltip_cards_show_on_hover_and_follow_the_theme() {
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let root = app
        .world_mut()
        .spawn(Node {
            padding: UiRect::all(px(100)),
            ..default()
        })
        .id();
    let below = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("Settings").shortcut("S"),
            ),
            ChildOf(root),
        ))
        .id();
    let left = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::CamPan,
                IconButtonSize::Rail,
                Tooltip::new("Pan").side(TooltipSide::Left),
            ),
            ChildOf(root),
        ))
        .id();
    frames(&mut app, 2);
    let card_of = |app: &mut App, trigger: Entity| {
        let w = app.world_mut();
        w.query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
            .iter(w)
            .find(|(_, p)| p.parent() == trigger)
            .map(|(e, _)| e)
    };
    // Hovered and held past the delay: a card under the trigger.
    app.world_mut().entity_mut(below).insert(Hovered(true));
    let timer = |app: &mut App| *app.world().resource::<TooltipTimer>();
    frames(&mut app, 1);
    assert!(card_of(&mut app, below).is_none());
    let mut tm = timer(&mut app);
    tm.target = tm.target.map(|(e, _)| (e, -1.0));
    app.insert_resource(tm);
    frames(&mut app, 3);
    let card = card_of(&mut app, below).expect("a tooltip");
    let w = app.world();
    assert_eq!(w.get::<Fill>(card).unwrap().0, ColorRole::Tooltip);
    // Light: no visible border.
    assert_eq!(w.get::<Stroke>(card).unwrap().0, ColorRole::Tooltip);
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    // Below the trigger: the arrow sits on top.
    assert_eq!(w.get::<Node>(arrow).unwrap().top, px(-4));
    // The shortcut is a Geist Mono span on the second line (D-26).
    let lines: Vec<Entity> = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .filter(|c| w.get::<Text>(*c).is_some())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(w.get::<Text>(lines[1]).unwrap().0, "");
    let keys = w.get::<Children>(lines[1]).unwrap()[0];
    assert_eq!(w.get::<TextSpan>(keys).unwrap().0, "S");
    assert_eq!(
        w.get::<Type>(keys).unwrap().0.font,
        crate::theme::FontFamily::Mono
    );
    // Dark: a 1px line.
    app.world_mut().resource_mut::<Theme>().toggle();
    frames(&mut app, 1);
    assert_eq!(app.world().get::<Stroke>(card).unwrap().0, ColorRole::Line);
    // Pressing hides it.
    app.world_mut().entity_mut(below).insert(Pressed);
    frames(&mut app, 2);
    assert!(card_of(&mut app, below).is_none());

    // Moving to the rail button: shows at once, to the left.
    app.world_mut()
        .entity_mut(below)
        .insert(Hovered(false))
        .remove::<Pressed>();
    app.world_mut().entity_mut(left).insert(Hovered(true));
    frames(&mut app, 3);
    let card = card_of(&mut app, left).expect("chained tooltip");
    let w = app.world();
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    assert_eq!(w.get::<Node>(arrow).unwrap().right, px(-4));
}

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
    let mut spec = SliderSpec::new("Linear", 0.0, 2.0, 0.5);
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

#[test]
fn toggles_follow_checked() {
    let t = Theme::default();
    assert_eq!(knob_left(&t, false), 2.0);
    assert_eq!(knob_left(&t, true), 16.0);
    let mut app = ui_app(ThemeMode::Light);
    let on = app.world_mut().spawn(toggle_row(&t, "Gravity", true)).id();
    let off = app.world_mut().spawn(toggle_row(&t, "Static", false)).id();
    frames(&mut app, 2);
    let parts = |app: &mut App, row: Entity| {
        let w = app.world_mut();
        let mut q = w.query::<(&TogglePart, &Fill, &Node)>();
        let kids = descendants(w, row);
        kids.iter()
            .filter_map(|e| q.get(w, *e).ok().map(|(p, f, n)| (*p, f.0, n.left)))
            .collect::<Vec<_>>()
    };
    let _ = &parts;
    let on_parts = parts(&mut app, on);
    assert!(on_parts.contains(&(TogglePart::Track, ColorRole::Accent, Val::Auto)));
    assert!(on_parts.contains(&(TogglePart::Knob, ColorRole::ToggleKnobOn, px(16))));
    let off_parts = parts(&mut app, off);
    assert!(off_parts.contains(&(TogglePart::Track, ColorRole::ToggleTrackOff, Val::Auto)));
    assert!(off_parts.contains(&(TogglePart::Knob, ColorRole::ToggleKnobOff, px(2))));
    // Clicking the row (the headless checkbox) flips it.
    app.world_mut().entity_mut(off).insert(Checked);
    frames(&mut app, 1);
    assert!(parts(&mut app, off).contains(&(TogglePart::Knob, ColorRole::ToggleKnobOn, px(16))));
}

fn part(app: &mut App, row: Entity, which: fn(&TreeRowPart) -> bool) -> Entity {
    let w = app.world_mut();
    let kids = descendants(w, row);
    kids.into_iter()
        .find(|e| w.get::<TreeRowPart>(*e).is_some_and(which))
        .unwrap()
}

#[test]
fn tree_rows_show_selection_and_visibility() {
    let t = Theme::default();
    let mut app = ui_app(ThemeMode::Light);
    let spec = TreeRowSpec {
        depth: 2,
        kind: EntityKind::Model,
        name: "amr_01".into(),
        expanded: Some(false),
        hidden: false,
    };
    let row = app.world_mut().spawn(tree_row(&t, spec)).id();
    frames(&mut app, 2);
    let badge = part(&mut app, row, |p| matches!(p, TreeRowPart::Badge));
    let letter = part(&mut app, row, |p| matches!(p, TreeRowPart::Letter));
    let name = part(&mut app, row, |p| matches!(p, TreeRowPart::Name));
    let eye = part(&mut app, row, |p| matches!(p, TreeRowPart::Eye));
    let glyph = app.world().get::<Children>(eye).unwrap()[0];
    let w = app.world();
    assert_eq!(w.get::<Text>(letter).unwrap().0, "M");
    assert_eq!(w.get::<Ink>(letter).unwrap().0, ColorRole::EntityModel);
    assert_eq!(w.get::<Fill>(badge).unwrap().0, ColorRole::EntityBadgeBg);
    // Visible and not hovered: no eye.
    assert_eq!(w.get::<Ink>(glyph).unwrap().0, ColorRole::Clear);
    // Selected: inverted badge, weight 500, accent_soft fill.
    app.world_mut()
        .entity_mut(row)
        .insert((Active, Hovered(true)));
    frames(&mut app, 2);
    let w = app.world();
    assert_eq!(w.get::<Fill>(badge).unwrap().0, ColorRole::Accent);
    assert_eq!(w.get::<Ink>(letter).unwrap().0, ColorRole::Surface);
    assert_eq!(w.get::<Type>(name).unwrap().0, name_type(true));
    assert_eq!(w.get::<Fill>(row).unwrap().0, ColorRole::AccentSoft);
    assert_eq!(w.get::<Ink>(glyph).unwrap().0, ColorRole::Text3);
    // Clicking the eye hides the row: eye_off, 50% opacity.
    app.world_mut().trigger(Activate { entity: eye });
    frames(&mut app, 2);
    let w = app.world();
    assert!(w.get::<TreeRowSpec>(row).unwrap().hidden);
    assert_eq!(w.get::<Icon>(glyph).unwrap().id, IconId::EyeOff);
    assert_eq!(w.get::<Opacity>(name).unwrap().0, 0.5);
    assert_eq!(name_type(false).weight, 400);
    for (kind, letter, role) in [
        (EntityKind::World, "W", ColorRole::EntityWorld),
        (EntityKind::Light, "L", ColorRole::EntityLight),
        (EntityKind::Link, "K", ColorRole::EntityLink),
        (EntityKind::Joint, "J", ColorRole::EntityJoint),
        (EntityKind::Sensor, "S", ColorRole::EntitySensor),
    ] {
        assert_eq!((kind.letter(), kind.role()), (letter, role));
    }
}

#[test]
fn chips_badges_and_buttons() {
    assert_eq!(BadgeKind::Count.fill(), ColorRole::Text2);
    assert_eq!(BadgeKind::Warning.fill(), ColorRole::WarnBadge);
    assert_eq!(BadgeKind::Error.fill(), ColorRole::Danger);
    assert_eq!(
        StatusKind::Warning.roles(),
        (ColorRole::WarnSoft, ColorRole::WarnText)
    );
    assert_eq!(
        StatusKind::Error.roles(),
        (ColorRole::DangerSoft, ColorRole::Danger)
    );
    assert_eq!(
        StatusKind::Info.roles(),
        (ColorRole::Clear, ColorRole::Text2)
    );
    let t = Theme::default();
    assert_eq!(IconButtonSize::Regular.metrics(&t), (32.0, 17.0, 7.0));
    assert_eq!(IconButtonSize::Small.metrics(&t), (28.0, 15.0, 7.0));
    assert_eq!(IconButtonSize::Rail.metrics(&t), (32.0, 18.0, 8.0));

    let mut app = ui_app(ThemeMode::Light);
    let w = app.world_mut();
    let chip = w.spawn(filter_chip("All", true)).id();
    let status = w
        .spawn(status_chip(
            &t,
            StatusKind::Warning,
            "Mesh not found",
            Some("2 warnings".into()),
        ))
        .id();
    let info = w
        .spawn(status_chip(&t, StatusKind::Info, "Saved", None))
        .id();
    let b = w.spawn(badge(&t, BadgeKind::Warning, 12)).id();
    let destructive = w
        .spawn(button(&t, "Delete", ButtonVariant::Destructive))
        .id();
    let play = w
        .spawn(primary_button(&t, IconId::Play, Tooltip::new("Play")))
        .id();
    let tb = w
        .spawn(text_button(
            &t,
            TextButton::new("warehouse_demo")
                .leading(IconId::Folder)
                .dot(ColorRole::Warn)
                .chevron()
                .large(),
        ))
        .id();
    let group = w.spawn(segmented_group("Tools", true, true)).id();
    let tool = w
        .spawn((
            tool_button(&t, IconId::Navigate, Tooltip::new("Navigate"), true),
            ChildOf(group),
        ))
        .id();
    let plain = w.spawn(segmented_group("Transport", false, false)).id();
    let activity = w
        .spawn(activity_button(
            &t,
            IconId::ActivityConsole,
            Tooltip::new("Console"),
            Some((BadgeKind::Error, 3)),
        ))
        .id();
    frames(&mut app, 2);
    let w = app.world();
    // A selected filter chip: `text` fill, `surface` label.
    assert!(w.get::<Checked>(chip).is_some());
    assert_eq!(w.get::<Fill>(chip).unwrap().0, ColorRole::Text);
    // No `text` border on the `text` fill (it leaves a ragged seam); the
    // border keeps its width, clear.
    assert_eq!(w.get::<Stroke>(chip).unwrap().0, ColorRole::Clear);
    assert_eq!(w.get::<Node>(chip).unwrap().border, UiRect::all(px(1)));
    assert_eq!(w.get::<Fill>(status).unwrap().0, ColorRole::WarnSoft);
    assert_eq!(w.get::<Children>(status).unwrap().len(), 3);
    assert_eq!(w.get::<Children>(info).unwrap().len(), 1);
    assert_eq!(w.get::<Fill>(b).unwrap().0, ColorRole::WarnBadge);
    let label = w.get::<Children>(destructive).unwrap()[0];
    assert_eq!(w.get::<Ink>(label).unwrap().0, ColorRole::Danger);
    assert_eq!(w.get::<Fill>(play).unwrap().0, ColorRole::Primary);
    assert_eq!(w.get::<Children>(tb).unwrap().len(), 4);
    assert!(w.get::<bevy::ui_widgets::RadioGroup>(group).is_some());
    assert!(w.get::<bevy::ui_widgets::RadioGroup>(plain).is_none());
    assert!(w.get::<Checked>(tool).is_some());
    assert_eq!(w.get::<Fill>(tool).unwrap().0, ColorRole::AccentSoft);
    assert_eq!(w.get::<Children>(activity).unwrap().len(), 2);
    // Active activity button: inset accent_ring.
    app.world_mut().entity_mut(activity).insert(Active);
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<Stroke>(activity).unwrap().0,
        ColorRole::AccentRing
    );
}

/// Hover cursors are on (Bevy's `CursorIconPlugin`), and a drag holds the
/// cursor of what it started on until it ends.
#[test]
fn drags_hold_their_cursor() {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::cursor::{EntityCursor, OverrideCursor};
    use bevy::picking::events::{Pointer, PointerDragEnd, PointerDragStart};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
    let mut app = ui_app(ThemeMode::Light);
    let pane = app.world_mut().spawn(Node::default()).id();
    let s = Splitter {
        axis: SplitAxis::Rows,
        pane,
        sign: 1.0,
        min: 0.0,
        max: 100.0,
    };
    let bar = app
        .world_mut()
        .spawn(splitter(&Theme::default(), s, "Resize"))
        .id();
    let plain = app.world_mut().spawn(Node::default()).id();
    let line = app.world().get::<Children>(bar).unwrap()[0];
    frames(&mut app, 2);
    // The plugin's system gives the window a cursor.
    let w = app.world_mut();
    let window = w
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap();
    assert!(w.get::<CursorIcon>(window).is_some());
    let pointer = |id| {
        Pointer::new(
            id,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        )
    };
    let start = |w: &mut World, entity, id| {
        w.trigger(PointerDragStart {
            entity,
            pointer: pointer(id),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        });
        w.flush();
    };
    let end = |w: &mut World, entity, id| {
        w.trigger(PointerDragEnd {
            entity,
            pointer: pointer(id),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
        });
        w.flush();
    };
    let held = |w: &World| w.resource::<OverrideCursor>().0.clone();
    // Started on the splitter's line: the splitter's cursor.
    start(w, line, PointerId::Mouse);
    assert_eq!(
        held(w),
        Some(EntityCursor::System(SystemCursorIcon::RowResize))
    );
    end(w, line, PointerId::Mouse);
    assert_eq!(held(w), None);
    // Started on something without a cursor: the default one.
    start(w, plain, PointerId::Mouse);
    assert_eq!(held(w), Some(EntityCursor::default()));
    end(w, plain, PointerId::Mouse);
    // Touch has no cursor to hold.
    start(w, bar, PointerId::Touch(1));
    assert_eq!(held(w), None);
}

/// Enter in a text field submits once, however deep the field sits (the
/// key bubbles through every ancestor).
#[test]
fn text_fields_submit_once() {
    use crate::widgets::{text_field, TextSubmit};
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::window::PrimaryWindow;
    #[derive(Resource, Default)]
    struct Submits(Vec<String>);
    let mut app = ui_app(ThemeMode::Light);
    app.init_resource::<Submits>()
        .add_observer(|ev: On<TextSubmit>, mut s: ResMut<Submits>| {
            s.0.push(ev.value.clone());
        });
    let outer = app.world_mut().spawn(Node::default()).id();
    let inner = app
        .world_mut()
        .spawn((Node::default(), ChildOf(outer)))
        .id();
    let field = app
        .world_mut()
        .spawn((
            text_field(&Theme::default(), "Address", "ws://localhost:9002"),
            ChildOf(inner),
        ))
        .id();
    frames(&mut app, 2);
    let input = app.world().get::<Children>(field).unwrap()[0];
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Navigated);
    frames(&mut app, 1);
    // The focus look.
    assert_eq!(
        app.world().get::<Stroke>(field).unwrap().0,
        ColorRole::Accent
    );
    let window = {
        let w = app.world_mut();
        w.query_filtered::<Entity, With<PrimaryWindow>>()
            .single(w)
            .unwrap()
    };
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    frames(&mut app, 2);
    assert_eq!(
        app.world().resource::<Submits>().0,
        ["ws://localhost:9002".to_string()]
    );
}

/// A live tooltip card's rect and its arrow's node, a few frames after
/// it was shown on `trigger`.
fn shown_tooltip(app: &mut App, trigger: Entity, tip: Tooltip) -> (Rect, Rect, Node) {
    use crate::widgets::tooltip::show_tooltip;
    let theme = app.world().resource::<Theme>().clone();
    let mut commands = app.world_mut().commands();
    show_tooltip(&mut commands, &theme, trigger, &tip);
    app.world_mut().flush();
    frames(app, 4);
    let w = app.world_mut();
    let rect = |w: &World, e: Entity| {
        let n = w.get::<ComputedNode>(e).unwrap();
        let t = w.get::<UiGlobalTransform>(e).unwrap();
        let k = n.inverse_scale_factor();
        Rect::from_center_size(t.translation * k, n.size() * k)
    };
    let card = w
        .query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
        .iter(w)
        .find(|(_, p)| p.parent() == trigger)
        .unwrap()
        .0;
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    (
        rect(w, card),
        rect(w, trigger),
        w.get::<Node>(arrow).unwrap().clone(),
    )
}

/// Tooltips never leave the window: activity buttons (at the left edge)
/// open theirs to the right; a centred tooltip on a trigger at the right
/// edge shifts in to an 8 px margin, its arrow still under the trigger.
#[test]
fn tooltips_stay_inside_the_window() {
    use crate::widgets::tooltip::{shift_inside, GAP};
    let bounds = Rect::new(8.0, 8.0, 1432.0, 892.0);
    assert_eq!(
        shift_inside(Rect::new(-20.0, 10.0, 80.0, 40.0), bounds),
        Vec2::new(28.0, 0.0)
    );
    assert_eq!(
        shift_inside(Rect::new(1400.0, 880.0, 1500.0, 900.0), bounds),
        Vec2::new(-68.0, -8.0)
    );
    assert_eq!(
        shift_inside(Rect::new(10.0, 10.0, 20.0, 20.0), bounds),
        Vec2::ZERO
    );

    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    // An activity button at the window's left edge.
    let at = |app: &mut App, node: Node| app.world_mut().spawn(node).id();
    let left_slot = at(
        &mut app,
        Node {
            position_type: PositionType::Absolute,
            left: px(6),
            top: px(60),
            ..default()
        },
    );
    let activity = app
        .world_mut()
        .spawn((
            activity_button(
                &t,
                IconId::ActivityScene,
                Tooltip::new("Scene · long enough to clip"),
                None,
            ),
            ChildOf(left_slot),
        ))
        .id();
    // An icon button at the right edge with a tooltip below.
    let right_slot = at(
        &mut app,
        Node {
            position_type: PositionType::Absolute,
            right: px(30),
            top: px(4),
            ..default()
        },
    );
    let right = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("x"),
            ),
            ChildOf(right_slot),
        ))
        .id();
    frames(&mut app, 2);
    let tip = app.world().get::<Tooltip>(activity).unwrap().clone();
    assert_eq!(tip.side, TooltipSide::Right);
    let (card, button, arrow) = shown_tooltip(&mut app, activity, tip);
    // (The popover measures from inside the button's 1px border.)
    assert!(
        card.min.x >= button.max.x + GAP - 1.5,
        "{card:?} right of {button:?}"
    );
    // One line per row: the title doesn't wrap to the 44 px trigger.
    assert!(card.height() < 40.0 && card.width() > 150.0, "{card:?}");
    assert!(card.min.x >= GAP && card.min.y >= GAP && card.max.y <= 900.0 - GAP);
    // The arrow is on the card's left edge.
    assert_eq!(arrow.left, px(-GAP / 2.0));
    let long =
        Tooltip::new("Settings for the whole workbench").detail("Below, near the right edge");
    let (card, button, arrow) = shown_tooltip(&mut app, right, long);
    assert!(card.max.x <= 1440.0 - GAP + 0.5, "{card:?} inside");
    assert!(card.min.y >= button.max.y + GAP - 1.5, "{card:?} below");
    // Shifted left, the arrow still points at the trigger.
    let Val::Px(left) = arrow.left else {
        panic!("{:?}", arrow.left)
    };
    assert!((card.min.x + 1.0 + left + GAP / 2.0 - button.center().x).abs() < 0.5);
    assert_eq!(arrow.top, px(-GAP / 2.0));
    // Shifted once, it stays put: no popover moving it back each frame.
    let w = app.world_mut();
    let shifted = w
        .query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
        .iter(w)
        .find(|(_, p)| p.parent() == right)
        .unwrap()
        .0;
    assert!(w
        .get::<bevy::ui_widgets::popover::Popover>(shifted)
        .is_none());
    let before = *w.get::<UiTransform>(shifted).unwrap();
    frames(&mut app, 3);
    assert_eq!(*app.world().get::<UiTransform>(shifted).unwrap(), before);
}
