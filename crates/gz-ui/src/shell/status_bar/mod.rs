//! The status bar (spec §3.6): connection, physics, frame rate,
//! iterations, the message chip slot, then the tool hint, selection,
//! cursor position and the workspace button. Below 1280 px the hint and
//! cursor drop out, below 1100 px the frame rate and iterations too.

pub(crate) mod readouts;
pub(crate) mod tray;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as Headless};
use gz_bevy::view::input::InputMap;

use super::layout::{INSPECTOR_START_W, SIDEBAR_START_W};
use super::regions::{region, Region};
use super::{ShellLayout, ShellSystems, Tool};
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Active, Content, Visuals};
use crate::theme::{border, ColorRole, FontFamily, Theme, TypeStyle, SPACE};
use crate::widgets::button::ICON_BUTTON_RADIUS;
use crate::widgets::chip::dot;
use crate::widgets::layout::{flex_spacer, row, set_shown};
use crate::widgets::menu::{menu, menu_item, menu_separator, MenuItemProps, MenuSide};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::{clickable, rounded, Tooltip, TooltipSide};
pub(crate) use readouts::{ConnectionDot, CursorReadout, StatusText};

/// The status bar's items, numbered as in §3.6.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StatusItem {
    /// 1: dot and address.
    Connection,
    /// 2: "dart · 1 ms".
    Physics,
    /// 3: "60 fps".
    Fps,
    /// 4: "iter 134,380".
    Iterations,
    /// 5: the latest warning's or error's chip (only when one exists).
    Message,
    /// 7: the active tool's hint.
    Hint,
    /// 8: "sel: amr_01", "3 selected" or "sel: —".
    Selection,
    /// 9: "cursor 2.41, −0.73".
    Cursor,
    /// 10: the workspace switcher (M8).
    Workspace,
}

impl StatusItem {
    /// Whether the item shows in a window `width` wide (§3.6: below 1280
    /// drop 7 and 9 first, then 3 and 4; DECISIONS.md puts "then" at 1100).
    pub(crate) fn shown_at(self, width: f32) -> bool {
        match self {
            StatusItem::Hint | StatusItem::Cursor => width >= INSPECTOR_START_W,
            StatusItem::Fps | StatusItem::Iterations => width >= SIDEBAR_START_W,
            _ => true,
        }
    }
}

/// Mono 12 `text_2`, the bar's type (§3.6).
pub(crate) const BAR_TYPE: TypeStyle = TypeStyle::MONO_SMALL;
/// The hint's type: Geist 12 `text_3`.
pub(crate) const HINT_TYPE: TypeStyle = TypeStyle::SMALL;

fn item(which: StatusItem, style: TypeStyle, role: ColorRole, value: &str) -> impl Bundle {
    (
        which,
        Node {
            flex_shrink: 0.0,
            ..row(SPACE[3])
        },
        children![(
            text(style, role, value.to_string()),
            StatusText(which),
            TextLayout::no_wrap()
        )],
    )
}

/// The status bar.
pub(crate) fn status_bar(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let tip = workspace_tooltip(map);
    let map = map.clone();
    (
        region(
            Region::StatusBar,
            Node {
                height: px(theme.s.status_bar_h),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(px(SPACE[6])),
                border: UiRect::top(px(border::HAIRLINE)),
                overflow: Overflow::clip(),
                ..row(SPACE[8])
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((
                StatusItem::Connection,
                Node {
                    flex_shrink: 0.0,
                    ..row(SPACE[3])
                },
            ))
            .with_children(|c| {
                c.spawn((dot(t.s.status_dot, ColorRole::Danger), ConnectionDot));
                c.spawn((
                    text(BAR_TYPE, ColorRole::Text2, ""),
                    StatusText(StatusItem::Connection),
                    TextLayout::no_wrap(),
                ));
            });
            b.spawn(item(StatusItem::Physics, BAR_TYPE, ColorRole::Text2, "—"));
            b.spawn(item(StatusItem::Fps, BAR_TYPE, ColorRole::Text2, "— fps"));
            b.spawn(item(
                StatusItem::Iterations,
                BAR_TYPE,
                ColorRole::Text2,
                "iter —",
            ));
            // A menu: the problem chip opens the message tray (§7.5).
            b.spawn((
                StatusItem::Message,
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    max_width: px(t.s.message_chip_max_w),
                    ..default()
                },
                crate::widgets::menu::Menu,
                bevy::ui_widgets::observe(crate::widgets::menu::on_menu_event),
            ))
            .with_child(tray::tray(&t));
            b.spawn(flex_spacer());
            b.spawn(item(
                StatusItem::Hint,
                HINT_TYPE,
                ColorRole::Text3,
                Tool::Navigate.hint(),
            ));
            b.spawn(item(
                StatusItem::Selection,
                BAR_TYPE,
                ColorRole::Text2,
                "sel: —",
            ));
            b.spawn(item(
                StatusItem::Cursor,
                BAR_TYPE,
                ColorRole::Text2,
                "cursor —",
            ));
            b.spawn(switcher(&t, &map, tip));
        })),
    )
}

