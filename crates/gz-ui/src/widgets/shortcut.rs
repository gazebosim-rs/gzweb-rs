//! Shortcut text (spec §5.14, §5.15): what menus and tooltips print for a
//! `shortcuts.json` binding on this platform.

use bevy::prelude::*;
use gz_bevy::view::input::{Chord, InputMap, Platform};

/// The text a menu or tooltip shows for a key binding (`shortcuts.json`
/// id), on this platform: `⇧F` for Shift chords, else
/// `Ctrl+Shift+E` (`Cmd+…` on macOS). Geist has no ⌘ or ⌥ glyphs
/// (DECISIONS.md D-26).
pub fn shortcut_label(map: &InputMap, id: &str) -> Option<String> {
    let binding = map.binding(id)?;
    let chord = binding.chords(map.platform).first()?;
    Some(chord_label(chord, map.platform))
}

/// One chord as text.
pub fn chord_label(chord: &Chord, platform: Platform) -> String {
    let key = key_label(chord.key);
    let m = chord.mods;
    if !(m.cmd || m.ctrl || m.alt) {
        return if m.shift { format!("⇧{key}") } else { key };
    }
    let mut parts = Vec::new();
    if m.cmd {
        parts.push(if platform == Platform::Mac {
            "Cmd"
        } else {
            "Ctrl"
        });
    }
    if m.ctrl && !(m.cmd && platform != Platform::Mac) {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push(if platform == Platform::Mac {
            "Opt"
        } else {
            "Alt"
        });
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(&key);
    parts.join("+")
}

/// A key's name as printed on a key cap.
pub fn key_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    let short = match key {
        KeyCode::Escape => "Esc",
        KeyCode::Space => "Space",
        KeyCode::Enter | KeyCode::NumpadEnter => "⏎",
        KeyCode::Backspace => "⌫",
        KeyCode::Delete => "Del",
        KeyCode::Tab => "Tab",
        KeyCode::ArrowLeft => "←",
        KeyCode::ArrowRight => "→",
        KeyCode::ArrowUp => "↑",
        KeyCode::ArrowDown => "↓",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Slash => "/",
        KeyCode::Minus => "-",
        KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        _ => "",
    };
    if !short.is_empty() {
        return short.to_string();
    }
    for prefix in ["Key", "Digit", "Numpad"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    name
}
