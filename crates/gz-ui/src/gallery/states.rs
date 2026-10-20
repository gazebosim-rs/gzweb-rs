//! The states grid: every clickable widget in its default, hover,
//! pressed, active and disabled looks, and with the focus ring.

use bevy::prelude::*;

use super::{card_node, sample_row};
use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::theme::visuals::{ForceState, WidgetState};
use crate::theme::{border, radius, ColorRole, FocusRing, Ring, Theme, TypeStyle, SPACE};
use crate::widgets::button::TextButtonProps;
use crate::widgets::MenuItemProps;
use crate::widgets::{
    activity_button, button, filter_chip, icon_button, menu_item, rounded, text_button,
    ButtonVariant, IconButtonSize, Tooltip,
};

const fn state(hovered: bool, pressed: bool, active: bool, disabled: bool) -> WidgetState {
    WidgetState {
        hovered,
        pressed,
        active,
        disabled,
    }
}

/// The states shown in the states grid (plus a Focus column).
pub const STATES: [(&str, WidgetState); 5] = [
    ("Default", state(false, false, false, false)),
    ("Hover", state(true, false, false, false)),
    ("Pressed", state(true, true, false, false)),
    ("Active", state(false, false, true, false)),
    ("Disabled", state(false, false, false, true)),
];

/// Draws a widget's focus ring permanently (the states grid's Focus column).
fn show_focus(commands: &mut Commands, e: Entity) {
    commands.entity(e).remove::<FocusRing>().insert((
        Ring(ColorRole::Accent),
        Outline::new(px(border::FOCUS_RING), px(2), Color::NONE),
    ));
}

/// One sample widget for a states-grid row.
#[derive(Clone, Copy, Debug)]
enum Sample {
    Icon32,
    Icon28,
    Rail,
    Text,
    Bordered,
    Destructive,
    Chip,
    Item,
    Activity,
}

impl Sample {
    const ALL: [(Sample, &'static str); 9] = [
        (Sample::Icon32, "IconButton 32"),
        (Sample::Icon28, "IconButton 28"),
        (Sample::Rail, "Rail 32"),
        (Sample::Text, "TextButton"),
        (Sample::Bordered, "Button"),
        (Sample::Destructive, "Destructive"),
        (Sample::Chip, "Filter chip"),
        (Sample::Item, "Menu item"),
        (Sample::Activity, "Activity"),
    ];

    fn spawn(self, p: &mut ChildSpawnerCommands, t: &Theme) -> Entity {
        match self {
            Sample::Icon32 => p.spawn(icon_button(
                t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("Settings"),
            )),
            Sample::Icon28 => p.spawn(icon_button(
                t,
                IconId::Filter,
                IconButtonSize::Small,
                Tooltip::new("Filter"),
            )),
            Sample::Rail => p.spawn(icon_button(
                t,
                IconId::CamPan,
                IconButtonSize::Rail,
                Tooltip::new("Pan"),
            )),
            Sample::Text => p.spawn(text_button(t, TextButtonProps::new("World").chevron())),
            Sample::Bordered => p.spawn(button(t, "Duplicate", ButtonVariant::Normal)),
            Sample::Destructive => p.spawn(button(t, "Delete", ButtonVariant::Destructive)),
            Sample::Chip => p.spawn(filter_chip("Models", false)),
            Sample::Item => {
                let mut e = p.spawn(menu_item(t, MenuItemProps::new("Grid").shortcut("G")));
                e.insert(Node {
                    width: px(84),
                    height: px(t.s.menu_item_h),
                    padding: UiRect::horizontal(px(SPACE[5])),
                    column_gap: px(SPACE[5]),
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::SM),
                    ..default()
                });
                e
            }
            Sample::Activity => p.spawn(activity_button(
                t,
                IconId::ActivityTopics,
                Tooltip::new("Topics"),
                None,
            )),
        }
        .id()
    }
}

pub(super) fn states_grid(c: &mut ChildSpawnerCommands, theme: &Theme) {
    const LABEL_W: f32 = 104.0;
    const CELL_W: f32 = 92.0;
    let caps = |t: &str, w: f32| {
        (
            text(TypeStyle::CAPS, ColorRole::Text3, t.to_string()),
            Node {
                width: px(w),
                ..default()
            },
        )
    };
    c.spawn(card_node(Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(SPACE[7]), px(SPACE[5])),
        row_gap: px(SPACE[1]),
        ..default()
    }))
    .with_children(|g| {
        g.spawn(sample_row(0.0)).with_children(|h| {
            h.spawn(caps("State", LABEL_W));
            for (name, _) in STATES {
                h.spawn(caps(name, CELL_W));
            }
            h.spawn(caps("Focus", CELL_W));
        });
        // Room for the Focus column's ring (offset 2, width 2) above and
        // below, so neighbouring rings don't touch.
        let ring = 2.0 + border::FOCUS_RING;
        for (sample, label) in Sample::ALL {
            let h = match sample {
                Sample::Activity => theme.s.activity_btn,
                _ => theme.s.icon_btn,
            } + 2.0 * ring
                + SPACE[1];
            g.spawn(Node {
                height: px(h),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    text(TypeStyle::SMALL, ColorRole::Text2, label),
                    Node {
                        width: px(LABEL_W),
                        ..default()
                    },
                ));
                for (_, s) in STATES {
                    r.spawn(Node {
                        width: px(CELL_W),
                        ..default()
                    })
                    .with_children(|cell| {
                        let e = sample.spawn(cell, theme);
                        cell.commands().entity(e).insert(ForceState(s));
                    });
                }
                r.spawn(Node {
                    width: px(CELL_W),
                    padding: UiRect::left(px(SPACE[1])),
                    ..default()
                })
                .with_children(|cell| {
                    let e = sample.spawn(cell, theme);
                    let mut commands = cell.commands();
                    commands.entity(e).insert(ForceState(STATES[0].1));
                    show_focus(&mut commands, e);
                });
            });
        }
    });
}
