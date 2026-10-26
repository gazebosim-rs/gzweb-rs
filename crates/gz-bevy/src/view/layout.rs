//! Letters by the keyboard layout (DECISIONS.md D-119): a letter shortcut
//! is the key that types the letter, so G toggles the grid on the key
//! printed G on AZERTY, QWERTZ or Dvorak alike. `ButtonInput<KeyCode>` says
//! which physical keys are down; [`KeyLayout`] remembers what each one
//! typed as it went down (from Bevy's `KeyboardInput`, its `logical_key`),
//! and [`Keys`] reads both for the [`InputMap`](super::InputMap).
//!
//! What counts as a letter ([`typed_letter`]): a Latin letter typed, in
//! either case, is that letter. Another ASCII character (`,` on AZERTY's
//! `KeyM`) is no letter. Anything else (Cyrillic, Greek, a dead key, a key
//! with no character) falls back to the key's own US letter, so ⌘K and G
//! keep their places on a Russian layout, as other apps do. Positional
//! bindings ([`POSITIONAL`](super::input::POSITIONAL): the fly keys and
//! the Teleop drive keys) and keys that aren't letters (digits, arrows,
//! Space) match the physical key, as before.

use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::{ButtonInput, ButtonState};
use bevy::prelude::*;

use super::input::{dom_code, key_name, KeyMods, LETTERS};

/// What each key typed, learned from the presses (and, in Chromium
/// browsers, the page's keyboard layout map). Keys never pressed type their
/// own US letter.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyLayout {
    /// The letter each key typed as it last went down; `None` for a key
    /// that typed something else.
    letters: Vec<(KeyCode, Option<KeyCode>)>,
    /// The character each key types without Ctrl, Alt or ⌘, as its cap
    /// shows it (`Z` on AZERTY's `KeyW`), for the positional keys' labels.
    caps: Vec<(KeyCode, char)>,
}

/// A US layout: every key types its own letter.
pub static US: KeyLayout = KeyLayout {
    letters: Vec::new(),
    caps: Vec::new(),
};

impl KeyLayout {
    /// Notes `physical` going down, typing `logical`, with Ctrl, Alt or ⌘
    /// held (`command`) or not. Whether that changed what it types.
    pub fn press(&mut self, physical: KeyCode, logical: &Key, command: bool) -> bool {
        let typed = match logical {
            Key::Character(s) => Some(s.as_str()),
            _ => None,
        };
        let before = self.letters.iter().find(|(k, _)| *k == physical);
        let letter = typed_letter(physical, typed, command, before.map(|(_, l)| *l));
        let mut changed = set(&mut self.letters, physical, letter);
        if let (false, Some(cap)) = (command, typed.and_then(cap)) {
            changed |= set(&mut self.caps, physical, cap);
        }
        changed
    }

    /// Learns what `physical` types without modifiers (`typed`, as a
    /// `KeyboardEvent.key` or a layout map has it). Whether it changed.
    pub fn learn(&mut self, physical: KeyCode, typed: &str) -> bool {
        self.press(physical, &Key::Character(typed.into()), false)
    }

    /// The letter `physical` types for the shortcuts: as it last went
    /// down, else its own (a US layout); `None` for a key that isn't one.
    pub fn letter(&self, physical: KeyCode) -> Option<KeyCode> {
        match self.letters.iter().find(|(k, _)| *k == physical) {
            Some((_, letter)) => *letter,
            None => own_letter(physical),
        }
    }

    /// Whether pressing `physical` types a binding's `key`: by the letter
    /// it types, unless the binding is positional or `key` isn't a letter;
    /// then the key itself, and for a digit the keypad's too.
    pub fn types(&self, physical: KeyCode, key: KeyCode, positional: bool) -> bool {
        if positional || own_letter(key).is_none() {
            physical == key || super::input::numpad(key) == Some(physical)
        } else {
            self.letter(physical) == Some(key)
        }
    }

