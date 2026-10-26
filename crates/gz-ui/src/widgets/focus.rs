//! Keyboard focus in text fields (spec §13: "single-letter shortcuts do
//! nothing while a text field has focus").
//!
//! While a UI text field has focus, the viewer's key bindings are off (its
//! [`ActiveScopes`] are emptied and restored afterwards), and in the
//! browser the canvas stops preventing any key's default but Tab, so
//! browser shortcuts keep working and Tab stays in the UI
//! (`gz_bevy::view::web`, spec §9.6).
//!
//! The scopes come back one frame after the field loses focus: the key
//! that blurred it (Esc, Tab, Enter in some fields) is still
//! `just_pressed` in that frame, and the viewer would act on it too.
//!
//! Keyboard focus never stays where the keyboard can't see it or didn't
//! put it ([`release_focus`], DECISIONS.md D-52): a widget that is hidden
//! (it or an ancestor `Display::None`) or on its way out ([`Hiding`])
//! loses focus, and so does a [`PressFocusless`] control that took focus
//! from the pointer. Bevy's buttons activate on Space and Enter while
//! focused, and Space is also the global play/pause chord (D-32), so a
//! clicked button kept focused would act twice on Space, or a hidden one
//! act unseen.
//!
//! Nor does Tab land there ([`hide_undisplayed`], DECISIONS.md D-58):
//! Bevy's tab navigation skips only invisible entities, so a node that
//! isn't displayed is made invisible too.
//!
//! A disabled control is no Tab stop ([`skip_disabled`]): Tab doesn't
//! land on a button that does nothing.
//!
//! A focused widget consumes the keys it acts on ([`hide_taken_keys`],
//! DECISIONS.md D-58a): Bevy hands a key to the focused widget first, and
//! a widget that acts on it (a button on Space or Enter, a menu on its
//! arrows, a slider on its keys) stops it there; only a key that reaches
//! the window is the shortcuts' to see. So a Tab-focused Reset takes
//! Space and the global play/pause chord doesn't fire as well.
//!
//! The arrow keys move within a group ([`arrow_keys`], spec §6.5,
//! DECISIONS.md D-117): along an [`ArrowGroup`] (the activity bar), or
//! through a radio group (the tool group, the rail's drag group, the
//! panels' chips), where the focused button is checked too, as WAI-ARIA's
//! radio groups do. Only plain arrows: with a modifier they stay the
//! shortcuts' (⌘→ steps the sim).

use bevy::ecs::change_detection::Tick;
use bevy::ecs::system::SystemChangeTick;
use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{MenuPopup, RadioButton, RadioGroup, TextInput, ValueChange};
use bevy::window::PrimaryWindow;
use gz_bevy::view::controls::ActiveScopes;
use gz_bevy::view::input::Scope;

use super::layout::Orientation;

/// A text field has keyboard focus.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextFocus(pub bool);

/// What [`text_focus_system`] keeps between frames.
#[derive(Default)]
pub(crate) struct Handover {
    /// The viewer's scopes while a field has them.
    saved: Option<Vec<Scope>>,
    /// The field lost focus last frame: give the scopes back now.
    restore: bool,
}

/// Tracks [`TextFocus`] and hands the keys to the field or back.
pub(crate) fn text_focus_system(
    focus: Res<InputFocus>,
    inputs: Query<(), With<TextInput>>,
    mut state: ResMut<TextFocus>,
    mut scopes: Option<ResMut<ActiveScopes>>,
    mut handover: Local<Handover>,
) {
    let typing = focus.get().is_some_and(|e| inputs.contains(e));
    if handover.restore && !typing {
        handover.restore = false;
        if let (Some(scopes), Some(previous)) = (scopes.as_mut(), handover.saved.take()) {
            // Scopes someone set while the field had focus win over the
            // ones saved before it.
            if scopes.0.is_empty() {
                scopes.0 = previous;
            }
        }
    }
    if state.0 == typing {
        return;
    }
    state.0 = typing;
    if typing {
        if handover.restore {
            // Straight into another field: the scopes are still saved.
            handover.restore = false;
        } else if let Some(scopes) = scopes.as_mut() {
            handover.saved = Some(std::mem::take(&mut scopes.0));
        }
    } else {
        handover.restore = true;
    }
    #[cfg(target_arch = "wasm32")]
    gz_bevy::view::web::set_text_focus(typing);
}

/// A control that takes keyboard focus only from the keyboard (Tab): a
/// pointer press on it doesn't leave focus there. Every clickable widget
/// has it ([`clickable`](crate::widgets::clickable)); text fields, sliders
/// and splitters, whose keys are their own, don't.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PressFocusless;

