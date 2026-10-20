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

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;
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
