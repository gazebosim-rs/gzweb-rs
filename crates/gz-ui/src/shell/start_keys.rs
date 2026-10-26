//! Keys pressed at start, for a capture (`gzweb --keys tab,tab,down`):
//! once the world is live, each chord is pressed and released in turn, so
//! a screenshot can show where keyboard focus went (DECISIONS.md D-117).

use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gz_bevy::snapshot::SnapshotHold;

use super::{CurrentPhase, Phase};

/// The chords to press once the world is live, first to last.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StartKeys(pub Vec<Vec<KeyCode>>);

impl StartKeys {
    /// Reads `tab,shift+tab,down`: chords by commas, keys in a chord by
    /// `+` (modifiers first). Names: the arrows (`up`, `down`, `left`,
    /// `right`), `tab`, `home`, `end`, `enter`, `space`, `esc`, `shift`,
    /// `ctrl`, `alt`, `cmd`, and the letters.
    pub fn parse(s: &str) -> Result<Self, String> {
        let chord = |c: &str| -> Result<Vec<KeyCode>, String> {
            c.split('+')
                .map(|k| key_named(k.trim()).ok_or_else(|| format!("no key '{}'", k.trim())))
                .collect()
        };
        let chords = s
            .split(',')
            .filter(|c| !c.trim().is_empty())
            .map(chord)
            .collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err("no keys".into());
        }
        Ok(Self(chords))
    }
}

/// The key a `--keys` name stands for.
fn key_named(name: &str) -> Option<KeyCode> {
    let name = name.to_ascii_lowercase();
    Some(match name.as_str() {
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        "tab" => KeyCode::Tab,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "enter" => KeyCode::Enter,
        "space" => KeyCode::Space,
        "esc" => KeyCode::Escape,
        "shift" => KeyCode::ShiftLeft,
        "ctrl" => KeyCode::ControlLeft,
        "alt" => KeyCode::AltLeft,
        "cmd" => KeyCode::SuperLeft,
        _ => return letter(&name),
    })
}

fn letter(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    c.is_ascii_lowercase()
        .then(|| LETTERS[c as usize - 'a' as usize])
}

/// Frames to wait once live before the first chord: the panels' bodies
/// and the views are built by then.
pub(crate) const SETTLE_FRAMES: u32 = 10;

/// Where [`press_start_keys`] is.
#[derive(Default)]
pub(crate) struct Pressing {
    /// Live frames so far.
    frames: u32,
    /// The chord down now, released next frame.
    down: Option<Vec<KeyCode>>,
    /// A screenshot is held until the keys are pressed.
    held: bool,
}

/// What `key` means and types, as a keyboard would send it (`shift` held
/// or not): a letter its character, the named keys theirs.
fn logical(key: KeyCode, shift: bool) -> (Key, Option<String>) {
    let named = |k: Key| (k, None);
    match key {
        KeyCode::ArrowUp => named(Key::ArrowUp),
        KeyCode::ArrowDown => named(Key::ArrowDown),
        KeyCode::ArrowLeft => named(Key::ArrowLeft),
        KeyCode::ArrowRight => named(Key::ArrowRight),
        KeyCode::Tab => named(Key::Tab),
        KeyCode::Home => named(Key::Home),
        KeyCode::End => named(Key::End),
        KeyCode::Enter => (Key::Enter, Some("\r".to_string())),
        KeyCode::Space => (Key::Space, Some(" ".to_string())),
        KeyCode::Escape => named(Key::Escape),
        KeyCode::ShiftLeft => named(Key::Shift),
        KeyCode::ControlLeft => named(Key::Control),
        KeyCode::AltLeft => named(Key::Alt),
        KeyCode::SuperLeft => named(Key::Super),
        _ => {
            let Some(c) = letter_of(key) else {
                return named(Key::Unidentified(NativeKey::Unidentified));
            };
            let c = if shift { c.to_ascii_uppercase() } else { c }.to_string();
            (Key::Character(c.as_str().into()), Some(c))
        }
    }
}

/// The letter `key` types, for the letters.
fn letter_of(key: KeyCode) -> Option<char> {
    ('a'..='z').find(|c| letter(&c.to_string()) == Some(key))
}

/// Presses the [`StartKeys`] once the world is live: a chord's keys down
/// in one frame (as typed, modifiers first), up in the next, then the
/// next chord. Runs in `First`, before Bevy reads the frame's keys.
pub(crate) fn press_start_keys(
    keys: Option<ResMut<StartKeys>>,
    phase: Res<CurrentPhase>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut input: MessageWriter<KeyboardInput>,
    mut pressing: Local<Pressing>,
    mut commands: Commands,
) {
    let Some(mut keys) = keys else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    // A screenshot waits for the keys.
    if !pressing.held {
        pressing.held = true;
        commands.insert_resource(SnapshotHold(true));
    }
    let mut send = |key_code: KeyCode, state: ButtonState, shift: bool| {
        let (logical_key, text) = logical(key_code, shift);
        input.write(KeyboardInput {
            key_code,
            logical_key,
            state,
            // Only a press types.
            text: text
                .filter(|_| state == ButtonState::Pressed)
                .map(|t| t.as_str().into()),
            repeat: false,
            window,
        });
    };
    // A chord down is released whatever the phase (none stays held).
    if let Some(chord) = pressing.down.take() {
        let shift = chord.contains(&KeyCode::ShiftLeft);
        for k in chord.into_iter().rev() {
            send(k, ButtonState::Released, shift);
        }
        if keys.0.is_empty() {
            commands.remove_resource::<StartKeys>();
            commands.insert_resource(SnapshotHold(false));
        }
        return;
    }
    if phase.0 != Phase::Live {
        return;
    }
    pressing.frames += 1;
    if pressing.frames < SETTLE_FRAMES || keys.0.is_empty() {
        return;
    }
    let chord = keys.0.remove(0);
    let shift = chord.contains(&KeyCode::ShiftLeft);
    for k in &chord {
        send(*k, ButtonState::Pressed, shift);
    }
    pressing.down = Some(chord);
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(First, press_start_keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pressed key says what it means and types, as a keyboard's would.
    #[test]
    fn keys_carry_what_they_mean() {
        assert_eq!(logical(KeyCode::Escape, false), (Key::Escape, None));
        assert_eq!(logical(KeyCode::ArrowDown, false), (Key::ArrowDown, None));
        assert_eq!(
            logical(KeyCode::KeyG, false),
            (Key::Character("g".into()), Some("g".into()))
        );
        assert_eq!(
            logical(KeyCode::KeyG, true),
            (Key::Character("G".into()), Some("G".into()))
        );
        assert_eq!(
            logical(KeyCode::Space, false),
            (Key::Space, Some(" ".into()))
        );
        assert_eq!(logical(KeyCode::ShiftLeft, false), (Key::Shift, None));
    }
}
