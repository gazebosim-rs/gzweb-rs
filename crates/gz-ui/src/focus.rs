//! Keyboard focus in text fields (spec §13: "single-letter shortcuts do
//! nothing while a text field has focus").
//!
//! While a UI text field has focus, the viewer's key bindings are off (its
//! [`ActiveScopes`] are emptied and restored afterwards), and in the
//! browser the canvas stops preventing any key's default, so browser
//! shortcuts keep working (`gz_bevy::view::web`, spec §9.6).

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;
use gz_bevy::view::controls::ActiveScopes;
use gz_bevy::view::input::Scope;

/// A text field has keyboard focus.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextFocus(pub bool);

/// Tracks [`TextFocus`] and hands the keys to the field or back.
pub fn text_focus_system(
    focus: Res<InputFocus>,
    inputs: Query<(), With<TextInput>>,
    mut state: ResMut<TextFocus>,
    scopes: Option<ResMut<ActiveScopes>>,
    mut saved: Local<Option<Vec<Scope>>>,
) {
    let typing = focus.get().is_some_and(|e| inputs.contains(e));
    if state.0 == typing {
        return;
    }
    state.0 = typing;
    if let Some(mut scopes) = scopes {
        if typing {
            *saved = Some(std::mem::take(&mut scopes.0));
        } else if let Some(previous) = saved.take() {
            scopes.0 = previous;
        }
    }
    #[cfg(target_arch = "wasm32")]
    gz_bevy::view::web::set_text_focus(typing);
}
