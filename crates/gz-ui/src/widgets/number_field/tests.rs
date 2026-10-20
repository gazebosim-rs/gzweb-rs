//! NumberField: the expression parser, commit and revert, scrubbing.

use bevy::a11y::AccessibilityNode;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::window::PrimaryWindow;

use crate::test_support::{frames, ui_app};
use crate::theme::{ColorRole, Ring, Stroke, Theme, ThemeMode};
use crate::widgets::number_field::expr::{eval, ExprError, Unit};
use crate::widgets::number_field::{Invalid, NumberInput, NumberTag, Scrub, TAG_TYPE, VALUE_TYPE};
use crate::widgets::{number_field, AxisTag, NumberCommit, NumberField};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn arithmetic() {
    let n = |s| eval(s, Unit::None).unwrap();
    assert!(close(n("1.5*2"), 3.0));
    assert!(close(n(" -(0.25 + 1)/2 "), -0.625));
    assert!(close(n("2+3*4"), 14.0));
    assert!(close(n("(2+3)*4"), 20.0));
    assert!(close(n("8/4/2"), 1.0));
    assert!(close(n("10-4-3"), 3.0));
    assert!(close(n("+5"), 5.0));
    assert!(close(n("--5"), 5.0));
    assert!(close(n(".5"), 0.5));
    assert!(close(n("1e3"), 1000.0));
    assert!(close(n("2.5E-1"), 0.25));
    assert!(close(n("PI"), std::f64::consts::PI));
    assert!(close(n("12.5%"), 0.125));
}

#[test]
fn units_convert_into_the_field_unit() {
    let a = |s| eval(s, Unit::Angle).unwrap();
    assert!(close(a("90deg"), 90.0));
    assert!(close(a("90 °"), 90.0));
    assert!(close(a("(pi/2)rad"), 90.0));
    assert!(close(a("1rad"), 180.0 / std::f64::consts::PI));
    assert!(close(a("45deg*2"), 90.0));
    // The module doc's example: the unit applies to the whole term.
    assert!(close(a("pi/2 rad"), 90.0));
    assert!(close(a("pi / 4rad * 2"), 90.0));
    assert!(close(a("2*pi/8 rad"), 45.0));
    assert!(close(a("-pi/2 rad + 10"), -80.0));
    assert!(close(eval("50/2%", Unit::None).unwrap(), 0.25));
    // One unit per term.
    assert_eq!(
        eval("90deg*2rad", Unit::Angle),
        Err(ExprError::Unexpected(7))
    );
    let l = |s| eval(s, Unit::Length).unwrap();
    assert!(close(l("50cm"), 0.5));
    assert!(close(l("2 mm"), 0.002));
    assert!(close(l("1.5km"), 1500.0));
    assert!(close(l("3m + 20cm"), 3.2));
    assert_eq!(
        eval("90deg", Unit::Length),
        Err(ExprError::WrongUnit("deg".into()))
    );
    assert_eq!(
        eval("5cm", Unit::Angle),
        Err(ExprError::WrongUnit("cm".into()))
    );
    assert_eq!(
        eval("5ft", Unit::Length),
        Err(ExprError::UnknownUnit("ft".into()))
    );
    assert_eq!(Unit::Length.suffix(), Some("m"));
    assert_eq!(Unit::Angle.suffix(), Some("°"));
    assert_eq!(Unit::None.suffix(), None);
}

