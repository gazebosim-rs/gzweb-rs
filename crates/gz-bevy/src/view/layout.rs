//! Letters by the keyboard layout (DECISIONS.md D-119): a letter shortcut
//! is the key that types the letter, so G toggles the grid on the key
//! printed G on AZERTY, QWERTZ or Dvorak alike. `ButtonInput<KeyCode>` says
//! which physical keys are down; [`KeyLayout`] remembers what each one
//! typed as it went down (from Bevy's `KeyboardInput`, its `logical_key`),
//! and [`Keys`] reads both for the [`InputMap`](super::InputMap).
//!
//! What counts as a letter ([`typed_letter`]): a Latin letter typed, in
//! either case, is that letter. Another ASCII character (`,` on AZERTY's
//! `KeyM`) is no letter. Anything else (Cyrillic, Greek, an accented
//! letter) falls back to the key's own US letter, so ⌘K and G keep their
//! places on a Russian layout, as other apps do; a dead key or a key with
//! no character says nothing new. A letter of another script typed without
//! Alt (Russian's `ы`) means the user switched layouts, and so does a
//! Latin letter typed plain after one: what was learned belonged to the
//! other layout and is forgotten. The browser's canvas listener keeps a
//! [`KeyLayout`] of its own, fed the same keydowns by the same rules, so
//! what it swallows is what the app runs. Positional
//! bindings ([`POSITIONAL`](super::input::POSITIONAL): the fly keys and
//! the Teleop drive keys) and keys that aren't letters (digits, arrows,
//! Space) match the physical key, as before.

use bevy::input::keyboard::{Key, KeyCode, KeyboardFocusLost, KeyboardInput, NativeKey};
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
    /// The character each key types with no modifier at all, as its cap
    /// shows it (`Z` on AZERTY's `KeyW`), for the positional keys' labels.
    caps: Vec<(KeyCode, char)>,
    /// The layout in use types another script than Latin (Russian).
    foreign: bool,
}

/// A US layout: every key types its own letter.
pub static US: KeyLayout = KeyLayout {
    letters: Vec::new(),
    caps: Vec::new(),
    foreign: false,
};

impl KeyLayout {
    /// Notes `physical` going down, typing `logical`, with `mods` held.
    /// Whether that changed what the keys type.
    pub fn press(&mut self, physical: KeyCode, logical: &Key, mods: KeyMods) -> bool {
        let typed = match logical {
            Key::Character(s) => Some(s.as_str()),
            _ => None,
        };
        let mut changed = false;
        // Alt (and AltGr, Ctrl+Alt) types symbols of any script (Option+P
        // is π on a Mac); Ctrl and ⌘ leave the layout's letter, so they
        // show a switch too.
        if let Some(c) = typed.and_then(one_char).filter(|_| !mods.alt) {
            let foreign = c.is_alphabetic() && !is_latin(c);
            let latin = c.is_ascii_alphabetic() && !mods.command();
            if (foreign && !self.foreign) || (latin && self.foreign) {
                changed = !(self.letters.is_empty() && self.caps.is_empty());
                self.letters.clear();
                self.caps.clear();
                self.foreign = foreign;
            }
        }
        let before = self.letters.iter().find(|(k, _)| *k == physical);
        let letter = typed_letter(physical, typed, mods.command(), before.map(|(_, l)| *l));
        changed |= set(&mut self.letters, physical, letter);
        let plain = mods == KeyMods::default();
        if let (true, Some(cap)) = (plain, typed.and_then(cap)) {
            changed |= set(&mut self.caps, physical, cap);
        }
        changed
    }

    /// Learns what `physical` types without modifiers (`typed`, as a
    /// `KeyboardEvent.key` or a layout map has it). Whether it changed.
    pub fn learn(&mut self, physical: KeyCode, typed: &str) -> bool {
        self.press(physical, &dom_key(typed), KeyMods::default())
    }

