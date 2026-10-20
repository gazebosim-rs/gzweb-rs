//! Keeping text up to date: readouts rewrite their text every frame from
//! the state they show, so the write happens only when the value differs
//! (an unchanged text isn't marked changed and isn't laid out again).

use std::ops::DerefMut;

use bevy::prelude::*;

/// Sets a [`Text`] or [`TextSpan`] (anything that is a `String` inside) to
/// `want` if it differs.
pub fn set_text<T>(text: &mut Mut<T>, want: &str)
where
    T: DerefMut<Target = String>,
{
    if text.as_str() != want {
        let s: &mut String = text;
        want.clone_into(s);
    }
}
