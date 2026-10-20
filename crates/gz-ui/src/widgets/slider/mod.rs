//! Slider (§5.11): a 4 px `line` track with the filled part in `accent`, a
//! 16 px thumb (`surface` fill, 2 px `accent` border), the label on the
//! left (12 `text_2`, fixed width) and the value on the right (mono 12
//! `text`). On Bevy's headless slider (drag, track click, Home/End,
//! Left/Right, AccessKit); Up and Down step too (spec: "arrow keys step"),
//! and every step is coarse (×10) with Shift and fine (÷10) with Alt. The
//! row is an input's height (`input_h`).

use accesskit::Role;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::FocusedInput;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{
    observe, slider_self_update, Slider, SliderPrecision, SliderRange, SliderStep, SliderThumb,
    SliderValue, TrackClick, ValueChange,
};

use super::{a11y, rounded};
use crate::assets::fonts::text;
use crate::theme::{ColorRole, Fill, FocusRing, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::GzUiSystems;

/// Thumb diameter.
pub const THUMB: f32 = 16.0;
/// Track thickness.
pub const TRACK: f32 = 4.0;
/// Value read-out type: mono 12/400.
pub const VALUE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 400);
/// The default label column width ("fixed width", §5.11).
pub const LABEL_W: f32 = 64.0;
/// The value read-out's minimum width, so the track doesn't move as the
/// digits change.
pub const READOUT_MIN_W: f32 = 40.0;

/// A slider's range, value and step.
#[derive(Clone, Debug, PartialEq)]
pub struct SliderSpec {
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    /// The arrow-key step without modifiers.
    pub step: f32,
    /// Decimals shown (and kept).
    pub precision: i32,
    /// Label column width.
    pub label_w: f32,
}

impl SliderSpec {
    pub fn new(label: impl Into<String>, min: f32, max: f32, value: f32) -> Self {
        Self {
            label: label.into(),
            min,
            max,
            value,
            step: (max - min) / 100.0,
            precision: 2,
            label_w: LABEL_W,
        }
    }
}

/// The step without modifiers; `modifier_steps` scales [`SliderStep`]
/// from it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct BaseStep(pub f32);

/// The value text next to a slider.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SliderReadout {
    pub precision: i32,
}

/// The arrow-key step with Shift (×10, coarse) or Alt (÷10, fine) held.
pub fn step_for(base: f32, shift: bool, alt: bool) -> f32 {
    match (shift, alt) {
        (true, false) => base * 10.0,
        (false, true) => base / 10.0,
        _ => base,
    }
}

/// Formats a slider value with `precision` decimals.
pub fn format_value(value: f32, precision: i32) -> String {
    format!("{value:.*}", precision.max(0) as usize)
}

/// Marks the filled part of the track and the thumb's travel box.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliderPart {
    Fill,
    Thumb,
}

/// A labelled slider row.
pub fn slider_row(theme: &Theme, spec: SliderSpec) -> impl Bundle {
    let frac = SliderRange::new(spec.min, spec.max).thumb_position(spec.value) * 100.0;
    let readout = format_value(spec.value, spec.precision);
    (
        Node {
            height: px(theme.s.input_h),
            width: Val::Percent(100.0),
            column_gap: px(SPACE[5]),
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                text(TypeStyle::SMALL, ColorRole::Text2, spec.label.clone()),
                Node {
                    width: px(spec.label_w),
                    flex_shrink: 0.0,
                    ..default()
                },
            ),
            (
                Node {
                    flex_grow: 1.0,
                    height: px(THUMB),
                    ..default()
                },
                Slider {
                    track_click: TrackClick::Snap,
                    ..default()
                },
                SliderValue(spec.value),
                SliderRange::new(spec.min, spec.max),
                SliderStep(spec.step),
                SliderPrecision(spec.precision),
                BaseStep(spec.step),
                Hovered::default(),
                TabIndex(0),
                FocusRing,
                a11y(Role::Slider, spec.label.clone()),
                observe(slider_self_update),
                children![
                    // Track, inset by half a thumb so the ends line up.
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(THUMB / 2.0),
                            right: px(THUMB / 2.0),
                            top: px((THUMB - TRACK) / 2.0),
                            height: px(TRACK),
                            border_radius: rounded(TRACK / 2.0),
                            ..default()
                        },
                        Fill(ColorRole::Line),
                        children![(
                            SliderPart::Fill,
                            Node {
                                width: Val::Percent(frac),
                                height: Val::Percent(100.0),
                                border_radius: rounded(TRACK / 2.0),
                                ..default()
                            },
                            Fill(ColorRole::Accent),
                        )],
                    ),
                    // Thumb travel: the slider minus one thumb.
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(THUMB),
                            top: px(0),
                            bottom: px(0),
                            ..default()
                        },
                        children![(
                            SliderPart::Thumb,
                            SliderThumb,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(frac),
                                width: px(THUMB),
                                height: px(THUMB),
                                border: UiRect::all(px(2)),
                                border_radius: rounded(THUMB / 2.0),
                                ..default()
                            },
                            Fill(ColorRole::Surface),
                            Stroke(ColorRole::Accent),
                        )],
                    ),
                ],
            ),
            (
                SliderReadout {
                    precision: spec.precision,
                },
                text(VALUE_TYPE, ColorRole::Text, readout),
                Node {
                    min_width: px(READOUT_MIN_W),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::End,
                    ..default()
                },
                TextLayout::justify(Justify::Right),
            ),
        ],
    )
}

