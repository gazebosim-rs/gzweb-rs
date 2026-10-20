//! Toggles draw their track and knob from `Checked`.

use bevy::prelude::*;
use bevy::ui::Checked;

use super::{knob_left, TogglePart};
use crate::test_support::{descendants, frames, ui_app};
use crate::theme::{ColorRole, Fill, Theme, ThemeMode};
use crate::widgets::*;

#[test]
fn toggles_follow_checked() {
    let t = Theme::default();
    assert_eq!(knob_left(&t, false), 2.0);
    assert_eq!(knob_left(&t, true), 16.0);
    let mut app = ui_app(ThemeMode::Light);
    let on = app.world_mut().spawn(toggle_row(&t, "Gravity", true)).id();
    let off = app.world_mut().spawn(toggle_row(&t, "Static", false)).id();
    frames(&mut app, 2);
    let parts = |app: &mut App, row: Entity| {
        let w = app.world_mut();
        let mut q = w.query::<(&TogglePart, &Fill, &Node)>();
        let kids = descendants(w, row);
        kids.iter()
            .filter_map(|e| q.get(w, *e).ok().map(|(p, f, n)| (*p, f.0, n.left)))
            .collect::<Vec<_>>()
    };
    let _ = &parts;
    let on_parts = parts(&mut app, on);
    assert!(on_parts.contains(&(TogglePart::Track, ColorRole::Accent, Val::Auto)));
    assert!(on_parts.contains(&(TogglePart::Knob, ColorRole::ToggleKnobOn, px(16))));
    let off_parts = parts(&mut app, off);
    assert!(off_parts.contains(&(TogglePart::Track, ColorRole::ToggleTrackOff, Val::Auto)));
    assert!(off_parts.contains(&(TogglePart::Knob, ColorRole::ToggleKnobOff, px(2))));
    // Clicking the row (the headless checkbox) flips it.
    app.world_mut().entity_mut(off).insert(Checked);
    frames(&mut app, 1);
    assert!(parts(&mut app, off).contains(&(TogglePart::Knob, ColorRole::ToggleKnobOn, px(16))));
}
