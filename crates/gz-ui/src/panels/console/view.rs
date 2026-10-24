//! The Console panel's body and footer (spec §3.3): the filter, the level
//! chips and the lines, newest first, each with its level's dot, its
//! message and `source · time`; the footer's command line. Built from the
//! panel's resources whenever the shell spawns them (DECISIONS.md D-58,
//! D-98); the rows follow in place ([`systems`](super::systems)).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{RadioGroup, ScrollArea};
use gz_bevy::messages::{MessageLevel, SystemMessage};

use super::model::{source_label, ConsoleState, LevelChip};
use super::{ConsoleBody, ConsoleFooter};
use crate::assets::fonts::text;
use crate::theme::{border, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, display, row};
use crate::widgets::text_field::command_field;
use crate::widgets::{a11y, choice_chip, filter_field, rounded};

/// The level dot's side (§3.3: 8px).
pub(crate) const DOT: f32 = 8.0;

/// The command line's placeholder (the mock's).
pub(crate) const PLACEHOLDER: &str = "gz command… e.g. service -s /world/…";

/// The filter field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct ConsoleFilter;

/// A level chip.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChipChoice(pub(crate) LevelChip);

/// The lines' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct ConsoleRows;

/// A line's row: the line's number.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ConsoleRow(pub(crate) u64);

/// "No messages match", while the chip or the filter hides every line.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct NoMatches;

/// The command line's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct CommandLine;

/// Builds the ready body's content from the panel's resources.
pub(super) fn build_console(
    ev: On<Add<ConsoleBody>>,
    theme: Res<Theme>,
    state: Res<ConsoleState>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let (filter, chip) = (state.filter.clone(), state.chip);
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
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
                filter_field(&t, "Filter messages", "Filter messages", filter),
                ConsoleFilter,
            ));
            c.spawn((
                RadioGroup,
                a11y(Role::RadioGroup, "Message level"),
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(SPACE[2]),
                    row_gap: px(SPACE[2]),
                    padding: UiRect {
                        left: px(SPACE[4]),
                        right: px(SPACE[4]),
                        top: px(SPACE[4]),
                        bottom: px(SPACE[2]),
                    },
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|chips| {
                for c in LevelChip::ALL {
                    chips.spawn((choice_chip(c.label(), c == chip), ChipChoice(c)));
                }
            });
            c.spawn((
                ScrollArea,
                ScrollPosition::default(),
                TabIndex(0),
                a11y(Role::Log, "Messages"),
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_height: px(t.s.tree_row_h * 3.0),
                    overflow: Overflow::scroll_y(),
                    ..column(0.0)
                },
            ))
            .with_children(|list| {
                list.spawn((
                    ConsoleRows,
                    // Bevy links an accessibility node only to its parent's,
                    // so the rows reach the log through this one.
                    AccessibilityNode(accesskit::Node::new(Role::GenericContainer)),
                    Node {
                        flex_shrink: 0.0,
                        ..column(0.0)
                    },
                ));
                list.spawn((
                    NoMatches,
                    text(TypeStyle::BODY, ColorRole::Text2, "No messages match"),
                    Node {
                        align_self: AlignSelf::Center,
                        margin: UiRect::top(px(SPACE[8])),
                        display: Display::None,
                        ..default()
                    },
                ));
            });
        });
    });
}

/// A level's dot color: `danger`, `warn`, `info`, `text_3`.
pub(crate) fn level_color(level: MessageLevel) -> ColorRole {
    match level {
        MessageLevel::Error => ColorRole::Danger,
        MessageLevel::Warning => ColorRole::Warn,
        MessageLevel::Info => ColorRole::Info,
        MessageLevel::Debug => ColorRole::Text3,
    }
}

/// A line's row: its level's dot, the message, then `source · time` in
/// mono 11; hidden unless `shown` (the chip and the filter take it).
pub(super) fn line_row(m: &SystemMessage, shown: bool) -> impl Bundle {
    let meta = format!("{} · {}", source_label(&m.source), m.clock);
    (
        ConsoleRow(m.seq),
        Node {
            padding: UiRect::axes(px(SPACE[5]), px(SPACE[3])),
            border: UiRect::bottom(px(border::HAIRLINE)),
            align_items: AlignItems::Start,
            flex_shrink: 0.0,
            display: display(shown),
            ..row(SPACE[4])
        },
        Stroke(ColorRole::LineSoft),
        a11y(Role::ListItem, m.text.clone()),
        children![
            (
                Node {
                    width: px(DOT),
                    height: px(DOT),
                    flex_shrink: 0.0,
                    // Centred on the message's first line.
                    margin: UiRect::top(px(SPACE[2])),
                    border_radius: rounded(DOT / 2.0),
                    ..default()
                },
                Fill(level_color(m.level)),
            ),
            (
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    ..column(SPACE[1])
                },
                children![
                    text(TypeStyle::BODY, ColorRole::Text, m.text.clone()),
                    text(TypeStyle::MONO_SMALL, ColorRole::Text3, meta),
                ],
            ),
        ],
    )
}

/// Builds the footer's command line into the shell's footer row.
pub(super) fn build_footer(ev: On<Add<ConsoleFooter>>, theme: Res<Theme>, mut commands: Commands) {
    commands
        .entity(ev.entity)
        .with_child((command_field(&theme, "Command", PLACEHOLDER), CommandLine));
}