#[test]
fn errors() {
    let e = |s| eval(s, Unit::None).unwrap_err();
    assert_eq!(e(""), ExprError::Empty);
    assert_eq!(e("   "), ExprError::Empty);
    assert_eq!(e("1+"), ExprError::UnexpectedEnd);
    assert_eq!(e("(1"), ExprError::UnexpectedEnd);
    assert_eq!(e("(1 2"), ExprError::Unexpected(3));
    assert_eq!(e("1 2"), ExprError::Unexpected(2));
    assert_eq!(e("abc"), ExprError::Unexpected(0));
    assert_eq!(e("2pi"), ExprError::Unexpected(1));
    assert_eq!(e("1/0"), ExprError::NotFinite);
    assert_eq!(e("."), ExprError::Unexpected(0));
    // `2e` is 2 followed by a unit-less `e`.
    assert_eq!(e("2e"), ExprError::Unexpected(1));
    for err in [
        ExprError::Empty,
        ExprError::Unexpected(3),
        ExprError::UnexpectedEnd,
        ExprError::UnknownUnit("ft".into()),
        ExprError::WrongUnit("cm".into()),
        ExprError::NotFinite,
        ExprError::TooDeep,
    ] {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn nesting_is_bounded() {
    use crate::widgets::number_field::expr::MAX_DEPTH;
    let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    assert_eq!(eval(&nested(MAX_DEPTH), Unit::None), Ok(1.0));
    assert_eq!(
        eval(&nested(MAX_DEPTH + 1), Unit::None),
        Err(ExprError::TooDeep)
    );
    // Far past the limit (this overflowed the stack before the limit).
    assert_eq!(
        eval(&"(".repeat(100_000), Unit::None),
        Err(ExprError::TooDeep)
    );
    // A long run of signs doesn't recurse at all.
    let signs = format!("{}1", "-".repeat(100_001));
    assert_eq!(eval(&signs, Unit::None), Ok(-1.0));
}

#[test]
fn format_parse_clamp_and_scrub() {
    let f = NumberField::new(0.0).precision(2, 0.1).range(-1.0, 1.0);
    assert_eq!(f.format(0.123), "0.12");
    assert_eq!(f.format(-0.001), "0.00");
    assert_eq!(f.format(-0.5), "-0.50");
    assert_eq!(f.parse("0.5*3"), Ok(1.0));
    assert_eq!(f.parse("x"), Err(ExprError::Unexpected(0)));
    assert_eq!(f.clamp(-4.0), -1.0);
    // One step per pixel; Shift ×10, Alt ÷10.
    assert!(close(f.scrub(0.0, 2.0, false, false), 0.2));
    assert!(close(f.scrub(0.0, 1.0, true, false), 1.0));
    assert!(close(f.scrub(0.0, 3.0, false, true), 0.03));
    assert!(close(f.scrub(0.0, 3.0, true, true), 0.3));
    assert!(close(f.scrub(0.0, 100.0, false, false), 1.0));
    assert!(close(f.scrub(0.5, -2.0, false, false), 0.3));
    let deg = NumberField::new(15.0).unit(Unit::Angle);
    assert_eq!(deg.parse("(pi/4)rad"), Ok(45.0));
    assert_eq!(deg.format(45.0), "45.000");
    for (tag, s, role) in [
        (AxisTag::X, "X", ColorRole::AxisXText),
        (AxisTag::Y, "Y", ColorRole::AxisYText),
        (AxisTag::Z, "Z", ColorRole::AxisZText),
        (AxisTag::Roll, "R", ColorRole::AxisXText),
        (AxisTag::Pitch, "P", ColorRole::AxisYText),
        (AxisTag::Yaw, "Yw", ColorRole::AxisZText),
        (AxisTag::Label("mass".into()), "mass", ColorRole::Text2),
    ] {
        assert_eq!(tag.tag(), s);
        assert_eq!(tag.role(), role);
    }
    assert_eq!(TAG_TYPE.weight, 600);
    assert_eq!(VALUE_TYPE.size, 12.0);
}

/// Commits seen by an observer.
#[derive(Resource, Default)]
struct Commits(Vec<(f64, bool)>);

struct Field {
    app: App,
    root: Entity,
    input: Entity,
    tag: Entity,
    window: Entity,
}

fn field() -> Field {
    let mut app = ui_app(ThemeMode::Light);
    app.init_resource::<Commits>()
        .add_observer(|ev: On<NumberCommit>, mut c: ResMut<Commits>| {
            c.0.push((ev.value, ev.is_final));
        });
    let t = Theme::default();
    let root = app
        .world_mut()
        .spawn(number_field(
            &t,
            AxisTag::X,
            NumberField::new(2.4).precision(3, 0.01),
            false,
        ))
        .id();
    frames(&mut app, 2);
    let children: Vec<Entity> = app.world().get::<Children>(root).unwrap().to_vec();
    let w = app.world_mut();
    let input = children
        .iter()
        .copied()
        .find(|c| w.get::<NumberInput>(*c).is_some())
        .unwrap();
    let tag = children
        .iter()
        .copied()
        .find(|c| w.get::<NumberTag>(*c).is_some())
        .unwrap();
    let window = w
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(w)
        .unwrap();
    Field {
        app,
        root,
        input,
        tag,
        window,
    }
}

impl Field {
    fn text(&self) -> String {
        self.app
            .world()
            .get::<EditableText>(self.input)
            .unwrap()
            .value()
            .to_string()
    }

    fn type_text(&mut self, s: &str) {
        let mut e = self
            .app
            .world_mut()
            .get_mut::<EditableText>(self.input)
            .unwrap();
        e.editor.set_text(s);
    }

    fn focus(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<InputFocus>()
            .set(self.input, FocusCause::Navigated);
        frames(&mut self.app, 2);
    }

    fn press(&mut self, key: KeyCode, logical: Key) {
        let window = self.window;
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: key,
                logical_key: logical.clone(),
                state,
                text: None,
                repeat: false,
                window,
            });
            frames(&mut self.app, 1);
        }
        frames(&mut self.app, 1);
    }

    fn value(&self) -> f64 {
        self.app
            .world()
            .get::<NumberField>(self.root)
            .unwrap()
            .value
    }

    fn commits(&self) -> Vec<(f64, bool)> {
        self.app.world().resource::<Commits>().0.clone()
    }
}