/// Sets each slider's [`SliderStep`] from its [`BaseStep`] and the held
/// modifiers, before the headless slider sees the arrow key. `SliderStep`
/// is immutable, so it is replaced rather than changed in place.
pub(crate) fn modifier_steps(
    keys: Res<ButtonInput<KeyCode>>,
    sliders: Query<(Entity, &BaseStep, &SliderStep)>,
    mut commands: Commands,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for (entity, base, step) in &sliders {
        let want = step_for(base.0, shift, alt);
        if step.0 != want {
            commands.entity(entity).insert(SliderStep(want));
        }
    }
}

/// Up and Down step like Right and Left (Bevy's slider takes only those).
pub(crate) fn on_slider_vertical_keys(
    mut ev: On<FocusedInput<KeyboardInput>>,
    sliders: Query<
        (
            &SliderValue,
            &SliderRange,
            &SliderStep,
            Has<InteractionDisabled>,
        ),
        With<Slider>,
    >,
    mut commands: Commands,
) {
    if ev.input.state != ButtonState::Pressed || ev.event_target() != ev.focused_entity {
        return;
    }
    let Ok((value, range, step, disabled)) = sliders.get(ev.focused_entity) else {
        return;
    };
    let delta = match ev.input.key_code {
        KeyCode::ArrowUp => step.0,
        KeyCode::ArrowDown => -step.0,
        _ => return,
    };
    ev.propagate(false);
    if !disabled {
        commands.trigger(ValueChange {
            source: ev.focused_entity,
            value: range.clamp(value.0 + delta),
            is_final: true,
        });
    }
}

/// Moves the fill and thumb, and updates the read-out, when a value changes.
pub(crate) fn slider_visuals_system(
    sliders: Query<(Entity, &SliderValue, &SliderRange, &ChildOf), Changed<SliderValue>>,
    children: Query<&Children>,
    mut parts: Query<(&SliderPart, &mut Node)>,
    readouts: Query<&SliderReadout>,
    mut texts: Query<&mut Text>,
) {
    for (slider, value, range, row) in &sliders {
        let frac = Val::Percent(range.thumb_position(value.0) * 100.0);
        for e in children.iter_descendants(slider) {
            if let Ok((part, mut node)) = parts.get_mut(e) {
                match part {
                    SliderPart::Fill if node.width != frac => node.width = frac,
                    SliderPart::Thumb if node.left != frac => node.left = frac,
                    _ => {}
                }
            }
        }
        for e in children.iter_descendants(row.parent()) {
            if let (Ok(readout), Ok(mut t)) = (readouts.get(e), texts.get_mut(e)) {
                let s = format_value(value.0, readout.precision);
                if t.0 != s {
                    t.0 = s;
                }
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_slider_vertical_keys)
        .add_systems(PreUpdate, modifier_steps.in_set(GzUiSystems::Input))
        .add_systems(Update, slider_visuals_system.in_set(GzUiSystems::Behaviour));
}
