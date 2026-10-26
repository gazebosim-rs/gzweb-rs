//! Keys pressed at start, for a capture (`gzweb --keys tab,tab,down`):
//! once the world is live, each chord is pressed and released in turn, so
//! a screenshot can show where keyboard focus went (DECISIONS.md D-117).

use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

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
    if phase.0 != Phase::Live {
        return;
    }
    let mut send = |key_code: KeyCode, state: ButtonState| {
        input.write(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    };
    if let Some(chord) = pressing.down.take() {
        for k in chord.into_iter().rev() {
            send(k, ButtonState::Released);
        }
        if keys.0.is_empty() {
            commands.remove_resource::<StartKeys>();
        }
        return;
    }
    pressing.frames += 1;
    if pressing.frames < SETTLE_FRAMES || keys.0.is_empty() {
        return;
    }
    let chord = keys.0.remove(0);
    for k in &chord {
        send(*k, ButtonState::Pressed);
    }
    pressing.down = Some(chord);
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(First, press_start_keys);
}