/// A node on its way out (a fade before it hides): focus inside it is
/// released as if it were hidden already.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Hiding;

/// An open menu takes this frame's Esc (spec §13: one step per Esc, the
/// menu first), so shortcuts on Esc stand down.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuTakesEscape(pub bool);

/// Something menu-like that closes on Esc (the command palette): with
/// focus in it, it takes the Esc as an open menu does.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TakesEscape;

/// The last press was a pointer's (a mouse button or a touch), not a
/// key's: focus that moves now moved because of it (a click, or a menu
/// item chosen with the mouse handing focus back to its trigger).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PointerLed(pub bool);

/// Keeps [`PointerLed`]. Runs before Bevy dispatches this frame's input.
pub(crate) fn note_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    touches: Option<Res<Touches>>,
    mut led: ResMut<PointerLed>,
) {
    let pointer =
        mouse.get_just_pressed().next().is_some() || touches.is_some_and(|t| t.any_just_pressed());
    if pointer {
        led.set_if_neq(PointerLed(true));
    } else if keys.get_just_pressed().next().is_some() {
        led.set_if_neq(PointerLed(false));
    }
}

/// Releases keyboard focus that the keyboard can't use: on a hidden or
/// hiding widget, or on a [`PressFocusless`] control that took it from the
/// pointer ([`PointerLed`]; Tab and arrow keys keep it). Runs in PostUpdate
/// after everything that hides, shows or focuses, before focus events go
/// out.
pub(crate) fn release_focus(
    mut focus: ResMut<InputFocus>,
    led: Res<PointerLed>,
    nodes: Query<(&Node, Has<Hiding>)>,
    pressless: Query<(), With<PressFocusless>>,
    parents: Query<&ChildOf>,
) {
    let Some(e) = focus.get() else { return };
    let gone = std::iter::once(e)
        .chain(parents.iter_ancestors(e))
        .any(|a| {
            nodes
                .get(a)
                .is_ok_and(|(n, h)| h || n.display == Display::None)
        });
    let pressed = led.0 && pressless.contains(e);
    if gone || pressed {
        focus.clear();
    }
}

/// A node [`hide_undisplayed`] made `Visibility::Hidden`, at this tick: a
/// later change to its visibility is its owner's, which then wins.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct HiddenByDisplay(Tick);

/// A node's display or its visibility changed.
type DisplayOrVisibilityChanged = Or<(Changed<Node>, Changed<Visibility>)>;

/// Makes a node that isn't displayed (`Display::None`: a collapsed side
/// panel, the closed second slot, a faded-out card) `Visibility::Hidden`,
/// and visible again once it is displayed, so Bevy's tab navigation,
/// which skips only what `InheritedVisibility` hides, skips it and its
/// subtree. Only a node left at `Visibility::Inherited` is touched: one
/// that sets its own visibility (a menu's popup, the 3D view's node)
/// keeps it, and so does one whose owner set its visibility while it was
/// hidden here. Nothing looks different: an undisplayed node draws
/// nothing either way. Runs in PostUpdate before visibility propagates.
pub(crate) fn hide_undisplayed(
    mut nodes: Query<
        (Entity, &Node, &mut Visibility, Option<&HiddenByDisplay>),
        DisplayOrVisibilityChanged,
    >,
    ticks: SystemChangeTick,
    mut commands: Commands,
) {
    for (e, node, mut visibility, hidden) in &mut nodes {
        let shown = node.display != Display::None;
        // Hidden here, and nobody has set its visibility since.
        let ours = hidden.is_some_and(|h| visibility.last_changed() == h.0);
        if ours && shown {
            *visibility = Visibility::Inherited;
            commands.entity(e).try_remove::<HiddenByDisplay>();
        } else if !ours && !shown && *visibility == Visibility::Inherited {
            *visibility = Visibility::Hidden;
            commands
                .entity(e)
                .try_insert(HiddenByDisplay(ticks.this_run()));
        } else if hidden.is_some() && !ours {
            // Its owner set its visibility: theirs from now on.
            commands.entity(e).try_remove::<HiddenByDisplay>();
        }
    }
}

/// Notes whether an Esc pressed this frame goes to an open menu: focus is
/// inside a visible menu popup (or something else that [`TakesEscape`])
/// when the key arrives. Runs before Bevy dispatches keys (the menu
/// closes on them).
pub(crate) fn menu_escape(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    popups: Query<&Visibility, With<MenuPopup>>,
    takers: Query<(), With<TakesEscape>>,
    parents: Query<&ChildOf>,
    mut taken: ResMut<MenuTakesEscape>,
) {
    let esc = keys.just_pressed(KeyCode::Escape);
    let in_menu = esc
        && focus.get().is_some_and(|e| {
            std::iter::once(e)
                .chain(parents.iter_ancestors(e))
                .any(|a| {
                    takers.contains(a) || popups.get(a).is_ok_and(|v| *v != Visibility::Hidden)
                })
        });
    taken.set_if_neq(MenuTakesEscape(in_menu));
}

