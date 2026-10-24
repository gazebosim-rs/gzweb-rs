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

use bevy::ecs::change_detection::Tick;
use bevy::ecs::system::SystemChangeTick;
use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{MenuPopup, TextInput};
use bevy::window::PrimaryWindow;
use gz_bevy::view::controls::ActiveScopes;
use gz_bevy::view::input::Scope;

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