    /// What `physical`'s cap shows, as far as the layout is known: the
    /// character it types (`Z` for AZERTY's `KeyW`, `'` for Dvorak's
    /// `KeyQ`), else its name ([`key_name`]).
    pub fn cap(&self, physical: KeyCode) -> String {
        match self.caps.iter().find(|(k, _)| *k == physical) {
            Some((_, c)) => c.to_string(),
            None => key_name(physical),
        }
    }
}

/// Sets `key`'s entry; whether that changed it.
fn set<T: PartialEq>(list: &mut Vec<(KeyCode, T)>, key: KeyCode, value: T) -> bool {
    match list.iter_mut().find(|(k, _)| *k == key) {
        Some((_, v)) if *v == value => false,
        Some((_, v)) => {
            *v = value;
            true
        }
        None => {
            list.push((key, value));
            true
        }
    }
}

/// `key` if it is a letter key (`KeyA` to `KeyZ`).
fn own_letter(key: KeyCode) -> Option<KeyCode> {
    LETTERS.contains(&key).then_some(key)
}

/// One printable character, as a cap shows it (upper case).
fn cap(typed: &str) -> Option<char> {
    let mut chars = typed.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && !c.is_whitespace() => {
            Some(c.to_uppercase().next().unwrap_or(c))
        }
        _ => None,
    }
}

/// The letter a press of `physical` types for the shortcuts, from the
/// character the layout gives it (`typed`; `None` for a named or dead
/// key): a Latin letter in either case is that letter's key; another ASCII
/// character is no letter; anything else falls back to the key's own
/// letter. With Ctrl, Alt or ⌘ held (`command`) a character that isn't a
/// Latin letter says nothing about the layout (Option+W types `∑` on a
/// Mac), so the key types what it typed `before`, if that is known.
pub fn typed_letter(
    physical: KeyCode,
    typed: Option<&str>,
    command: bool,
    before: Option<Option<KeyCode>>,
) -> Option<KeyCode> {
    let mut chars = typed.unwrap_or_default().chars();
    let one = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    match one {
        Some(c) if c.is_ascii_alphabetic() => {
            Some(LETTERS[(c.to_ascii_lowercase() as u8 - b'a') as usize])
        }
        _ if command => before.unwrap_or_else(|| own_letter(physical)),
        Some(c) if c.is_ascii() => None,
        _ => own_letter(physical),
    }
}

/// The letter a browser `keydown` types for the shortcuts, from its `code`
/// (`KeyQ`) and `key` (`a`), as [`typed_letter`] reads a press: the page's
/// canvas listener has no [`KeyLayout`] of its own.
pub fn dom_letter(code: &str, key: &str, command: bool) -> Option<KeyCode> {
    // A key that isn't a letter key types a letter only as a Latin letter
    // (AZERTY's `m` is on `Semicolon`).
    let physical = LETTERS
        .iter()
        .copied()
        .find(|k| dom_code(*k) == code)
        .unwrap_or(KeyCode::Unidentified(NativeKeyCode::Unidentified));
    typed_letter(physical, Some(key), command, None)
}

/// The keys as the shortcuts read them: which are down, and the letters
/// they type.
#[derive(Clone, Copy)]
pub struct Keys<'a> {
    pub input: &'a ButtonInput<KeyCode>,
    pub layout: &'a KeyLayout,
}

impl<'a> Keys<'a> {
    /// `input` as `layout` types it; without one, as a US layout does.
    pub fn new(input: &'a ButtonInput<KeyCode>, layout: Option<&'a KeyLayout>) -> Self {
        Keys {
            input,
            layout: layout.unwrap_or(&US),
        }
    }

    /// The modifiers held.
    pub fn mods(&self) -> KeyMods {
        KeyMods::held(self.input)
    }

    /// The physical key that typed `key` this frame ([`KeyLayout::types`]).
    pub fn just_pressed(&self, key: KeyCode, positional: bool) -> Option<KeyCode> {
        self.input
            .get_just_pressed()
            .copied()
            .find(|p| self.layout.types(*p, key, positional))
    }

    /// The physical keys down that type `key`.
    pub fn pressed(&self, key: KeyCode, positional: bool) -> impl Iterator<Item = KeyCode> + 'a {
        let layout = self.layout;
        self.input
            .get_pressed()
            .copied()
            .filter(move |p| layout.types(*p, key, positional))
    }

    /// Whether a key that types the letter `key` went down this frame
    /// (the gizmo's X, Y and Z, Teleop's X).
    pub fn letter_just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed(key, false).is_some()
    }
}

