//! The command palette's look (spec §7.7): centered, 560 wide, 15% from
//! the window's top, over everything but tooltips; a 44px search field,
//! then the results under their groups' labels, each with its icon, name,
//! hint in `text_3` and shortcut at the right. The chosen result shows
//! `accent_soft` like a menu's current item; an unavailable one is dimmed.

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, ScrollArea};

use super::model::Item;
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::visuals::{Content, OwnInk, Visuals};
use crate::theme::{border, elevation, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, flex_spacer, row};
use crate::widgets::menu::menu_group_label;
use crate::widgets::text_field::search_field;
use crate::widgets::{a11y, clickable, rounded};

/// The palette's width (§7.7).
pub(crate) const WIDTH: f32 = 560.0;
/// How far down the window it opens (§7.7: 15%).
pub(crate) const TOP: f32 = 15.0;
/// The search field's height (§7.7).
pub(crate) const FIELD_H: f32 = 44.0;
/// The results' most height, in the window's height.
const RESULTS_VH: f32 = 55.0;

/// The palette's root: the window-sized layer it sits in.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PaletteRoot;

/// Under the card: a press there closes the palette.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PaletteBackdrop;

/// The search field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PaletteField;

/// The results' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PaletteRows;

/// A result's row: its index in the results.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaletteRow(pub usize);

/// The palette, empty: [`systems`](super::systems) fills its rows.
pub(crate) fn palette() -> impl Bundle {
    let (shadow, tint) = elevation::MENU.shadow();
    (
        PaletteRoot,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::new(px(SPACE[6]), px(SPACE[6]), Val::Vh(TOP), px(0)),
            align_items: AlignItems::Center,
            ..column(0.0)
        },
        GlobalZIndex(950),
        children![
            (
                PaletteBackdrop,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
            ),
            (
                Node {
                    width: px(WIDTH),
                    max_width: Val::Percent(100.0),
                    border: UiRect::all(px(border::HAIRLINE)),
                    border_radius: rounded(radius::LG),
                    overflow: Overflow::clip(),
                    ..column(0.0)
                },
                Fill(ColorRole::Surface),
                Stroke(ColorRole::Line),
                shadow,
                tint,
                a11y(Role::Dialog, "Command palette"),
                children![
                    (
                        Node {
                            padding: UiRect::all(px(SPACE[3])),
                            border: UiRect::bottom(px(border::HAIRLINE)),
                            flex_shrink: 0.0,
                            ..row(0.0)
                        },
                        Stroke(ColorRole::LineSoft),
                        children![(
                            search_field(FIELD_H, "Search or run a command"),
                            PaletteField,
                        )],
                    ),
                    (
                        PaletteRows,
                        ScrollArea,
                        ScrollPosition::default(),
                        AccessibilityNode(accesskit::Node::new(Role::ListBox)),
                        Node {
                            max_height: Val::Vh(RESULTS_VH),
                            overflow: Overflow::scroll_y(),
                            padding: UiRect::all(px(SPACE[3])),
                            ..column(0.0)
                        },
                    ),
                ],
            ),
        ],
    )
}

/// A result's row: icon, name, hint and shortcut; an unavailable one
/// shows its reason in the hint's place (and its spawner makes it inert).
pub(crate) fn result_row(theme: &Theme, i: usize, item: &Item) -> impl Bundle {
    let visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    let hint = match item.disabled {
        Some(why) => why.to_string(),
        None => item.hint.clone(),
    };
    let icon_px = theme.s.icon_sm;
    let (name, shortcut, id): (String, Option<String>, IconId) =
        (item.name.clone(), item.shortcut.clone(), item.icon);
    (
        PaletteRow(i),
        Node {
            height: px(theme.s.menu_item_h),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[5])),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..row(SPACE[5])
        },
        Headless,
        clickable(visuals, Role::ListBoxOption, name.clone()),
        children![
            (icon(id, icon_px, ColorRole::Text2), Content, OwnInk),
            (
                text(TypeStyle::BODY, ColorRole::Text, name),
                Content,
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ),
            (
                text(TypeStyle::SMALL, ColorRole::Text3, hint),
                Content,
                OwnInk,
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ),
            flex_spacer(),
            (
                text(
                    crate::widgets::menu::SHORTCUT_TYPE,
                    ColorRole::Text3,
                    shortcut.unwrap_or_default()
                ),
                Content,
                OwnInk,
            ),
        ],
    )
}

/// A group's label above its results.
pub(crate) fn group_label(label: &str) -> impl Bundle {
    menu_group_label(label.to_string())
}

/// "No results", when the query matches nothing.
pub(crate) fn no_results() -> impl Bundle {
    (
        text(TypeStyle::BODY, ColorRole::Text2, "No results"),
        Node {
            align_self: AlignSelf::Center,
            margin: UiRect::vertical(px(SPACE[6])),
            ..default()
        },
    )
}