#[test]
fn enter_commits_an_expression_and_reformats() {
    let mut f = field();
    assert_eq!(f.text(), "2.400");
    f.focus();
    // Focus look: accent border, accent_soft halo.
    let w = f.app.world();
    assert_eq!(*w.get::<Stroke>(f.root).unwrap(), Stroke(ColorRole::Accent));
    assert_eq!(*w.get::<Ring>(f.root).unwrap(), Ring(ColorRole::AccentSoft));
    f.type_text("1.5*2");
    f.press(KeyCode::Enter, Key::Enter);
    assert_eq!(f.value(), 3.0);
    assert_eq!(f.text(), "3.000");
    assert_eq!(f.commits(), vec![(3.0, true)]);
    // The same value again: no new commit.
    f.type_text("3");
    f.press(KeyCode::Enter, Key::Enter);
    assert_eq!(f.commits().len(), 1);
}

#[test]
fn bad_text_marks_the_field_and_esc_reverts() {
    let mut f = field();
    f.focus();
    f.type_text("1+");
    f.press(KeyCode::Enter, Key::Enter);
    assert!(f.app.world().get::<Invalid>(f.root).is_some());
    assert_eq!(
        *f.app.world().get::<Stroke>(f.root).unwrap(),
        Stroke(ColorRole::Danger)
    );
    assert_eq!(f.value(), 2.4);
    f.press(KeyCode::Escape, Key::Escape);
    assert!(f.app.world().get::<Invalid>(f.root).is_none());
    assert_eq!(f.text(), "2.400");
    // Esc also dropped focus; nothing was committed.
    assert_eq!(f.app.world().resource::<InputFocus>().get(), None);
    assert!(f.commits().is_empty());
    assert_eq!(
        *f.app.world().get::<Stroke>(f.root).unwrap(),
        Stroke(ColorRole::Line)
    );
}

#[test]
fn blur_commits_or_reverts() {
    let mut f = field();
    f.focus();
    f.type_text("10cm");
    // Unit::None doesn't take cm: blur reverts.
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut f.app, 2);
    assert_eq!(f.text(), "2.400");
    assert!(f.commits().is_empty());
    f.focus();
    f.type_text("0.5 + 0.25");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut f.app, 2);
    assert_eq!(f.value(), 0.75);
    assert_eq!(f.text(), "0.750");
    assert_eq!(f.commits(), vec![(0.75, true)]);
}