    /// The physical keys that type the letter `key`: those learned to,
    /// and the key itself while nothing is known of it.
    pub fn typing(&self, key: KeyCode) -> impl Iterator<Item = KeyCode> + '_ {
        let own = own_letter(key).filter(|k| !self.letters.iter().any(|(p, _)| p == k));
        self.letters
            .iter()
            .filter(move |(_, l)| *l == Some(key))
            .map(|(p, _)| *p)
            .chain(own)
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

/// `typed` if it is one character.
fn one_char(typed: &str) -> Option<char> {
    let mut chars = typed.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// One printable character, as a cap shows it (upper case).
fn cap(typed: &str) -> Option<char> {
    one_char(typed)
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .map(|c| c.to_uppercase().next().unwrap_or(c))
}

/// Whether `c` is written in the Latin script (accented letters too).
fn is_latin(c: char) -> bool {
    c <= '\u{24F}'
        || ('\u{1E00}'..='\u{1EFF}').contains(&c)
        || ('\u{2C60}'..='\u{2C7F}').contains(&c)
        || ('\u{A720}'..='\u{A7FF}').contains(&c)
        || ('\u{AB30}'..='\u{AB6F}').contains(&c)
}

/// A `KeyboardEvent.key` as Bevy's logical key: one character, or a key
/// without one (`Enter`, `Dead`).
pub fn dom_key(key: &str) -> Key {
    match one_char(key) {
        Some(_) => Key::Character(key.into()),
        None if key == "Dead" => Key::Dead(None),
        None => Key::Unidentified(NativeKey::Unidentified),
    }
}

/// The key a `KeyboardEvent.code` names, among those that can type a
/// letter on some layout (the letters, digits and punctuation: AZERTY's
/// `m` is on `Semicolon`, Dvorak's `z` on `Slash`).
pub fn dom_physical(code: &str) -> Option<KeyCode> {
    const OTHERS: [KeyCode; 22] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Semicolon,
        KeyCode::Quote,
        KeyCode::Comma,
        KeyCode::Period,
        KeyCode::Slash,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Backslash,
        KeyCode::Backquote,
        KeyCode::Minus,
        KeyCode::Equal,
        KeyCode::IntlBackslash,
    ];
    LETTERS
        .iter()
        .chain(OTHERS.iter())
        .copied()
        .find(|k| dom_code(*k) == code)
}

/// The letter a press of `physical` types for the shortcuts, from the
/// character the layout gives it (`typed`; `None` for a named or dead
/// key): a Latin letter in either case is that letter's key; another ASCII
/// character is no letter; any other character falls back to the key's own
/// letter. No character (a dead key, `Unidentified`) says nothing about
/// the layout, and with Ctrl, Alt or ⌘ held (`command`) neither does a
/// character that isn't a Latin letter (Option+W types `∑` on a Mac): the
/// key types what it typed `before`, if that is known, else its own.
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
        None => before.unwrap_or_else(|| own_letter(physical)),
        Some(c) if c.is_ascii() => None,
        _ => own_letter(physical),
    }
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
/// has read the frame's keys and before the shortcuts do. The modifiers
/// are followed through the presses and releases in order (`held`), so
/// Option let go in the frame of Option+W still held W.
pub fn learn_layout(
    mut input: MessageReader<KeyboardInput>,
    mut lost: MessageReader<KeyboardFocusLost>,
    mut layout: ResMut<KeyLayout>,
    mut held: Local<KeyMods>,
) {
    if lost.read().count() > 0 {
        *held = KeyMods::default();
    }
    let mut changed = false;
    for ev in input.read() {
        let down = ev.state == ButtonState::Pressed;
        if !held.note(ev.key_code, down) && down {
            changed |= layout
                .bypass_change_detection()
                .press(ev.key_code, &ev.logical_key, *held);
        }
    }
    if changed {
        layout.set_changed();
    }
}