/// The workspace button's tooltip: its shortcuts from the input map
/// ("Alt+1–4", "Opt+1–4" on macOS).
pub(crate) fn workspace_tooltip(map: &InputMap) -> Tooltip {
    workspace_tooltip_for(map, "Build")
}

/// The workspace button's tooltip for the workspace `name`.
pub(crate) fn workspace_tooltip_for(map: &InputMap, name: &str) -> Tooltip {
    let first = shortcut_label(map, "ui.workspace_1");
    let last = shortcut_label(map, "ui.workspace_4");
    let keys = match (first, last) {
        (Some(a), Some(b)) => Some(match (a.strip_suffix('1'), b.strip_suffix('4')) {
            (Some(p), Some(q)) if p == q => format!("{p}1–4"),
            _ => format!("{a}–{b}"),
        }),
        (a, b) => a.or(b),
    };
    Tooltip::new(format!("Workspace: {name}"))
        .detail("Click to switch")
        .shortcut_opt(keys)
        .side(TooltipSide::Above)
}

/// The workspace switcher's items (§7.8).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwitcherItem {
    Switch(usize),
    Save,
    Manage,
}

/// The workspace button's name.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WorkspaceName;

/// The switcher's separator: the user's workspaces go before it.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SwitcherSeparator;

/// One of the user's workspaces in the switcher: its name.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UserItem(String);

/// The workspace switcher (§7.8): the workspace button opens it upward.
/// Build Alt+1 · Run Alt+2 · Debug Alt+3 · Review Alt+4, then the user's
/// own ([`switcher_items`]; the current one marked) · Save current layout ·
/// Manage… (the Workspaces panel).
fn switcher(theme: &Theme, map: &InputMap, tip: Tooltip) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    menu(theme, MenuSide::Above, workspace_button(tip), move |p| {
        for (i, (_, name, binding)) in crate::shell::workspaces::WORKSPACES.iter().enumerate() {
            p.spawn((
                menu_item(
                    &t,
                    MenuItemProps::new(*name).shortcut_opt(shortcut_label(&map, binding)),
                ),
                SwitcherItem::Switch(i),
            ));
        }
        p.spawn((menu_separator(), SwitcherSeparator));
        p.spawn((
            menu_item(&t, MenuItemProps::new("Save current layout")),
            SwitcherItem::Save,
        ));
        p.spawn((
            menu_item(&t, MenuItemProps::new("Manage…")),
            SwitcherItem::Manage,
        ));
    })
}

/// The user's workspaces in the switcher, before its separator, as the
/// list has them.
pub(crate) fn switcher_items(
    workspaces: Res<crate::shell::Workspaces>,
    theme: Res<Theme>,
    items: Query<(Entity, &SwitcherItem, &UserItem)>,
    separators: Query<&ChildOf, With<SwitcherSeparator>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if !workspaces.is_changed() {
        return;
    }
    let want: Vec<(usize, &str)> = workspaces
        .list()
        .iter()
        .enumerate()
        .filter(|(_, w)| w.builtin.is_none())
        .map(|(i, w)| (i, w.name.as_str()))
        .collect();
    let mut have: Vec<(usize, &str)> = items
        .iter()
        .filter_map(|(_, item, name)| match item {
            SwitcherItem::Switch(i) => Some((*i, name.0.as_str())),
            _ => None,
        })
        .collect();
    have.sort_unstable();
    if have == want {
        return;
    }
    for (e, ..) in &items {
        commands.entity(e).despawn();
    }
    let Ok(popup) = separators.single().map(ChildOf::parent) else {
        return;
    };
    // Counted without the items going, which are despawned only later.
    let at = children
        .get(popup)
        .ok()
        .and_then(|c| {
            c.iter()
                .filter(|e| !items.contains(*e))
                .position(|e| separators.contains(e))
        })
        .unwrap_or(0);
    let spawned: Vec<Entity> = want
        .iter()
        .map(|(i, name)| {
            commands
                .spawn((
                    menu_item(&theme, MenuItemProps::new(*name)),
                    SwitcherItem::Switch(*i),
                    UserItem(name.to_string()),
                ))
                .id()
        })
        .collect();
    commands.entity(popup).insert_children(at, &spawned);
}