/// Which keys focused widgets took (DECISIONS.md D-58a).
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct WidgetKeys {
    /// This frame's presses that reached the window: no widget took them.
    reached: Vec<KeyCode>,
    /// Keys a focused widget took, hidden from the shortcuts until they
    /// are released.
    taken: Vec<KeyCode>,
}

/// Notes a key press that reached the window: nothing focused took it.
/// Observes each primary window ([`watch_windows`]), where Bevy delivers
/// a key when nothing has focus and where a focused widget's keys arrive
/// unless it stopped them.
pub(crate) fn note_window_key(ev: On<FocusedInput<KeyboardInput>>, mut keys: ResMut<WidgetKeys>) {
    let input = &ev.input;
    if input.state == ButtonState::Pressed && !input.repeat {
        keys.reached.push(input.key_code);
    }
}

/// Watches the primary window for keys that reach it ([`note_window_key`]).
pub(crate) fn watch_windows(windows: Query<Entity, With<PrimaryWindow>>, mut commands: Commands) {
    for window in &windows {
        commands.entity(window).observe(note_window_key);
    }
}

/// Hides a key a focused widget took from the shortcuts: a press Bevy
/// dispatched that never reached the window is reset in
/// `ButtonInput<KeyCode>` (with its repeats) until the key is released, so
/// the viewer's and the shell's shortcuts, which read it, never see it: a
/// Tab-focused button takes Space, and play/pause stays put. Runs in
/// PreUpdate once Bevy has dispatched the frame's keys.
pub(crate) fn hide_taken_keys(
    mut input: MessageReader<KeyboardInput>,
    mut lost: MessageReader<KeyboardFocusLost>,
    windows: Query<(), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut state: ResMut<WidgetKeys>,
) {
    let reached = std::mem::take(&mut state.reached);
    if lost.read().count() > 0 {
        // Every key was released.
        state.taken.clear();
    }
    if windows.is_empty() {
        // Without a window Bevy dispatches no key, so none was taken.
        input.clear();
        return;
    }
    let mut hide = state.taken.clone();
    for ev in input.read() {
        let k = ev.key_code;
        match ev.state {
            ButtonState::Pressed if !ev.repeat && !reached.contains(&k) => {
                if !state.taken.contains(&k) {
                    state.taken.push(k);
                }
                if !hide.contains(&k) {
                    hide.push(k);
                }
            }
            ButtonState::Released => state.taken.retain(|t| *t != k),
            ButtonState::Pressed => {}
        }
    }
    for k in hide {
        keys.reset(k);
    }
}

/// A tab stop taken out of the Tab order while it is disabled, with the
/// index it gets back.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct DisabledTabStop(i32);

/// A control that becomes disabled (`InteractionDisabled`) leaves the Tab
/// order (`TabIndex(-1)`), as a disabled HTML button does.
pub(crate) fn skip_disabled(
    ev: On<Add<InteractionDisabled>>,
    mut stops: Query<&mut TabIndex>,
    mut commands: Commands,
) {
    if let Ok(mut index) = stops.get_mut(ev.entity) {
        if index.0 >= 0 {
            commands
                .entity(ev.entity)
                .try_insert(DisabledTabStop(index.0));
            index.0 = -1;
        }
    }
}

/// An enabled control gets its place in the Tab order back.
pub(crate) fn restore_enabled(
    ev: On<Remove<InteractionDisabled>>,
    mut stops: Query<(&mut TabIndex, &DisabledTabStop)>,
    mut commands: Commands,
) {
    if let Ok((mut index, stop)) = stops.get_mut(ev.entity) {
        index.0 = stop.0;
        commands.entity(ev.entity).try_remove::<DisabledTabStop>();
    }
}

/// A row or column of controls that the arrow keys move between (spec
/// §6.5), laid out as its orientation says: ← and → in a horizontal one,
/// ↑ and ↓ in a vertical one, Home and End to the ends. Moving only
/// focuses; Space and Enter act (DECISIONS.md D-58a, D-117). A radio group
/// needs none: the arrows move through its buttons anyway.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrowGroup(pub Orientation);

/// Where an arrow key goes in a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Previous,
    Next,
    First,
    Last,
}

