//! The Topics panel's ready body (spec §3.3): the filter, the chips, the
//! topic list and the echo, built from the panel's resources whenever the
//! shell spawns the body (DECISIONS.md D-58, D-99). The list's rows and
//! the echo's text follow in place ([`systems`](super::systems)).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, RadioGroup, ScrollArea};

use super::model::{Echo, TopicChip, TopicsState};
use super::systems::TopicsScroll;
use super::TopicsBody;
use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::theme::visuals::{Content, OwnInk, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, display, row};
use crate::widgets::{
    a11y, choice_chip, filter_field, focus, icon_button, rounded, IconButtonSize, Tooltip,
};

/// The filter field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TopicFilter;

/// A topic chip.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChipChoice(pub(crate) TopicChip);

/// The list: the tab stop whose arrow keys move the echo.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TopicList;

/// The list's rows' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TopicRows;

/// A topic's row.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct TopicRow(pub(crate) String);

/// A row's rate.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct RowRate(pub(crate) String);

/// "No topics match", while the filter hides every topic.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct NoMatches;

/// The echo section, shown while a topic is echoed.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct EchoSection;

/// The echo's parts that follow [`Echo`] in place.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EchoPart {
    /// The topic's name.
    Title,
    /// Its message type and rate.
    Meta,
    /// "Waiting for first message…" or "Topic ended".
    Status,
    /// The message, as text.
    Message,
}

/// The echo header's close button: stop echoing.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct EchoClose;

/// Builds the ready body's content from the panel's resources.
pub(super) fn build_topics(
    ev: On<Add<TopicsBody>>,
    theme: Res<Theme>,
    state: Res<TopicsState>,
    scroll: Res<TopicsScroll>,
    echo: Res<Echo>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let filter = state.filter.clone();
    let chip = state.chip;
    let echoing = echo.topic.is_some();
    let y = scroll.0;
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
            // The filter.
            c.spawn(Node {
                padding: UiRect {
                    left: px(SPACE[4]),
                    right: px(SPACE[4]),
                    top: px(SPACE[4]),
                    bottom: px(0),
                },
                flex_shrink: 0.0,
                ..row(SPACE[3])
            })
            .with_child((
                filter_field(&t, "Filter topics", "Filter", filter),
                TopicFilter,
            ));
            // The chips.
            c.spawn((
                RadioGroup,
                a11y(Role::RadioGroup, "Topic kind"),
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(SPACE[2]),
                    row_gap: px(SPACE[2]),
                    padding: UiRect {
                        left: px(SPACE[4]),
                        right: px(SPACE[4]),
                        top: px(SPACE[4]),
                        bottom: px(0),
                    },
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|chips| {
                for c in TopicChip::ALL {
                    chips.spawn((choice_chip(c.label(), c == chip), ChipChoice(c)));
                }
            });
            // The list; its rows come from `sync_rows`.
            c.spawn((
                TopicList,
                ScrollArea,
                ScrollPosition(Vec2::new(0.0, y)),
                TabIndex(0),
                a11y(Role::ListBox, "Topics"),
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_height: px(t.s.tree_row_h * 3.0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::all(px(SPACE[4])),
                    ..column(0.0)
                },
            ))
            .with_children(|list| {
                list.spawn((
                    TopicRows,
                    // Bevy links an accessibility node only to its parent's,
                    // so the rows reach the list through this one.
                    AccessibilityNode(accesskit::Node::new(Role::GenericContainer)),
                    Node {
                        flex_shrink: 0.0,
                        ..column(0.0)
                    },
                ));
                list.spawn((
                    NoMatches,
                    text(TypeStyle::BODY, ColorRole::Text2, "No topics match"),
                    Node {
                        align_self: AlignSelf::Center,
                        margin: UiRect::top(px(SPACE[8])),
                        display: Display::None,
                        ..default()
                    },
                ));
            });
            spawn_echo(c, &t, echoing);
        });
    });
}

/// The echo section (§3.3): a header with the topic, its type and rate,
/// and the message as monospace text.
fn spawn_echo(c: &mut ChildSpawnerCommands, t: &Theme, shown: bool) {
    c.spawn((
        EchoSection,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(t.s.tree_row_h * 4.0),
            border: UiRect::top(px(border::HAIRLINE)),
            display: display(shown),
            ..column(0.0)
        },
        Stroke(ColorRole::Line),
    ))
    .with_children(|e| {
        e.spawn(Node {
            padding: UiRect::new(px(SPACE[7]), px(SPACE[3]), px(SPACE[4]), px(SPACE[2])),
            flex_shrink: 0.0,
            align_items: AlignItems::Start,
            ..row(SPACE[3])
        })
        .with_children(|h| {
            h.spawn(Node {
                flex_grow: 1.0,
                min_width: px(0),
                overflow: Overflow::clip(),
                ..column(SPACE[1])
            })
            .with_children(|names| {
                names.spawn((
                    EchoPart::Title,
                    text(TypeStyle::MONO_SMALL, ColorRole::Text, ""),
                    TextLayout::no_wrap(),
                ));
                names.spawn((
                    EchoPart::Meta,
                    text(TypeStyle::TINY, ColorRole::Text2, ""),
                    TextLayout::no_wrap(),
                ));
            });
            h.spawn((
                icon_button(
                    t,
                    IconId::Close,
                    IconButtonSize::Small,
                    Tooltip::new("Stop echo"),
                ),
                EchoClose,
            ));
        });
        e.spawn((
            EchoPart::Status,
            text(TypeStyle::SMALL, ColorRole::Text3, ""),
            Node {
                margin: UiRect::horizontal(px(SPACE[7])),
                ..default()
            },
        ));
        e.spawn((
            ScrollArea,
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::scroll(),
                padding: UiRect::new(px(SPACE[7]), px(SPACE[7]), px(SPACE[2]), px(SPACE[5])),
                ..column(0.0)
            },
        ))
        .with_child((
            EchoPart::Message,
            text(TypeStyle::MONO_SMALL, ColorRole::Text, ""),
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        ));
    });
}

/// A topic's row: its name, clipped at the edge, and its rate in `text_2`
/// at the right (§3.3). Clicking it echoes the topic.
pub(super) fn topic_row(theme: &Theme, name: &str, rate: String) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    visuals.active_ink = ColorRole::Text;
    (
        Node {
            height: px(theme.s.tree_row_h),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[4])),
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
        a11y(Role::ListBoxOption, name.to_string()),
        TopicRow(name.to_string()),
        children![
            (
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                children![(
                    text(TypeStyle::MONO_SMALL, ColorRole::Text, name.to_string()),
                    TextLayout::no_wrap(),
                    Content,
                )],
            ),
            (
                RowRate(name.to_string()),
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