#[test]
fn unedited_text_commits_nothing() {
    let mut f = field();
    f.app
        .world_mut()
        .get_mut::<NumberField>(f.root)
        .unwrap()
        .value = 2.4567;
    frames(&mut f.app, 2);
    assert_eq!(f.text(), "2.457");
    // Tabbing through: focus and blur without typing keeps the full value.
    f.focus();
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut f.app, 2);
    assert_eq!(f.value(), 2.4567);
    assert!(f.commits().is_empty());
    // Enter on unedited text doesn't commit either.
    f.focus();
    f.press(KeyCode::Enter, Key::Enter);
    assert_eq!(f.value(), 2.4567);
    assert!(f.commits().is_empty());
    // A live value arriving while focused: the text stays frozen, and blur
    // without change shows the new value instead of writing the old one.
    f.app
        .world_mut()
        .get_mut::<NumberField>(f.root)
        .unwrap()
        .value = 5.0;
    frames(&mut f.app, 2);
    assert_eq!(f.text(), "2.457");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut f.app, 2);
    assert_eq!(f.value(), 5.0);
    assert_eq!(f.text(), "5.000");
    assert!(f.commits().is_empty());
}

#[test]
fn the_field_has_an_accessible_name_and_value() {
    let f = field();
    let node = f.app.world().get::<AccessibilityNode>(f.input).unwrap();
    assert_eq!(node.label(), Some("X"));
    assert_eq!(node.numeric_value(), Some(2.4));
    assert_eq!(AxisTag::Yaw.name(), "Yaw");
    assert_eq!(AxisTag::Label("mass".into()).name(), "mass");
    let named = NumberField::new(0.0).named("Position X");
    assert_eq!(named.name.as_deref(), Some("Position X"));
}

#[test]
fn scrubbing_the_tag() {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerDrag, PointerDragEnd, PointerDragStart};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};

    let mut f = field();
    let target = NormalizedRenderTarget::None {
        width: 1,
        height: 1,
    };
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target,
            position: Vec2::ZERO,
        },
    );
    let tag = f.tag;
    let w = f.app.world_mut();
    w.trigger(PointerDragStart {
        entity: tag,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    w.flush();
    assert_eq!(w.get::<Scrub>(f.root), Some(&Scrub { value: 2.4 }));
    let mut distance = 0.0;
    let mut drag = |w: &mut World, dx: f32| {
        distance += dx;
        w.trigger(PointerDrag {
            entity: tag,
            pointer: pointer.clone(),
            button: PointerButton::Primary,
            distance: Vec2::new(distance, 3.0),
            delta: Vec2::new(dx, 1.0),
        });
        w.flush();
    };
    let value = |w: &World| w.get::<NumberField>(f.root).unwrap().value;
    drag(w, 10.0);
    assert!(close(value(w), 2.5));
    // Shift: coarse, for the moves made while it's held. Pressing it
    // doesn't rescale the distance already dragged.
    w.resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    drag(w, 0.0);
    assert!(close(value(w), 2.5));
    drag(w, 10.0);
    assert!(close(value(w), 3.5));
    w.resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    w.resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    drag(w, 10.0);
    assert!(close(value(w), 3.51));
    drag(w, -20.0);
    assert!(close(value(w), 3.49));
    w.trigger(PointerDragEnd {
        entity: tag,
        pointer,
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    });
    w.flush();
    assert!(w.get::<Scrub>(f.root).is_none());
    frames(&mut f.app, 2);
    let commits = f.commits();
    assert_eq!(commits.len(), 5);
    assert!(commits[..4].iter().all(|c| !c.1));
    assert!(close(commits[4].0, 3.49) && commits[4].1);
    // The unfocused field shows the scrubbed value.
    assert_eq!(f.text(), "3.490");
}
