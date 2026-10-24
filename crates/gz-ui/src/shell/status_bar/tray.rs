//! The message tray (spec §7.5; DECISIONS.md D-110): the status bar's
//! warning or error chip opens it upward, 560 wide. Header: "System
//! messages", the counts ("2 warn · 1 err"), Open console › and Clear.
//! Rows, newest first: the level's 8 px dot, the message, the time in mono
//! 11. It lists the warnings and errors the log still holds (the Console
//! has everything); Clear drops those so far from it and from the chip,
//! not from the Console.
//!
//! The message slot is a [`Menu`](crate::widgets::menu::Menu) whose
//! trigger is the problem chip (rebuilt as messages come), so the tray
//! closes like a menu: Esc, a press outside, or a choice.

use accesskit::Role;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, MenuAction, MenuEvent};
use gz_bevy::messages::{MessageLevel, SystemMessage, SystemMessages};

use crate::assets::fonts::text;
use crate::panels::{PanelRegistry, CONSOLE};
use crate::shell::{Slot, UiCommand};
use crate::theme::{border, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, flex_spacer, row};
use crate::widgets::menu::{popup, MenuPopupOf, MenuSide};
use crate::widgets::{a11y, rounded, text_button, TextButtonProps};

/// The tray's width (§7.5).
pub(crate) const TRAY_W: f32 = 560.0;

/// How tall its list grows before it scrolls.
const LIST_MAX_H: f32 = 320.0;

/// The rows' dots.
const DOT: f32 = 8.0;

/// The tray's list of rows.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TrayList;

/// Its header's counts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TrayCounts;

/// One row: the message it shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrayRow(pub(crate) u64);

/// The header's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayButton {
    OpenConsole,
    Clear,
}

/// A level's dot colour (the Console's).
fn level_color(level: MessageLevel) -> ColorRole {
    match level {
        MessageLevel::Error => ColorRole::Danger,
        MessageLevel::Warning => ColorRole::Warn,
        MessageLevel::Info => ColorRole::Info,
        MessageLevel::Debug => ColorRole::Text3,
    }
}

/// "2 warn · 1 err", "No warnings or errors".
pub(crate) fn counts_text(messages: &SystemMessages) -> String {
    let (w, e) = messages.problem_counts();
    match (w, e) {
        (0, 0) => "none".into(),
        (w, 0) => format!("{w} warn"),
        (0, e) => format!("{e} err"),
        (w, e) => format!("{w} warn · {e} err"),
    }
}

/// The tray: the message slot's popup.
pub(crate) fn tray(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    popup(theme, MenuSide::Above, move |p: &mut ChildSpawner| {
        p.spawn((
            Node {
                width: px(TRAY_W - 2.0 * SPACE[3]),
                max_width: Val::Vw(90.0),
                ..column(0.0)
            },
            a11y(Role::Group, "System messages"),
        ))
        .with_children(|c| {
            c.spawn(Node {
                padding: UiRect::new(px(SPACE[5]), px(SPACE[2]), px(SPACE[2]), px(SPACE[3])),
                align_items: AlignItems::Center,
                ..row(SPACE[4])
            })
            .with_children(|h| {
                h.spawn(text(
                    TypeStyle::new(FontFamily::Ui, 13.0, 600),
                    ColorRole::Text,
                    "System messages",
                ));
                h.spawn((
                    text(TypeStyle::MONO_SMALL, ColorRole::Text3, ""),
                    TrayCounts,
                ));
                h.spawn(flex_spacer());
                h.spawn((
                    text_button(&t, TextButtonProps::new("Open console ›")),
                    TrayButton::OpenConsole,
                ));
                h.spawn((
                    text_button(&t, TextButtonProps::new("Clear")),
                    TrayButton::Clear,
                ));
            });
            c.spawn((
                Node {
                    max_height: px(LIST_MAX_H),
                    overflow: Overflow::scroll_y(),
                    border: UiRect::top(px(border::HAIRLINE)),
                    ..column(0.0)
                },
                Stroke(ColorRole::LineSoft),
                ScrollPosition::default(),
                TrayList,
                a11y(Role::List, "Messages"),
            ));
        });
    })
}

/// A row: the dot, the message, the time.
fn tray_row(m: &SystemMessage) -> impl Bundle {
    (
        TrayRow(m.seq),
        Node {
            padding: UiRect::axes(px(SPACE[5]), px(SPACE[3])),
            border: UiRect::bottom(px(border::HAIRLINE)),
            align_items: AlignItems::Start,
            flex_shrink: 0.0,
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
                text(TypeStyle::BODY, ColorRole::Text, m.text.clone()),
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    ..default()
                },
            ),
            text(TypeStyle::MONO_SMALL, ColorRole::Text3, m.clock.clone()),
        ],
    )
}

/// The rows and counts follow the log while the tray shows ("No warnings
/// or errors" with none).
#[allow(clippy::type_complexity)]
pub(crate) fn tray_rows(
    messages: Option<Res<SystemMessages>>,
    popups: Query<(&Visibility, &ChildOf), (With<MenuPopupOf>, Changed<Visibility>)>,
    lists: Query<(Entity, Option<&Children>), With<TrayList>>,
    mut counts: Query<&mut Text, With<TrayCounts>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Some(messages) = messages else {
        return;
    };
    let Ok((list, children)) = lists.single() else {
        return;
    };
    // Its popup just opened, or the log changed.
    let tray = parents.iter_ancestors(list).find(|a| popups.contains(*a));
    let opened = tray.is_some_and(|t| popups.get(t).is_ok_and(|(v, _)| *v == Visibility::Visible));
    if !opened && !messages.is_changed() {
        return;
    }
    for mut t in &mut counts {
        crate::widgets::text::set_text(&mut t, &counts_text(&messages));
    }
    for &c in children.into_iter().flatten() {
        commands.entity(c).despawn();
    }
    commands.entity(list).with_children(|l| {
        let mut any = false;
        for m in messages.problems() {
            l.spawn(tray_row(m));
            any = true;
        }
        if !any {
            l.spawn((
                Node {
                    padding: UiRect::all(px(SPACE[6])),
                    ..default()
                },
                children![text(
                    TypeStyle::BODY,
                    ColorRole::Text3,
                    "No warnings or errors"
                )],
            ));
        }
    });
}

/// Open console › opens the Console; Clear drops the problems so far.
/// Either closes the tray.
pub(crate) fn on_tray_button(
    ev: On<Activate>,
    buttons: Query<&TrayButton>,
    panels: Res<PanelRegistry>,
    messages: Option<ResMut<SystemMessages>>,
    mut ui: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(ev.entity) else {
        return;
    };
    match button {
        TrayButton::OpenConsole => {
            if panels.get(CONSOLE).is_some() {
                ui.write(UiCommand::OpenPanel {
                    panel: CONSOLE,
                    slot: Slot::Primary,
                });
            }
        }
        TrayButton::Clear => {
            if let Some(mut m) = messages {
                m.clear_problems();
            }
        }
    }
    commands.trigger(MenuEvent {
        source: ev.entity,
        action: MenuAction::CloseAll,
    });
}