/// Keeps the input map's [`unreachable`](super::InputMap::unreachable)
/// letters up with the layout, so labels don't offer them.
pub fn mark_unreachable(layout: Res<KeyLayout>, map: Option<ResMut<super::InputMap>>) {
    let Some(mut map) = map.filter(|_| layout.is_changed()) else {
        return;
    };
    let unreachable = map.unreachable_on(&layout);
    if map.unreachable != unreachable {
        map.unreachable = unreachable;
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

    fn mods(ctrl: bool, shift: bool, alt: bool) -> KeyMods {
        KeyMods {
            ctrl,
            shift,
            alt,
            super_key: false,
        }
    }

    const PLAIN: KeyMods = KeyMods {
        ctrl: false,
        shift: false,
        alt: false,
        super_key: false,
    };

    #[test]
    fn a_latin_letter_is_its_key_in_either_case() {
        let t = |p, s, cmd| typed_letter(p, Some(s), cmd, None);
        assert_eq!(t(KeyCode::KeyQ, "a", false), Some(KeyCode::KeyA));
        assert_eq!(t(KeyCode::KeyQ, "A", false), Some(KeyCode::KeyA));
        assert_eq!(t(KeyCode::KeyV, "k", true), Some(KeyCode::KeyK));
        // Another ASCII character is no letter: AZERTY's `KeyM`.
        assert_eq!(t(KeyCode::KeyM, ",", false), None);
        assert_eq!(t(KeyCode::Digit2, "é", false), None);
        // Cyrillic, an accented letter: the key's own.
        assert_eq!(t(KeyCode::KeyK, "л", false), Some(KeyCode::KeyK));
        assert_eq!(t(KeyCode::KeyE, "é", false), Some(KeyCode::KeyE));
        // No character: what it typed before, else its own.
        assert_eq!(
            typed_letter(KeyCode::KeyN, None, false, None),
            Some(KeyCode::KeyN)
        );
        assert_eq!(
            typed_letter(KeyCode::KeyN, None, false, Some(Some(KeyCode::KeyB))),
            Some(KeyCode::KeyB)
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
        assert!(!layout.press(KeyCode::KeyQ, &ch("A"), mods(false, true, false)));
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyA));
        assert_eq!(layout.letter(KeyCode::Semicolon), Some(KeyCode::KeyM));
        assert_eq!(layout.letter(KeyCode::KeyM), None);
        // Option+W on a Mac AZERTY: still Z, and the cap stays.
        assert!(!layout.press(KeyCode::KeyW, &ch("∑"), mods(false, false, true)));
        assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyZ));
        assert!(layout.press(KeyCode::KeyE, &ch("€"), mods(true, false, true)));
        assert_eq!(layout.cap(KeyCode::KeyE), "E");
        assert_eq!(layout.cap(KeyCode::KeyW), "Z");
        assert_eq!(layout.cap(KeyCode::KeyM), ",");
        assert_eq!(layout.cap(KeyCode::ArrowUp), "Up");
        // A dead key has no cap, and an unidentified press keeps the letter.
        assert!(layout.press(KeyCode::KeyY, &Key::Dead(Some('^')), PLAIN));
        assert_eq!(layout.cap(KeyCode::KeyY), "Y");
        let unknown = Key::Unidentified(NativeKey::Unidentified);
        assert!(!layout.press(KeyCode::KeyQ, &unknown, PLAIN));
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyA));
        // Which keys type a letter: those learned, and an unknown key its own.
        assert_eq!(
            layout.typing(KeyCode::KeyA).collect::<Vec<_>>(),
            [KeyCode::KeyQ]
        );
        assert_eq!(
            layout.typing(KeyCode::KeyF).collect::<Vec<_>>(),
            [KeyCode::KeyF]
        );
    }

    /// Shift types other characters on the same caps: the caps are learned
    /// unshifted only (Dvorak's `KeyW` is `,`, not `<`).
    #[test]
    fn shift_leaves_the_caps_alone() {
        let mut layout = KeyLayout::default();
        layout.learn(KeyCode::KeyW, ",");
        layout.press(KeyCode::KeyW, &ch("<"), mods(false, true, false));
        assert_eq!(layout.cap(KeyCode::KeyW), ",");
        // Nor does a seed get overwritten by a shifted press of a key.
        layout.learn(KeyCode::KeyQ, "'");
        layout.press(KeyCode::KeyQ, &ch("\""), mods(false, true, false));
        assert_eq!(layout.cap(KeyCode::KeyQ), "'");
        // A shifted letter still teaches the letter.
        layout.press(KeyCode::KeyS, &ch("O"), mods(false, true, false));
        assert_eq!(layout.letter(KeyCode::KeyS), Some(KeyCode::KeyO));
        assert_eq!(layout.cap(KeyCode::KeyS), "S");
    }

    /// A letter of another script typed without Alt is a layout switch:
    /// Dvorak's `KeyS` typed O, and after Russian's `ы` (with Ctrl or
    /// without) it is S again; a plain Latin letter switches back.
    #[test]
    fn a_layout_switch_forgets_the_old_letters() {
        for first in [PLAIN, mods(true, false, false)] {
            let mut layout = KeyLayout::default();
            layout.learn(KeyCode::KeyS, "o");
            layout.learn(KeyCode::KeyW, ",");
            assert!(layout.press(KeyCode::KeyS, &ch("ы"), first));
            assert_eq!(layout.letter(KeyCode::KeyS), Some(KeyCode::KeyS));
            assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyW));
            assert_eq!(layout.cap(KeyCode::KeyW), "W");
            // Back to Dvorak.
            assert!(layout.press(KeyCode::KeyF, &ch("u"), PLAIN));
            assert_eq!(layout.letter(KeyCode::KeyF), Some(KeyCode::KeyU));
            assert_eq!(layout.letter(KeyCode::KeyS), Some(KeyCode::KeyS));
        }
        // Alt's symbols are no switch (Option+P is π on a Mac), nor are
        // accented Latin letters (AZERTY's é) or a command's Latin letter.
        let mut layout = KeyLayout::default();
        layout.learn(KeyCode::KeyS, "o");
        layout.press(KeyCode::KeyP, &ch("π"), mods(false, false, true));
        layout.learn(KeyCode::Digit2, "é");
        assert_eq!(layout.letter(KeyCode::KeyS), Some(KeyCode::KeyO));
        layout.learn(KeyCode::KeyK, "л");
        layout.press(KeyCode::KeyS, &ch("s"), mods(true, false, false));
        assert_eq!(layout.letter(KeyCode::KeyS), Some(KeyCode::KeyS));
        assert_eq!(layout.cap(KeyCode::KeyK), "Л");
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
    fn browser_codes_and_keys() {
        assert_eq!(dom_physical("KeyQ"), Some(KeyCode::KeyQ));
        assert_eq!(dom_physical("Semicolon"), Some(KeyCode::Semicolon));
        assert_eq!(dom_physical("Digit2"), Some(KeyCode::Digit2));
        assert_eq!(dom_physical("ArrowRight"), None);
        assert_eq!(dom_key("a"), ch("a"));
        assert_eq!(dom_key("Dead"), Key::Dead(None));
        assert_eq!(dom_key("Enter"), Key::Unidentified(NativeKey::Unidentified));
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

    fn event(key_code: KeyCode, logical_key: Key, state: ButtonState) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn learning_runs_on_the_frames_presses_with_their_modifiers() {
        let mut world = World::new();
        world.init_resource::<Messages<KeyboardInput>>();
        world.init_resource::<Messages<KeyboardFocusLost>>();
        world.init_resource::<KeyLayout>();
        world.write_message(event(KeyCode::KeyQ, ch("a"), ButtonState::Pressed));
        world.write_message(event(KeyCode::KeyW, ch("z"), ButtonState::Released));
        let mut system = IntoSystem::into_system(learn_layout);
        system.initialize(&mut world);
        let mut run = |world: &mut World| {
            system.run((), world).unwrap();
        };
        run(&mut world);
        let layout = world.resource::<KeyLayout>();
        assert_eq!(layout.letter(KeyCode::KeyQ), Some(KeyCode::KeyA));
        assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyW));
        // Option+W then Option up, in one frame: W was typed with Option,
        // so `∑` teaches nothing.
        world.write_message(event(KeyCode::KeyW, ch("z"), ButtonState::Pressed));
        world.write_message(event(KeyCode::AltLeft, ch("x"), ButtonState::Pressed));
        world.write_message(event(KeyCode::KeyW, ch("∑"), ButtonState::Pressed));
        world.write_message(event(KeyCode::AltLeft, ch("x"), ButtonState::Released));
        run(&mut world);
        let layout = world.resource::<KeyLayout>();
        assert_eq!(layout.letter(KeyCode::KeyW), Some(KeyCode::KeyZ));
        assert_eq!(layout.cap(KeyCode::KeyW), "Z");
        // Losing focus lets go of the modifiers.
        world.write_message(event(KeyCode::ControlLeft, ch("x"), ButtonState::Pressed));
        run(&mut world);
        world.write_message(KeyboardFocusLost);
        run(&mut world);
        world.write_message(event(KeyCode::KeyE, ch("é"), ButtonState::Pressed));
        run(&mut world);
        assert_eq!(world.resource::<KeyLayout>().cap(KeyCode::KeyE), "É");
    }
}
