//! The status bar (spec §3.6): connection, physics, frame rate,
//! iterations, the message chip slot, then the tool hint, selection,
//! cursor position and the workspace button. Below 1280 px the hint and
//! cursor drop out, below 1100 px the frame rate and iterations too.

pub(crate) mod readouts;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;
use gz_bevy::view::input::InputMap;

use super::layout::{INSPECTOR_START_W, SIDEBAR_START_W};
use super::regions::{region, Region};
use super::{ShellLayout, ShellSystems, Tool};
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{border, ColorRole, FontFamily, Theme, TypeStyle, SPACE};
use crate::widgets::button::ICON_BUTTON_RADIUS;
use crate::widgets::chip::dot;
use crate::widgets::layout::{flex_spacer, row, set_shown};
use crate::widgets::shortcut::shortcut_label;
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
    /// 5: the latest message's chip (none yet; the slot stays empty).
    Message,
    /// 7: the active tool's hint.
    Hint,
    /// 8: "sel: —".
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
            b.spawn((
                StatusItem::Message,
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    max_width: px(t.s.message_chip_max_w),
                    ..default()
                },
            ));
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
            b.spawn(workspace_button(&t, tip));
        })),
    )
}

/// The workspace button's tooltip: its shortcuts from the input map
/// ("Alt+1–4", "Opt+1–4" on macOS).
pub(crate) fn workspace_tooltip(map: &InputMap) -> Tooltip {
    let first = shortcut_label(map, "ui.workspace_1");
    let last = shortcut_label(map, "ui.workspace_4");
    let keys = match (first, last) {
        (Some(a), Some(b)) => Some(match (a.strip_suffix('1'), b.strip_suffix('4')) {
            (Some(p), Some(q)) if p == q => format!("{p}1–4"),
            _ => format!("{a}–{b}"),
        }),
        (a, b) => a.or(b),
    };
    Tooltip::new("Workspace: Build")
        .detail("Click to switch")
        .shortcut_opt(keys)
        .side(TooltipSide::Above)
}

/// The workspace button: icon, name and chevron_up, 22 tall (§3.6 item
/// 10). It opens the switcher in M8.
fn workspace_button(theme: &Theme, tip: Tooltip) -> impl Bundle {
    let _ = theme;
    let label = TypeStyle::new(FontFamily::Ui, 12.0, 500);
    (
        StatusItem::Workspace,
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
            (text(label, ColorRole::Text2, "Build"), Content),
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
    app.init_resource::<CursorReadout>().add_systems(
        Update,
        (
            // The cursor's ground point first, so its text is this frame's.
            (readouts::cursor_readout, readouts::status_texts).chain(),
            readouts::connection_dot,
            narrow_rules,
        )
            .in_set(ShellSystems::Readouts),
    );
}

#[cfg(test)]
mod tests;
