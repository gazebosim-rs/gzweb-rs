//! The rest of the widgets, below the token list: a Scene panel of tree
//! rows beside a splitter, a slider, a select, a popover, a menu with
//! icons, chips, badges and status chips.

use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use super::sample_row;
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Theme, TypeStyle, SPACE};
use crate::widgets::button::{text_button_parts, TextButtonProps};
use crate::widgets::layout::flex_spacer;
use crate::widgets::menu::{menu, menu_group_label, MenuSide};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::MenuItemProps;
use crate::widgets::{
    badge, filter_chip, icon_button, menu_checkbox, menu_item, menu_separator, number_field,
    section_header, select, slider_row, status_chip, tree_row, AxisTag, BadgeKind, EntityKind,
    IconButtonSize, NumberField, SliderProps, StatusKind, Tooltip, TreeRowSpec, Unit,
};

pub(super) fn scene_panel(t: &mut ChildSpawnerCommands, theme: &Theme) {
    t.spawn(section_header(theme, "Scene", Some("14 entities".into())))
        .with_children(|h| {
            h.spawn(icon_button(
                theme,
                IconId::ExpandAll,
                IconButtonSize::Small,
                Tooltip::new("Expand all"),
            ));
            h.spawn(icon_button(
                theme,
                IconId::CollapseAll,
                IconButtonSize::Small,
                Tooltip::new("Collapse all"),
            ));
        });
    t.spawn(Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(SPACE[4])),
        ..default()
    })
    .with_children(|rows| {
        let spec = |depth, kind, name: &str, expanded, hidden| TreeRowSpec {
            depth,
            kind,
            name: name.into(),
            expanded,
            hidden,
            hideable: !matches!(kind, EntityKind::World | EntityKind::Joint),
        };
        rows.spawn(tree_row(
            theme,
            spec(0, EntityKind::World, "warehouse_demo", Some(true), false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(1, EntityKind::Light, "sun", None, false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(1, EntityKind::Light, "ceiling_light", None, true),
        ));
        rows.spawn((
            tree_row(
                theme,
                spec(1, EntityKind::Model, "amr_01", Some(true), false),
            ),
            Active,
        ));
        rows.spawn(tree_row(
            theme,
            spec(2, EntityKind::Link, "base_link", Some(false), false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(2, EntityKind::Joint, "wheel_left_joint", None, false),
        ));
    });
}

pub(super) fn more_widgets(w: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let mut linear = SliderProps::new("Linear", 0.0, 2.0, 0.6);
    linear.step = 0.05;
    w.spawn(slider_row(theme, linear));
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(select(
            theme,
            "Camera mode",
            vec!["Free".into(), "Follow amr_01".into(), "Track amr_01".into()],
            0,
            128.0,
        ));
        // A popover (§5.16): the Snap settings.
        let t = theme.clone();
        r.spawn(menu(
            theme,
            MenuSide::Below,
            text_button_parts(theme, TextButtonProps::new("Snap 0.25").mono().chevron()),
            move |p: &mut ChildSpawner| {
                p.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(SPACE[3]),
                        width: px(220),
                        padding: UiRect::all(px(SPACE[2])),
                        ..default()
                    },
                    children![
                        text(TypeStyle::CAPS, ColorRole::Text3, "Snap"),
                        number_field(
                            &t,
                            AxisTag::Label("move".into()),
                            NumberField::new(0.25).unit(Unit::Length),
                            false,
                        ),
                        number_field(
                            &t,
                            AxisTag::Label("turn".into()),
                            NumberField::new(15.0).unit(Unit::Angle).precision(1, 0.5),
                            false,
                        ),
                    ],
                ));
            },
        ));
        // A menu with icons, a group label, a separator and a checkbox.
        let t = theme.clone();
        let keys = |id: &str| shortcut_label(map, id);
        let (focus, home, follow) = (keys("view.focus"), keys("view.home"), keys("view.follow"));
        r.spawn(menu(
            theme,
            MenuSide::Below,
            text_button_parts(
                theme,
                TextButtonProps::new("Camera")
                    .leading(IconId::CameraMode)
                    .chevron(),
            ),
            move |p: &mut ChildSpawner| {
                p.spawn(menu_group_label("View"));
                p.spawn(menu_item(
                    &t,
                    MenuItemProps::new("Focus")
                        .icon(IconId::Focus)
                        .shortcut_opt(focus),
                ));
                p.spawn(menu_item(
                    &t,
                    MenuItemProps::new("Home")
                        .icon(IconId::Home)
                        .shortcut_opt(home),
                ));
                p.spawn(menu_separator());
                p.spawn((
                    menu_item(
                        &t,
                        MenuItemProps::new("Follow")
                            .icon(IconId::Follow)
                            .shortcut_opt(follow),
                    ),
                    Active,
                ));
                p.spawn(menu_checkbox(&t, MenuItemProps::new("Link cameras"), false));
            },
        ));
    });
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(filter_chip("All", true));
        r.spawn(filter_chip("Models", false));
        r.spawn(filter_chip("Lights", false));
        r.spawn(flex_spacer());
        r.spawn(badge(theme, BadgeKind::Count, 3));
        r.spawn(badge(theme, BadgeKind::Warning, 12));
        r.spawn(badge(theme, BadgeKind::Error, 1));
    });
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(status_chip(
            theme,
            StatusKind::Error,
            "Lost connection",
            None,
        ));
        r.spawn(status_chip(
            theme,
            StatusKind::Info,
            "Saved",
            Some("1".into()),
        ));
        r.spawn(icon(IconId::Warning, 15.0, ColorRole::WarnText));
    });
}
