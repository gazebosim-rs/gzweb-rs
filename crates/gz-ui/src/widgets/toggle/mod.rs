//! Toggle (§5.9): a 34×20 track (radius 10) with a 16 px knob at inset 2.
//! Off: `toggle_track_off` / `toggle_knob_off`; on: `accent` /
//! `toggle_knob_on`, knob at left 16. Always in a row with its label on the
//! left; the whole row is the hit target (a headless checkbox).

use accesskit::Role;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{checkbox_self_update, observe, Checkbox};

use super::button::Checkedness;
use super::{clickable, rounded, spacer};
use crate::assets::fonts::text;
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{ColorRole, Fill, Theme, TypeStyle};
use crate::GzUiSystems;

/// Row height for a toggle (§5.9: 30–32).
pub const TOGGLE_ROW_H: f32 = 30.0;

/// Marks a toggle's track and knob.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TogglePart {
    Track,
    Knob,
}

/// Where the knob sits: inset 2 when off, `toggle_w - knob - 2` when on.
pub fn knob_left(theme: &Theme, on: bool) -> f32 {
    if on {
        theme.s.toggle_w - theme.s.toggle_knob - 2.0
    } else {
        2.0
    }
}

/// A labelled toggle row.
pub fn toggle_row(theme: &Theme, label: impl Into<String>, on: bool) -> impl Bundle {
    let label = label.into();
    let s = &theme.s;
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text).keep_ink();
    visuals.hover_fill = ColorRole::Clear;
    visuals.pressed_fill = ColorRole::Clear;
    visuals.active_fill = ColorRole::Clear;
    visuals.active_ink = ColorRole::Text;
    (
        Node {
            height: px(TOGGLE_ROW_H),
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            ..default()
        },
        Checkbox,
        clickable(visuals, Role::Switch, label.clone()),
        Checkedness(on),
        observe(checkbox_self_update),
        children![
            (text(TypeStyle::BODY, ColorRole::Text, label), Content),
            spacer(),
            (
                TogglePart::Track,
                Node {
                    width: px(s.toggle_w),
                    height: px(s.toggle_h),
                    flex_shrink: 0.0,
                    border_radius: rounded(s.toggle_h / 2.0),
                    ..default()
                },
                Fill(ColorRole::ToggleTrackOff),
                children![(
                    TogglePart::Knob,
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(2),
                        left: px(knob_left(theme, on)),
                        width: px(s.toggle_knob),
                        height: px(s.toggle_knob),
                        border_radius: rounded(s.toggle_knob / 2.0),
                        ..default()
                    },
                    Fill(ColorRole::ToggleKnobOff),
                )],
            ),
        ],
    )
}

/// Draws each toggle's track and knob from `Checked`: for rows that are
/// new or were checked or unchecked, or all of them when the theme changes.
#[allow(clippy::type_complexity)]
pub(crate) fn toggle_system(
    theme: Res<Theme>,
    rows: Query<(Entity, Has<Checked>), With<Checkbox>>,
    changed: Query<Entity, (With<Checkbox>, Or<(Added<Checkbox>, Added<Checked>)>)>,
    mut unchecked: RemovedComponents<Checked>,
    children: Query<&Children>,
    mut parts: Query<(&TogglePart, &mut Fill, &mut Node)>,
    mut dirty: Local<Vec<Entity>>,
) {
    dirty.clear();
    if theme.is_changed() {
        dirty.extend(rows.iter().map(|(e, _)| e));
    } else {
        dirty.extend(changed.iter().chain(unchecked.read()));
    }
    for (row, on) in dirty.iter().filter_map(|&e| rows.get(e).ok()) {
        for e in children.iter_descendants(row) {
            let Ok((part, mut fill, mut node)) = parts.get_mut(e) else {
                continue;
            };
            match part {
                TogglePart::Track => {
                    fill.set_if_neq(Fill(if on {
                        ColorRole::Accent
                    } else {
                        ColorRole::ToggleTrackOff
                    }));
                }
                TogglePart::Knob => {
                    fill.set_if_neq(Fill(if on {
                        ColorRole::ToggleKnobOn
                    } else {
                        ColorRole::ToggleKnobOff
                    }));
                    let left = px(knob_left(&theme, on));
                    if node.left != left {
                        node.left = left;
                    }
                }
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, toggle_system.in_set(GzUiSystems::Visuals));
}
