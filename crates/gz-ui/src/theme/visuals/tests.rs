//! State visuals (spec §5 state table).

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled, Pressed};

use crate::test_support::{frames, ui_app};
use crate::theme::visuals::{
    Active, Content, ForceState, HoverOnFocus, Look, Visuals, WidgetState, DISABLED_OPACITY,
};
use crate::theme::{ColorRole, Fill, Ink, Opacity, Stroke, ThemeMode};

const BASE: Visuals = Visuals::new(ColorRole::Clear, ColorRole::Icon);

fn look(s: WidgetState) -> Look {
    BASE.look(s)
}

fn st(hovered: bool, pressed: bool, active: bool, disabled: bool) -> WidgetState {
    WidgetState {
        hovered,
        pressed,
        active,
        disabled,
    }
}

#[test]
fn the_state_table() {
    let rest = look(st(false, false, false, false));
    assert_eq!(
        (rest.fill, rest.ink, rest.opacity),
        (ColorRole::Clear, ColorRole::Icon, 1.0)
    );
    let hover = look(st(true, false, false, false));
    assert_eq!((hover.fill, hover.ink), (ColorRole::Hover, ColorRole::Text));
    let pressed = look(st(true, true, false, false));
    assert_eq!(
        (pressed.fill, pressed.ink),
        (ColorRole::Pressed, ColorRole::Text)
    );
    let active = look(st(true, true, true, false));
    assert_eq!(
        (active.fill, active.ink),
        (ColorRole::AccentSoft, ColorRole::AccentText)
    );
    // Disabled: unchanged fill, 40% content, no hover.
    let disabled = look(st(true, true, false, true));
    assert_eq!(
        (disabled.fill, disabled.ink),
        (ColorRole::Clear, ColorRole::Icon)
    );
    assert_eq!(disabled.opacity, DISABLED_OPACITY);
    // Borders.
    let stroked = BASE.stroked(ColorRole::Line);
    assert_eq!(
        stroked.look(st(true, false, false, false)).stroke,
        Some(ColorRole::Line)
    );
    assert_eq!(BASE.look(st(false, false, false, false)).stroke, None);
    // keep_ink: bordered buttons keep their label colour on hover.
    let kept = Visuals::new(ColorRole::Surface2, ColorRole::Danger).keep_ink();
    assert_eq!(
        kept.look(st(true, false, false, false)).ink,
        ColorRole::Danger
    );
}

#[test]
fn the_system_drives_fill_content_and_opacity() {
    let mut app = ui_app(ThemeMode::Light);
    let w = app.world_mut();
    let button = w
        .spawn((
            Node::default(),
            BASE.stroked(ColorRole::Line),
            Hovered(false),
        ))
        .id();
    let icon = w.spawn((Node::default(), Content, ChildOf(button))).id();
    let wrapper = w.spawn((Node::default(), ChildOf(button))).id();
    let label = w.spawn((Text::new("a"), Content, ChildOf(wrapper))).id();
    // A nested widget's content is its own.
    let nested = w
        .spawn((Node::default(), Content, BASE, ChildOf(button)))
        .id();
    frames(&mut app, 1);
    let fill = |app: &App| *app.world().get::<Fill>(button).unwrap();
    let ink = |app: &App, e| *app.world().get::<Ink>(e).unwrap();
    assert_eq!(fill(&app), Fill(ColorRole::Clear));
    assert_eq!(
        *app.world().get::<Stroke>(button).unwrap(),
        Stroke(ColorRole::Line)
    );
    assert_eq!(ink(&app, icon), Ink(ColorRole::Icon));
    assert_eq!(ink(&app, label), Ink(ColorRole::Icon));
    assert!(app.world().get::<Ink>(nested).is_none());

    app.world_mut().entity_mut(button).insert(Hovered(true));
    frames(&mut app, 1);
    assert_eq!(fill(&app), Fill(ColorRole::Hover));
    assert_eq!(ink(&app, label), Ink(ColorRole::Text));

    app.world_mut().entity_mut(button).insert(Pressed);
    frames(&mut app, 1);
    assert_eq!(fill(&app), Fill(ColorRole::Pressed));

    app.world_mut()
        .entity_mut(button)
        .remove::<Pressed>()
        .insert(Active);
    frames(&mut app, 1);
    assert_eq!(fill(&app), Fill(ColorRole::AccentSoft));
    assert_eq!(ink(&app, icon), Ink(ColorRole::AccentText));

    app.world_mut()
        .entity_mut(button)
        .remove::<Active>()
        .insert(InteractionDisabled);
    frames(&mut app, 1);
    assert_eq!(fill(&app), Fill(ColorRole::Clear));
    assert_eq!(
        *app.world().get::<Opacity>(icon).unwrap(),
        Opacity(DISABLED_OPACITY)
    );

    // ForceState wins over the pointer.
    app.world_mut()
        .entity_mut(button)
        .remove::<InteractionDisabled>()
        .insert(ForceState(st(false, false, true, false)));
    frames(&mut app, 1);
    assert_eq!(fill(&app), Fill(ColorRole::AccentSoft));
}

