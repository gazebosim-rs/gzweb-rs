//! The status bar's own parts (its readouts are in the shell's tests).

use gz_bevy::view::input::{InputMap, Platform};

use super::workspace_tooltip;

/// The workspace button's shortcut comes from the input map.
#[test]
fn the_workspace_tooltip_reads_the_input_map() {
    let mut map = InputMap::builtin();
    map.platform = Platform::Linux;
    assert_eq!(workspace_tooltip(&map).shortcut.as_deref(), Some("Alt+1–4"));
    map.platform = Platform::Mac;
    assert_eq!(workspace_tooltip(&map).shortcut.as_deref(), Some("Opt+1–4"));
}