/// The button names the current workspace (and its tooltip); the
/// switcher marks it.
pub(crate) fn switcher_states(
    workspaces: Res<crate::shell::Workspaces>,
    map: Option<Res<InputMap>>,
    mut names: Query<&mut Text, With<WorkspaceName>>,
    mut buttons: Query<&mut Tooltip, With<WorkspaceButton>>,
    items: Query<(Entity, &SwitcherItem, Has<Active>)>,
    added: Query<(), Added<SwitcherItem>>,
    mut commands: Commands,
) {
    if !workspaces.is_changed() && added.is_empty() {
        return;
    }
    let name = workspaces.name();
    for mut t in &mut names {
        set_text(&mut t, name);
    }
    if let Some(map) = map {
        for mut tip in &mut buttons {
            *tip = workspace_tooltip_for(&map, name);
        }
    }
    for (e, item, active) in &items {
        let want = *item == SwitcherItem::Switch(workspaces.current);
        if want && !active {
            commands.entity(e).insert(Active);
        } else if !want && active {
            commands.entity(e).remove::<Active>();
        }
    }
}

/// A switcher item switches or saves.
pub(crate) fn on_switcher_item(
    ev: On<Activate>,
    items: Query<&SwitcherItem, Without<InteractionDisabled>>,
    mut out: MessageWriter<crate::shell::WorkspaceCommand>,
    mut ui: MessageWriter<super::UiCommand>,
) {
    match items.get(ev.entity) {
        Ok(SwitcherItem::Switch(i)) => {
            out.write(crate::shell::WorkspaceCommand::Switch(*i));
        }
        Ok(SwitcherItem::Save) => {
            out.write(crate::shell::WorkspaceCommand::Save);
        }
        Ok(SwitcherItem::Manage) => {
            ui.write(super::UiCommand::OpenPanel {
                panel: crate::shell::WORKSPACES_PANEL,
                slot: super::Slot::Primary,
            });
        }
        Err(_) => {}
    }
}

/// The workspace button (the switcher's trigger).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WorkspaceButton;

/// The workspace button: icon, name and chevron_up, 22 tall (§3.6 item
/// 10).
fn workspace_button(tip: Tooltip) -> impl Bundle {
    let label = TypeStyle::new(FontFamily::Ui, 12.0, 500);
    (
        StatusItem::Workspace,
        WorkspaceButton,
        Node {
            height: px(22),
            padding: UiRect::horizontal(px(SPACE[3])),
            flex_shrink: 0.0,
            border_radius: rounded(ICON_BUTTON_RADIUS - 2.0),
            ..row(SPACE[2] + 1.0)
        },
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text2),
            accesskit::Role::Button,
            "Workspace",
        ),
        tip,
        children![
            content_icon(IconId::ActivityWorkspaces, 12.0, ColorRole::Text2),
            (
                text(label, ColorRole::Text2, "Build"),
                Content,
                WorkspaceName
            ),
            content_icon(IconId::ChevronUp, 10.0, ColorRole::Text2),
        ],
    )
}

/// Drops items on narrow windows (§3.6).
pub(crate) fn narrow_rules(layout: Res<ShellLayout>, mut items: Query<(&StatusItem, &mut Node)>) {
    if !layout.is_changed() {
        return;
    }
    for (which, mut node) in &mut items {
        set_shown(&mut node, which.shown_at(layout.window_w));
    }
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<CursorReadout>()
        .init_resource::<readouts::StatusNote>()
        .add_message::<gz_bevy::undo::UndoCommand>()
        .add_observer(readouts::on_undo_chip)
        .add_observer(tray::on_tray_button)
        .add_observer(on_switcher_item)
        .add_systems(
            Update,
            (
                // The cursor's ground point first, so its text is this frame's.
                (readouts::cursor_readout, readouts::status_texts).chain(),
                readouts::connection_dot,
                (readouts::message_chip, tray::tray_rows).chain(),
                narrow_rules,
                (switcher_items, switcher_states).chain(),
            )
                .in_set(ShellSystems::Readouts),
        );
}

#[cfg(test)]
mod tests;
