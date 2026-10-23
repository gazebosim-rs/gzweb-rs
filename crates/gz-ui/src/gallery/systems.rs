//! The gallery's behaviour: the theme switch (T and the button), the
//! theme name and token values, the scene panel's splitter, and the
//! capture.

use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use bevy::ui::ComputedNode;
use bevy::ui_widgets::Activate;

use super::components::FocusExample;
use super::tokens::{hex, TokenHex};
use super::{GalleryCapture, ThemeText, ThemeToggle};
use crate::theme::Theme;
use crate::widgets::focus::TextFocus;
use crate::widgets::number_field::NumberInput;
use crate::widgets::text::set_text;
use crate::widgets::{Splitter, SplitterMoved, SplitterReset};

/// The scene panel's default width and its splitter's range.
pub(crate) const TREE_W: f32 = 320.0;
const TREE_MIN_W: f32 = 240.0;
const TREE_MAX_W: f32 = 420.0;

/// The pane a gallery splitter sizes.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SizesPane(pub Entity);

/// Keeps the splitter's size the pane's laid-out width.
pub(crate) fn sized_pane_ranges(
    mut splitters: Query<(&mut Splitter, &SizesPane)>,
    nodes: Query<&ComputedNode>,
) {
    for (mut s, pane) in &mut splitters {
        let Ok(n) = nodes.get(pane.0) else {
            continue;
        };
        let w = n.size().x * n.inverse_scale_factor();
        let want = s.with_range(w, TREE_MIN_W, TREE_MAX_W);
        s.set_if_neq(want);
    }
}

fn set_pane_width(pane: Option<&SizesPane>, nodes: &mut Query<&mut Node>, w: f32) {
    if let Some(mut node) = pane.and_then(|p| nodes.get_mut(p.0).ok()) {
        node.width = px(w);
    }
}

/// The splitter's drag and keys set the pane's width.
pub(crate) fn on_pane_resized(
    ev: On<SplitterMoved>,
    panes: Query<&SizesPane>,
    mut nodes: Query<&mut Node>,
) {
    set_pane_width(panes.get(ev.entity).ok(), &mut nodes, ev.size);
}

/// A double-click puts the pane back to its default width.
pub(crate) fn on_pane_reset(
    ev: On<SplitterReset>,
    panes: Query<&SizesPane>,
    mut nodes: Query<&mut Node>,
) {
    set_pane_width(panes.get(ev.entity).ok(), &mut nodes, TREE_W);
}

/// T switches light ↔ dark (not while typing in a field).
pub(crate) fn toggle_theme_key(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextFocus>,
    mut theme: ResMut<Theme>,
) {
    if keys.just_pressed(KeyCode::KeyT) && !typing.0 {
        theme.toggle();
    }
}

/// The theme button switches light ↔ dark.
pub(crate) fn on_theme_button(
    activate: On<Activate>,
    toggles: Query<(), With<ThemeToggle>>,
    mut theme: ResMut<Theme>,
) {
    if toggles.contains(activate.entity) {
        theme.toggle();
    }
}

/// The theme name and caption, the toggle's label and the token hex values.
pub(crate) fn gallery_texts(
    theme: Res<Theme>,
    mut names: Query<(&ThemeText, &mut Text)>,
    mut hexes: Query<(&TokenHex, &mut Text), Without<ThemeText>>,
    toggles: Query<&Children, With<ThemeToggle>>,
    mut labels: Query<&mut Text, (Without<ThemeText>, Without<TokenHex>)>,
) {
    if !theme.is_changed() {
        return;
    }
    let dark = theme.is_dark;
    for (which, mut t) in &mut names {
        let want = match (which, dark) {
            (ThemeText::Name, false) => "Light",
            (ThemeText::Name, true) => "Dark",
            (ThemeText::Caption, false) => "DEFAULT · FOLLOWS OS",
            (ThemeText::Caption, true) => "SAME TOKENS · OWN VALUES",
        };
        set_text(&mut t, want);
    }
    for (token, mut t) in &mut hexes {
        set_text(&mut t, &hex(theme.color(token.0)));
    }
    for children in &toggles {
        for &c in children {
            if let Ok(mut t) = labels.get_mut(c) {
                set_text(&mut t, if dark { "Light theme" } else { "Dark theme" });
            }
        }
    }
}

/// In a capture, focuses the example field once to show the focus look
/// (and a caret). Not interactively: the focused field would take the T
/// key (and, selected on focus, be replaced by a "t").
pub(crate) fn focus_example(
    capture: Option<Res<GalleryCapture>>,
    examples: Query<&Children, With<FocusExample>>,
    inputs: Query<(), With<NumberInput>>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    mut done: Local<bool>,
) {
    if *done || capture.is_none() {
        return;
    }
    for children in &examples {
        if let Some(input) = children.iter().find(|c| inputs.contains(*c)) {
            focus.set(input, FocusCause::Navigated);
            visible.0 = true;
            *done = true;
        }
    }
}

/// Saves the screenshot after [`GalleryCapture::frames`] frames, once.
pub(crate) fn capture_gallery(capture: Option<ResMut<GalleryCapture>>, mut commands: Commands) {
    let Some(mut capture) = capture else {
        return;
    };
    if capture.frames == 0 {
        return;
    }
    capture.frames -= 1;
    if capture.frames == 0 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(gz_bevy::snapshot::save_screenshot(capture.path.clone()));
    }
}

/// The sample tree rows are controlled: the gallery owns their state and
/// answers their caret and eye.
pub(crate) fn on_tree_row_expand(
    ev: On<crate::widgets::TreeRowExpand>,
    mut rows: Query<&mut crate::widgets::TreeRowSpec>,
) {
    if let Ok(mut spec) = rows.get_mut(ev.entity) {
        spec.expanded = Some(ev.expanded);
    }
}

/// See [`on_tree_row_expand`].
pub(crate) fn on_tree_row_visibility(
    ev: On<crate::widgets::TreeRowVisibility>,
    mut rows: Query<&mut crate::widgets::TreeRowSpec>,
) {
    if let Ok(mut spec) = rows.get_mut(ev.entity) {
        spec.hidden = ev.hidden;
    }
}