impl Step {
    /// The step `key` takes in a group laid out along `along` (a radio
    /// group, `None`, takes every arrow, as Bevy's does).
    fn of(key: KeyCode, along: Option<Orientation>) -> Option<Step> {
        use Orientation::{Horizontal as Row, Vertical as Column};
        Some(match (key, along) {
            (KeyCode::Home, _) => Step::First,
            (KeyCode::End, _) => Step::Last,
            (KeyCode::ArrowLeft, Some(Row) | None) | (KeyCode::ArrowUp, Some(Column) | None) => {
                Step::Previous
            }
            (KeyCode::ArrowRight, Some(Row) | None) | (KeyCode::ArrowDown, Some(Column) | None) => {
                Step::Next
            }
            _ => return None,
        })
    }

    /// The index this step lands on from `at` of `n`, wrapping round.
    fn from(self, at: usize, n: usize) -> usize {
        match self {
            Step::Previous => (at + n - 1) % n,
            Step::Next => (at + 1) % n,
            Step::First => 0,
            Step::Last => n - 1,
        }
    }
}

/// The modifier keys: an arrow with one is the shortcuts' (⌘→ steps).
const MODIFIERS: [KeyCode; 8] = [
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
];

/// What a group's control is to the arrows: its tab index, whether it
/// shows, is a radio button, is disabled.
type StopState = (
    &'static TabIndex,
    Option<&'static InheritedVisibility>,
    Has<RadioButton>,
    Has<InteractionDisabled>,
);

/// A group the arrows move within, and whether it is a radio group.
type GroupKind = (Option<&'static ArrowGroup>, Has<RadioGroup>);
type AnyGroup = Or<(With<ArrowGroup>, With<RadioGroup>)>;

/// A group's controls the arrows land on: its enabled tab stops that Tab
/// would reach (shown), in tree order; a radio group's radio buttons.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct GroupStops<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    stops: Query<'w, 's, StopState>,
}

impl GroupStops<'_, '_> {
    fn of(&self, group: Entity, radio: bool) -> Vec<Entity> {
        self.children
            .iter_descendants_depth_first(group)
            .filter(|e| {
                self.stops
                    .get(*e)
                    .is_ok_and(|(index, shown, is_radio, off)| {
                        index.0 >= 0
                            && shown.is_none_or(|v| v.get())
                            && !off
                            && (is_radio || !radio)
                    })
            })
            .collect()
    }
}

/// The arrow keys within a group (spec §6.5, DECISIONS.md D-117): a plain
/// arrow on a control in an [`ArrowGroup`] or a radio group focuses the
/// group's next or previous control, wrapping; Home and End its first and
/// last. In a radio group the control it lands on is checked too (Bevy's
/// radio group checks from the checked button, leaving focus where it
/// was). The key stops there, so the shortcuts don't see it. An arrow with
/// a modifier is the shortcuts' even in a radio group, whose Bevy handler
/// would take it: it goes no further, and counts as having reached the
/// window. Acts where the key starts (the focused control), before the
/// group's own handler sees it; a text field's keys are its own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arrow_keys(
    mut ev: On<FocusedInput<KeyboardInput>>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    keys: Res<ButtonInput<KeyCode>>,
    groups: Query<GroupKind, AnyGroup>,
    inputs: Query<(), With<TextInput>>,
    checked: Query<(), With<Checked>>,
    parents: Query<&ChildOf>,
    stops: GroupStops,
    mut widget_keys: ResMut<WidgetKeys>,
    mut commands: Commands,
) {
    let here = ev.focused_entity;
    let input = &ev.input;
    if focus.get() != Some(here)
        || input.state != ButtonState::Pressed
        || input.repeat
        || inputs.contains(here)
    {
        return;
    }
    let key = input.key_code;
    let Some((group, (layout, radio))) = parents
        .iter_ancestors(here)
        .find_map(|a| groups.get(a).ok().map(|g| (a, g)))
    else {
        return;
    };
    let along = layout.filter(|_| !radio).map(|g| g.0);
    let Some(step) = Step::of(key, along) else {
        return;
    };
    if keys.any_pressed(MODIFIERS) {
        if radio {
            ev.propagate(false);
            widget_keys.reached.push(key);
        }
        return;
    }
    let controls = stops.of(group, radio);
    let Some(at) = controls.iter().position(|c| *c == here) else {
        return;
    };
    ev.propagate(false);
    let next = controls[step.from(at, controls.len())];
    if next != here {
        focus.set(next, FocusCause::Navigated);
    }
    if !visible.0 {
        visible.0 = true;
    }
    if radio && !checked.contains(next) {
        commands.trigger(ValueChange::<bool> {
            source: next,
            value: true,
            is_final: true,
        });
        commands.trigger(ValueChange::<Entity> {
            source: group,
            value: next,
            is_final: true,
        });
    }
}