#[test]
fn checked_and_focus_hover() {
    let mut app = ui_app(ThemeMode::Light);
    let mut no_check = BASE;
    no_check.checked_is_active = false;
    let w = app.world_mut();
    let radio = w.spawn((Node::default(), BASE, Checked)).id();
    let check_item = w.spawn((Node::default(), no_check, Checked)).id();
    let item = w.spawn((Node::default(), BASE, HoverOnFocus)).id();
    frames(&mut app, 1);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(item, FocusCause::Navigated);
    frames(&mut app, 1);
    let fill = |app: &App, e| app.world().get::<Fill>(e).unwrap().0;
    assert_eq!(fill(&app, radio), ColorRole::AccentSoft);
    assert_eq!(fill(&app, check_item), ColorRole::Clear);
    assert_eq!(fill(&app, item), ColorRole::Hover);
}

/// The system visits only widgets whose state changed: a settled widget
/// isn't rewritten, and each kind of change (and its removal) restyles.
#[test]
fn only_changed_widgets_are_restyled() {
    let mut app = ui_app(ThemeMode::Light);
    let w = app.world_mut();
    let a = w.spawn((Node::default(), BASE, Hovered(false))).id();
    let b = w.spawn((Node::default(), BASE, HoverOnFocus)).id();
    frames(&mut app, 2);
    let fill = |app: &App, e| app.world().get::<Fill>(e).unwrap().0;
    // A marker colour nothing would write: it stays while `a` is settled.
    app.world_mut()
        .entity_mut(a)
        .insert(Fill(ColorRole::Danger));
    frames(&mut app, 2);
    assert_eq!(fill(&app, a), ColorRole::Danger);
    // Each change and its undoing restyles.
    for (on, off, want) in [
        (
            (|e: &mut EntityWorldMut| {
                e.insert(Checked);
            }) as fn(&mut EntityWorldMut),
            (|e: &mut EntityWorldMut| {
                e.remove::<Checked>();
            }) as fn(&mut EntityWorldMut),
            ColorRole::AccentSoft,
        ),
        (
            |e| {
                e.insert(Active);
            },
            |e| {
                e.remove::<Active>();
            },
            ColorRole::AccentSoft,
        ),
        (
            |e| {
                e.insert(Pressed);
            },
            |e| {
                e.remove::<Pressed>();
            },
            ColorRole::Pressed,
        ),
        (
            |e| {
                e.insert(ForceState(st(true, false, false, false)));
            },
            |e| {
                e.remove::<ForceState>();
            },
            ColorRole::Hover,
        ),
    ] {
        on(&mut app.world_mut().entity_mut(a));
        frames(&mut app, 1);
        assert_eq!(fill(&app, a), want);
        off(&mut app.world_mut().entity_mut(a));
        frames(&mut app, 1);
        assert_eq!(fill(&app, a), ColorRole::Clear);
    }
    // Content spawned later takes its widget's look.
    app.world_mut().entity_mut(a).insert(Active);
    frames(&mut app, 1);
    let late = app
        .world_mut()
        .spawn((Node::default(), ChildOf(a)))
        .with_child((Text::new("late"), Content))
        .id();
    frames(&mut app, 1);
    let text = app.world().get::<Children>(late).unwrap()[0];
    assert_eq!(
        *app.world().get::<Ink>(text).unwrap(),
        Ink(ColorRole::AccentText)
    );
    // Focus moving off a HoverOnFocus widget restyles it.
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(b, FocusCause::Navigated);
    frames(&mut app, 1);
    assert_eq!(fill(&app, b), ColorRole::Hover);
    app.world_mut().resource_mut::<InputFocus>().clear();
    frames(&mut app, 1);
    assert_eq!(fill(&app, b), ColorRole::Clear);
}

/// A disabled widget that is on still shows that it's on (§10: state
/// isn't hidden), with its content at 40%.
#[test]
fn disabled_keeps_the_active_look() {
    let l = look(st(false, false, true, true));
    assert_eq!(
        (l.fill, l.ink, l.opacity),
        (
            ColorRole::AccentSoft,
            ColorRole::AccentText,
            DISABLED_OPACITY
        )
    );
}
