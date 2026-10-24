//! The Sensors panel's body (spec §3.3): the model's sensors, then the
//! chosen one's preview (a camera's frames, a lidar's scan from above, or
//! the message as text). Built whenever the shell spawns the body
//! (DECISIONS.md D-58, D-103); the rows and the preview follow in place
//! ([`systems`](super::systems)).

use accesskit::Role;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, ScrollArea};

use super::model::SensorEntry;
use super::SensorsBody;
use crate::assets::fonts::text;
use crate::theme::visuals::{Content, OwnInk, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, flex_spacer, row};
use crate::widgets::{a11y, focus, rounded};

/// The lidar view's height over its width.
pub(crate) const SCAN_ASPECT: f32 = 0.6;

/// The sensors' rows' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct SensorRows;

/// A sensor's row: its link and name.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct SensorRow(pub(crate) (Entity, String));

/// The preview's parts.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewPart {
    /// The section (shown once a sensor is chosen).
    Section,
    /// The sensor's name.
    Title,
    /// Its size and rate ("640×480 · 10 Hz").
    Meta,
    /// The camera frame.
    Image,
    /// The lidar view.
    Scan,
    /// The message as text, and its scroll area.
    Text,
    TextArea,
    /// "Waiting for first message…", "No data for 3.2 s".
    Status,
}

/// Builds the body.
pub(super) fn build_sensors(ev: On<Add<SensorsBody>>, mut commands: Commands) {
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
            c.spawn((
                SensorRows,
                a11y(Role::ListBox, "Sensors"),
                Node {
                    padding: UiRect::all(px(SPACE[4])),
                    flex_shrink: 0.0,
                    ..column(SPACE[1])
                },
            ));
            spawn_preview(c);
        });
    });
}

/// The preview section: a header with the sensor's name and size, then
/// the frame, the scan or the text, with a status over it.
fn spawn_preview(c: &mut ChildSpawnerCommands) {
    c.spawn((
        PreviewPart::Section,
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            border: UiRect::top(px(border::HAIRLINE)),
            display: Display::None,
            ..column(0.0)
        },
        Stroke(ColorRole::Line),
    ))
    .with_children(|s| {
        s.spawn(Node {
            padding: UiRect::axes(px(SPACE[5]), px(SPACE[4])),
            align_items: AlignItems::Baseline,
            flex_shrink: 0.0,
            ..row(SPACE[3])
        })
        .with_children(|h| {
            h.spawn((
                PreviewPart::Title,
                text(TypeStyle::SECTION, ColorRole::Text2, ""),
                TextLayout::no_wrap(),
            ));
            h.spawn((
                PreviewPart::Meta,
                text(TypeStyle::MONO_SMALL, ColorRole::Text2, ""),
                TextLayout::no_wrap(),
            ));
            h.spawn(flex_spacer());
        });
        s.spawn(Node {
            flex_shrink: 0.0,
            ..column(0.0)
        })
        .with_children(|p| {
            p.spawn((
                PreviewPart::Image,
                ImageNode::default(),
                Node {
                    width: percent(100),
                    display: Display::None,
                    ..default()
                },
            ));
            p.spawn((
                PreviewPart::Scan,
                ImageNode::default(),
                Node {
                    width: percent(100),
                    aspect_ratio: Some(1.0 / SCAN_ASPECT),
                    display: Display::None,
                    ..default()
                },
                Fill(ColorRole::Bg),
            ));
            p.spawn((
                PreviewPart::Status,
                text(TypeStyle::SMALL, ColorRole::WarnText, ""),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(SPACE[3]),
                    left: px(SPACE[4]),
                    padding: UiRect::axes(px(SPACE[3]), px(SPACE[1])),
                    border_radius: rounded(radius::SM),
                    display: Display::None,
                    ..default()
                },
                Fill(ColorRole::WarnSoft),
            ));
        });
        s.spawn((
            PreviewPart::TextArea,
            ScrollArea,
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::scroll(),
                padding: UiRect::new(px(SPACE[5]), px(SPACE[5]), px(0), px(SPACE[5])),
                display: Display::None,
                ..column(0.0)
            },
        ))
        .with_child((
            PreviewPart::Text,
            text(TypeStyle::MONO_SMALL, ColorRole::Text, ""),
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        ));
    });
}

/// A sensor's row (§3.3): its type badge, name and rate. A click shows
/// it.
pub(super) fn sensor_row(theme: &Theme, s: &SensorEntry, badge: &str, rate: String) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    visuals.active_ink = ColorRole::Text;
    (
        Node {
            height: px(theme.s.tree_row_h),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[3])),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Headless,
        Hovered::default(),
        focus::PressFocusless,
        visuals,
        Fill(visuals.fill),
        a11y(Role::ListBoxOption, s.name.clone()),
        SensorRow(s.key()),
        children![
            (
                Node {
                    padding: UiRect::axes(px(SPACE[2]), px(1)),
                    border_radius: rounded(radius::SM),
                    flex_shrink: 0.0,
                    ..default()
                },
                Fill(ColorRole::SensorSoft),
                children![text(
                    TypeStyle::TINY,
                    ColorRole::EntitySensor,
                    badge.to_string()
                )],
            ),
            (
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                children![(
                    text(TypeStyle::BODY, ColorRole::Text, s.name.clone()),
                    TextLayout::no_wrap(),
                    Content,
                )],
            ),
            (
                text(TypeStyle::MONO_SMALL, ColorRole::Text2, rate),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                Content,
                OwnInk,
            ),
        ],
    )
}
