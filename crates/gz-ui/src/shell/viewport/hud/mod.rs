//! The view's HUD pills (spec §3.4; `hud` fill, 1px `line`, no shadow per
//! `elevation.hud`): the scale label of an orthographic view and the
//! target chip of a following or tracking camera.
//!
//! - **Scale label:** bottom right, inset 10, mono 11 `text_2`. A label
//!   alone ("1 m") doesn't say what is a metre long, so the pill also
//!   holds a bar that long (DECISIONS.md D-63): a 1-2-5 length between
//!   [`MIN_BAR`] and [`MAX_BAR`] pixels at the view's scale.
//! - **Target chip:** 28 tall, radius 8, left of the rail's Camera mode
//!   button (gap 8, centred on it): the `follow` (or `track`) icon 13 in
//!   `accent_text`, the target's name 12/500 and a 20 px close button that
//!   frees the camera (as ⇧F or ⇧T does). Shown only while following or
//!   tracking (sensor cameras come with the Sensors panel, M7).

pub(crate) mod systems;

use bevy::picking::Pickable;
use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::{border, radius, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::row;
use crate::widgets::{icon_button, rounded, IconButtonSize, Tooltip, TooltipSide};

/// The scale label's text: mono 11 (§3.4).
pub(crate) const SCALE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// The chip's target name: 12/500.
pub(crate) const CHIP_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 12.0, 500);
/// The chip's icon size.
pub(crate) const CHIP_ICON: f32 = 13.0;
/// The chip's height and radius.
pub(crate) const CHIP_H: f32 = 28.0;
/// The scale bar's length bounds, in UI pixels.
pub(crate) const MIN_BAR: f32 = 32.0;
pub(crate) const MAX_BAR: f32 = 80.0;
/// The scale bar's tick height.
const BAR_TICK: f32 = 5.0;

/// The scale label's pill.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ScaleLabel;

/// The scale label's bar.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ScaleBar;

/// The target chip.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TargetChip;

/// The chip's icon.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TargetIcon;

/// The chip's close button.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ReleaseTarget;

/// A HUD pill's look: `hud` fill, 1px `line`.
fn pill(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(border::HAIRLINE)),
            ..node
        },
        Fill(ColorRole::Hud),
        Stroke(ColorRole::Line),
    )
}

/// The scale label, hidden until the view is orthographic.
pub(crate) fn scale_label(theme: &Theme) -> impl Bundle {
    let inset = theme.s.viewport_inset;
    (
        ScaleLabel,
        Name::new("scale label"),
        pill(Node {
            position_type: PositionType::Absolute,
            right: px(inset),
            bottom: px(inset),
            padding: UiRect::axes(px(SPACE[4]), px(SPACE[2])),
            border_radius: rounded(radius::PILL),
            display: Display::None,
            ..row(SPACE[3])
        }),
        Pickable::IGNORE,
        children![
            (
                ScaleBar,
                Node {
                    width: px(MIN_BAR),
                    height: px(BAR_TICK),
                    border: UiRect {
                        left: px(border::HAIRLINE),
                        right: px(border::HAIRLINE),
                        bottom: px(border::HAIRLINE),
                        top: px(0),
                    },
                    ..default()
                },
                Stroke(ColorRole::Text2),
                Pickable::IGNORE,
            ),
            (text(SCALE_TYPE, ColorRole::Text2, "1 m"), Pickable::IGNORE),
        ],
    )
}

/// The target chip, placed left of the Camera mode button it is a child
/// of (`right` past the button and the rail's padding and border, plus the
/// 8 px gap), hidden until the camera follows or tracks something.
pub(crate) fn target_chip(theme: &Theme, map: &InputMap) -> impl Bundle {
    let rail_pad = super::rail::RAIL_PADDING + border::HAIRLINE;
    (
        TargetChip,
        Name::new("target chip"),
        pill(Node {
            position_type: PositionType::Absolute,
            right: Val::Percent(100.0),
            margin: UiRect::right(px(SPACE[4] + rail_pad)),
            top: px((theme.s.rail_btn - CHIP_H) / 2.0),
            height: px(CHIP_H),
            padding: UiRect {
                left: px(SPACE[4]),
                right: px(SPACE[2]),
                ..default()
            },
            border_radius: rounded(radius::MD),
            display: Display::None,
            flex_shrink: 0.0,
            ..row(SPACE[3])
        }),
        children![
            (
                icon(IconId::Follow, CHIP_ICON, ColorRole::AccentText),
                TargetIcon
            ),
            (text(CHIP_TYPE, ColorRole::Text, ""), Pickable::IGNORE),
            (
                icon_button(
                    theme,
                    IconId::Close,
                    IconButtonSize::Bar,
                    Tooltip::new("Stop following")
                        .binding(map, "view.follow")
                        .side(TooltipSide::Below),
                ),
                ReleaseTarget,
            ),
        ],
    )
}

/// A 1-2-5 length (metres) whose bar is between [`MIN_BAR`] and
/// [`MAX_BAR`] pixels long at `metres_per_px`, and that length in pixels.
pub(crate) fn scale_bar(metres_per_px: f32) -> Option<(f32, f32)> {
    if !(metres_per_px.is_finite() && metres_per_px > 0.0) {
        return None;
    }
    let most = MAX_BAR * metres_per_px;
    let decade = 10f32.powf(most.log10().floor());
    let metres = [5.0, 2.0, 1.0]
        .iter()
        .map(|m| m * decade)
        .find(|m| *m <= most * (1.0 + 1e-5))
        .unwrap_or(decade);
    Some((metres, metres / metres_per_px))
}

/// A length as the scale label reads: "1 m", "50 cm", "2 mm", "5 km".
pub(crate) fn scale_text(metres: f32) -> String {
    let (v, unit) = if metres >= 1000.0 {
        (metres / 1000.0, "km")
    } else if metres >= 1.0 {
        (metres, "m")
    } else if metres >= 0.01 {
        (metres * 100.0, "cm")
    } else {
        (metres * 1000.0, "mm")
    };
    let v = if v >= 1.0 { v.round() } else { v };
    format!("{v} {unit}")
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (systems::scale_label_state, systems::target_chip_state)
            .in_set(crate::shell::ShellSystems::Readouts),
    )
    .add_observer(systems::on_release);
}

#[cfg(test)]
mod tests;