/// Notes what each key press this frame types ([`KeyLayout`]), once Bevy
/// has read the frame's keys and before the shortcuts do.
pub fn learn_layout(
    mut input: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut layout: ResMut<KeyLayout>,
) {
    let command = KeyMods::held(&keys).command();
    let mut changed = false;
    for ev in input.read().filter(|e| e.state == ButtonState::Pressed) {
        changed |= layout
            .bypass_change_detection()
            .press(ev.key_code, &ev.logical_key, command);
    }
    if changed {
        layout.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Physical keys and what AZERTY types on them, unshifted.
    const AZERTY: &[(KeyCode, &str)] = &[
        (KeyCode::KeyQ, "a"),
        (KeyCode::KeyW, "z"),
        (KeyCode::KeyA, "q"),
        (KeyCode::KeyZ, "w"),
        (KeyCode::KeyM, ","),
        (KeyCode::Semicolon, "m"),
        (KeyCode::Digit1, "&"),
    ];

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    #[test]
    fn a_latin_letter_is_its_key_in_either_case() {
        let t = |p, s, cmd| typed_letter(p, Some(s), cmd, None);
        assert_eq!(t(KeyCode::KeyQ, "a", false), Some(KeyCode::KeyA));
        assert_eq!(t(KeyCode::KeyQ, "A", false), Some(KeyCode::KeyA));
        assert_eq!(t(KeyCode::KeyV, "k", true), Some(KeyCode::KeyK));
        // Another ASCII character is no letter: AZERTY's `KeyM`.
        assert_eq!(t(KeyCode::KeyM, ",", false), None);
        assert_eq!(t(KeyCode::Digit2, "é", false), None);
        // Cyrillic, an accented letter, a dead key, nothing: the key's own.
        assert_eq!(t(KeyCode::KeyK, "л", false), Some(KeyCode::KeyK));
        assert_eq!(t(KeyCode::KeyE, "é", false), Some(KeyCode::KeyE));
        assert_eq!(
            typed_letter(KeyCode::KeyN, None, false, None),
            Some(KeyCode::KeyN)
        );
        assert_eq!(t(KeyCode::KeyA, "ab", false), Some(KeyCode::KeyA));
        // With a command modifier, what it typed before.
        let before = Some(Some(KeyCode::KeyZ));
        assert_eq!(
            typed_letter(KeyCode::KeyW, Some("∑"), true, before),
            Some(KeyCode::KeyZ)
        );
        assert_eq!(
            typed_letter(KeyCode::KeyL, Some("@"), true, None),
            Some(KeyCode::KeyL)
        );
        assert_eq!(
            typed_letter(KeyCode::KeyM, Some("µ"), true, Some(None)),
            None
        );
    }

    #[test]
    fn the_layout_learns_from_presses_and_says_when_it_changed() {
        let mut layout = KeyLayout::default();
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyQ));
        assert_eq!(layout.letter(KeyCode::Digit1), None);
        for (k, s) in AZERTY {
            assert!(layout.learn(*k, s), "{k:?}");
        }
        assert!(!layout.learn(KeyCode::KeyQ, "a"));
        assert!(!layout.press(KeyCode::KeyQ, &ch("A"), false));
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyA));
        assert_eq!(layout.letter(KeyCode::Semicolon), Some(KeyCode::KeyM));
        assert_eq!(layout.letter(KeyCode::KeyM), None);
        // Option+W on a Mac AZERTY: still Z.
        assert!(!layout.press(KeyCode::KeyW, &ch("∑"), true));
        assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyZ));
        // A command press doesn't change the cap.
        assert!(layout.press(KeyCode::KeyE, &ch("€"), true));
        assert_eq!(layout.cap(KeyCode::KeyE), "E");
        assert_eq!(layout.cap(KeyCode::KeyW), "Z");
        assert_eq!(layout.cap(KeyCode::KeyM), ",");
        assert_eq!(layout.cap(KeyCode::ArrowUp), "Up");
        // A dead key has no cap and its own letter.
        assert!(layout.press(KeyCode::KeyY, &Key::Dead(Some('^')), false));
        assert_eq!(layout.cap(KeyCode::KeyY), "Y");
        // Switching to Russian: the keys are their own letters again.
        assert!(layout.learn(KeyCode::KeyQ, "й"));
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyQ));
        assert_eq!(layout.cap(KeyCode::KeyQ), "Й");
    }

    #[test]
    fn letters_match_by_layout_and_positions_by_key() {
        let mut layout = KeyLayout::default();
        for (k, s) in AZERTY {
            layout.learn(*k, s);
        }
        assert!(layout.types(KeyCode::KeyQ, KeyCode::KeyA, false));
        assert!(!layout.types(KeyCode::KeyA, KeyCode::KeyA, false));
        assert!(layout.types(KeyCode::KeyA, KeyCode::KeyA, true));
        // Digits by position, the keypad's too.
        assert!(layout.types(KeyCode::Digit1, KeyCode::Digit1, false));
        assert!(layout.types(KeyCode::Numpad1, KeyCode::Digit1, false));
        assert!(!layout.types(KeyCode::KeyM, KeyCode::KeyM, false));
        assert!(layout.types(KeyCode::Semicolon, KeyCode::KeyM, false));
    }

    #[test]
    fn a_browser_keydown_types_as_a_press_does() {
        assert_eq!(dom_letter("KeyQ", "a", false), Some(KeyCode::KeyA));
        assert_eq!(dom_letter("KeyK", "K", true), Some(KeyCode::KeyK));
        assert_eq!(dom_letter("KeyK", "л", true), Some(KeyCode::KeyK));
        assert_eq!(dom_letter("KeyM", ",", false), None);
        assert_eq!(dom_letter("Semicolon", "m", false), Some(KeyCode::KeyM));
        assert_eq!(dom_letter("Semicolon", "ö", false), None);
        assert_eq!(dom_letter("Space", " ", false), None);
        assert_eq!(dom_letter("ArrowRight", "ArrowRight", true), None);
    }

    #[test]
    fn keys_read_the_input_through_the_layout() {
        let mut input = ButtonInput::<KeyCode>::default();
        let mut layout = KeyLayout::default();
        layout.learn(KeyCode::KeyQ, "a");
        input.press(KeyCode::KeyQ);
        let keys = Keys::new(&input, Some(&layout));
        assert_eq!(keys.just_pressed(KeyCode::KeyA, false), Some(KeyCode::KeyQ));
        assert_eq!(keys.just_pressed(KeyCode::KeyQ, false), None);
        assert_eq!(keys.just_pressed(KeyCode::KeyQ, true), Some(KeyCode::KeyQ));
        assert!(keys.letter_just_pressed(KeyCode::KeyA));
        assert_eq!(
            keys.pressed(KeyCode::KeyA, false).collect::<Vec<_>>(),
            [KeyCode::KeyQ]
        );
        // Without a layout: US.
        let us = Keys::new(&input, None);
        assert!(us.letter_just_pressed(KeyCode::KeyQ));
        assert_eq!(us.mods(), KeyMods::default());
    }

    #[test]
    fn learning_runs_on_the_frames_presses() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<KeyboardInput>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<KeyLayout>();
        let press = |key_code, logical_key, state| KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        world.write_message(press(KeyCode::KeyQ, ch("a"), ButtonState::Pressed));
        world.write_message(press(KeyCode::KeyW, ch("z"), ButtonState::Released));
        world.run_system_once(learn_layout).unwrap();
        let layout = world.resource::<KeyLayout>();
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyA));
        assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyW));
        // With Ctrl held, a character that isn't a letter teaches nothing.
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        world.write_message(press(KeyCode::KeyE, ch("\u{5}"), ButtonState::Pressed));
        world.run_system_once(learn_layout).unwrap();
        assert_eq!(
            world.resource::<KeyLayout>().letter(KeyCode::KeyE),
            Some(KeyCode::KeyE)
        );
    }
}
