//! The board's component strip (board 06): a top bar, the activity
//! buttons, the viewport rail, a static menu and an inspector card.

use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use super::states::STATES;
use super::{card_node, sample_row};
use crate::assets::brand::logo;
use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::theme::visuals::{Active, ForceState};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::button::TextButtonProps;
use crate::widgets::chip::dot;
use crate::widgets::layout::{column, flex_spacer, row, separator, Orientation, BAR_SEPARATOR_H};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::MenuItemProps;
use crate::widgets::{
    activity_button, button, icon_button, menu_checkbox, menu_item, menu_panel, number_field,
    primary_button, rounded, segmented_group, text_button, toggle_row, tool_button, AxisTag,
    BadgeKind, ButtonVariant, IconButtonSize, NumberField, Tooltip, TooltipSide,
};

/// The number field the gallery focuses to show the focus look.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct FocusExample;

pub(super) fn top_bar(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    c.spawn(card_node(Node {
        height: px(theme.s.top_bar_h + 2.0 * border::HAIRLINE + SPACE[4]),
        padding: UiRect {
            left: px(SPACE[5]),
            right: px(SPACE[5]),
            ..default()
        },
        ..row(SPACE[4])
    }))
    .with_children(|b| {
        b.spawn(logo(theme.s.logo));
        b.spawn(text_button(
            theme,
            TextButtonProps::new("warehouse_demo")
                .dot(ColorRole::Warn)
                .chevron()
                .large(),
        ));
        b.spawn(separator(Orientation::Vertical, BAR_SEPARATOR_H, SPACE[2]));
        b.spawn(segmented_group("Tools", true, false))
            .with_children(|g| {
                g.spawn(tool_button(
                    theme,
                    IconId::Navigate,
                    Tooltip::new("Navigate").binding(map, "tool.navigate"),
                    true,
                ));
                g.spawn(tool_button(
                    theme,
                    IconId::Plus,
                    Tooltip::new("Translate").binding(map, "tool.translate"),
                    false,
                ));
                g.spawn(tool_button(
                    theme,
                    IconId::Rotate,
                    Tooltip::new("Rotate").binding(map, "tool.rotate"),
                    false,
                ));
            });
        b.spawn(flex_spacer());
        b.spawn(primary_button(
            theme,
            IconId::Pause,
            Tooltip::new("Pause").binding(map, "sim.play_pause"),
        ));
        b.spawn(text(TypeStyle::CLOCK, ColorRole::Text, "00:02:14.380"));
        b.spawn(sample_row(SPACE[2] + 1.0)).with_children(|r| {
            r.spawn(dot(theme.s.status_dot, ColorRole::Ok));
            r.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text2, "RTF 98%"));
        });
    });
}

pub(super) fn activity_card(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    r.spawn(card_node(Node {
        padding: UiRect::all(px(SPACE[3])),
        ..column(SPACE[2])
    }))
    .with_children(|a| {
        a.spawn((
            activity_button(
                theme,
                IconId::ActivityScene,
                Tooltip::new("Scene").binding(map, "panel.scene"),
                None,
            ),
            Active,
        ));
        a.spawn((
            activity_button(
                theme,
                IconId::ActivityAssets,
                Tooltip::new("Assets").binding(map, "panel.assets"),
                None,
            ),
            ForceState(STATES[1].1),
        ));
        a.spawn(activity_button(
            theme,
            IconId::ActivityPlot,
            Tooltip::new("Plot").binding(map, "panel.plot"),
            Some((BadgeKind::Count, 2)),
        ));
        a.spawn(activity_button(
            theme,
            IconId::ActivityConsole,
            Tooltip::new("Console").binding(map, "panel.console"),
            Some((BadgeKind::Warning, 2)),
        ));
    });
}

pub(super) fn rail_card(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let tip = |id: &str, name: &str| Tooltip::new(name).binding(map, id).side(TooltipSide::Left);
    // The rail floats over a view: show it on a patch of `bg`.
    r.spawn((
        Node {
            padding: UiRect::all(px(SPACE[3])),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..default()
        },
        Fill(ColorRole::Hover),
    ))
    .with_children(|v| {
        v.spawn((
            Node {
                width: px(theme.s.rail_w),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(0), px(SPACE[2] - 1.0)),
                row_gap: px(SPACE[1]),
                border: UiRect::all(px(border::HAIRLINE)),
                border_radius: rounded(11.0),
                ..default()
            },
            Fill(ColorRole::Hud),
            Stroke(ColorRole::Line),
        ))
        .with_children(|rail| {
            rail.spawn((
                icon_button(
                    theme,
                    IconId::CamOrbit,
                    IconButtonSize::Rail,
                    tip("", "Orbit"),
                ),
                Active,
            ));
            rail.spawn(icon_button(
                theme,
                IconId::CamPan,
                IconButtonSize::Rail,
                tip("", "Pan"),
            ));
            rail.spawn(separator(Orientation::Horizontal, 22.0, 3.0));
            rail.spawn((
                icon_button(
                    theme,
                    IconId::CameraMode,
                    IconButtonSize::Rail,
                    tip("view.follow", "Camera: Follow amr_01"),
                ),
                Active,
            ))
            .with_children(|b| {
                let d = theme.s.status_dot;
                b.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(4),
                        right: px(4),
                        width: px(d),
                        height: px(d),
                        border_radius: rounded(d / 2.0),
                        ..default()
                    },
                    Fill(ColorRole::Accent),
                ));
            });
            rail.spawn(icon_button(
                theme,
                IconId::Overlays,
                IconButtonSize::Rail,
                tip("", "Overlays"),
            ));
        });
    });
}

pub(super) fn static_menu(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let keys = |id: &str| shortcut_label(map, id);
    r.spawn(menu_panel(theme))
        .insert(Node {
            width: px(222),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(SPACE[3])),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|m| {
            m.spawn((
                menu_item(
                    theme,
                    MenuItemProps::new("Perspective").shortcut_opt(keys("view.projection")),
                ),
                Active,
            ));
            m.spawn((
                menu_item(
                    theme,
                    MenuItemProps::new("Follow amr_01").shortcut_opt(keys("view.follow")),
                ),
                ForceState(STATES[1].1),
            ));
            m.spawn(menu_checkbox(
                theme,
                MenuItemProps::new("Grid").shortcut_opt(keys("overlay.grid")),
                true,
            ));
            m.spawn(menu_checkbox(
                theme,
                MenuItemProps::new("Collisions").shortcut_opt(keys("overlay.collisions")),
                false,
            ));
        });
}

pub(super) fn inspector_card(r: &mut ChildSpawnerCommands, theme: &Theme) {
    r.spawn(card_node(Node {
        width: px(236),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(SPACE[7])),
        row_gap: px(SPACE[2]),
        ..default()
    }))
    .with_children(|i| {
        i.spawn(sample_row(SPACE[3])).with_children(|f| {
            f.spawn(number_field(
                theme,
                AxisTag::X,
                NumberField::new(2.4),
                false,
            ));
            f.spawn((
                number_field(theme, AxisTag::Y, NumberField::new(-0.75), false),
                FocusExample,
            ));
        });
        i.spawn(toggle_row(theme, "Gravity", true));
        i.spawn(toggle_row(theme, "Static", false));
        i.spawn(Node {
            margin: UiRect::top(px(SPACE[2])),
            column_gap: px(SPACE[3]),
            ..default()
        })
        .with_children(|b| {
            b.spawn(button(theme, "Duplicate", ButtonVariant::Normal));
            b.spawn(button(theme, "Delete", ButtonVariant::Destructive));
        });
    });
}
