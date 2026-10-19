//! The status bar (spec §3.6): connection, physics, frame rate,
//! iterations, the message chip slot, then the tool hint, selection,
//! cursor position and the workspace button. Below 1280 px the hint and
//! cursor drop out, below 1100 px the frame rate and iterations too.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;
use bevy::window::PrimaryWindow;
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::{cursor_on_ground, ViewOrigin};

use super::connection::{dot_role, CurrentPhase};
use super::layout::{INSPECTOR_START_W, SIDEBAR_START_W};
use super::{format, region, Region, ShellLayout, Tool};
use crate::fonts::text;
use crate::icons::{content_icon, IconId};
use crate::theme::{border, ColorRole, Fill, FontFamily, Theme, TypeStyle, SPACE};
use crate::visuals::{Content, Visuals};
use crate::widgets::button::ICON_BUTTON_RADIUS;
use crate::widgets::chip::dot;
use crate::widgets::{clickable, rounded, spacer, Tooltip, TooltipSide};

/// The status bar's items, numbered as in §3.6.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusItem {
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
    pub fn shown_at(self, width: f32) -> bool {
        match self {
            StatusItem::Hint | StatusItem::Cursor => width >= INSPECTOR_START_W,
            StatusItem::Fps | StatusItem::Iterations => width >= SIDEBAR_START_W,
            _ => true,
        }
    }
}

/// The text of an item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusText(pub StatusItem);

/// The connection dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ConnectionDot;

/// The ground point under the cursor, Gazebo x, y (§3.6 item 9).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorReadout(pub Option<[f32; 2]>);

/// Mono 12 `text_2`, the bar's type (§3.6).
pub const BAR_TYPE: TypeStyle = TypeStyle::MONO_SMALL;
/// The hint's type: Geist 12 `text_3`.
pub const HINT_TYPE: TypeStyle = TypeStyle::SMALL;

fn item(which: StatusItem, style: TypeStyle, role: ColorRole, value: &str) -> impl Bundle {
    (
        which,
        Node {
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(SPACE[3]),
            ..default()
        },
        children![(
            text(style, role, value.to_string()),
            StatusText(which),
            TextLayout::no_wrap()
        )],
    )
}

/// The status bar.
pub fn status_bar(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        region(
            Region::StatusBar,
            Node {
                height: px(theme.s.status_bar_h),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(px(SPACE[6])),
                column_gap: px(SPACE[8]),
                align_items: AlignItems::Center,
                border: UiRect::top(px(border::HAIRLINE)),
                overflow: Overflow::clip(),
                ..default()
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((
                StatusItem::Connection,
                Node {
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: px(SPACE[3]),
                    ..default()
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
            b.spawn(spacer());
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
            b.spawn(workspace_button(&t));
        })),
    )
}

/// The workspace button: icon, name and chevron_up, 22 tall (§3.6 item
/// 10). It opens the switcher in M8.
fn workspace_button(theme: &Theme) -> impl Bundle {
    let _ = theme;
    let label = TypeStyle::new(FontFamily::Ui, 12.0, 500);
    (
        StatusItem::Workspace,
        Node {
            height: px(22),
            padding: UiRect::horizontal(px(SPACE[3])),
            column_gap: px(SPACE[2] + 1.0),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            border_radius: rounded(ICON_BUTTON_RADIUS - 2.0),
            ..default()
        },
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text2),
            accesskit::Role::Button,
            "Workspace",
        ),
        Tooltip::new("Workspace: Build")
            .detail("Click to switch")
            .shortcut("Alt+1–4")
            .side(TooltipSide::Above),
        children![
            content_icon(IconId::ActivityWorkspaces, 12.0, ColorRole::Text2),
            (text(label, ColorRole::Text2, "Build"), Content),
            content_icon(IconId::ChevronUp, 10.0, ColorRole::Text2),
        ],
    )
}

fn set(text: &mut Text, want: &str) {
    if text.0 != want {
        text.0 = want.to_string();
    }
}

/// Every item's text and the connection dot.
#[allow(clippy::too_many_arguments)]
pub fn status_texts(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    tool: Res<Tool>,
    cursor: Res<CursorReadout>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    mut texts: Query<(&StatusText, &mut Text)>,
    mut dots: Query<&mut Fill, With<ConnectionDot>>,
) {
    let status = status.as_deref();
    let sim = status.and_then(|s| s.sim);
    let fps = diagnostics
        .as_ref()
        .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS))
        .and_then(|d| d.smoothed());
    for (which, mut t) in &mut texts {
        let want = match which.0 {
            StatusItem::Connection => status.map_or_else(String::new, |s| format::address(&s.url)),
            StatusItem::Physics => format::physics(None, sim.map(|s| s.step_size)),
            StatusItem::Fps => format::fps(fps),
            StatusItem::Iterations => format::iterations(sim.as_ref()),
            StatusItem::Hint => tool.hint().into(),
            StatusItem::Selection => "sel: —".into(),
            StatusItem::Cursor => format::cursor(cursor.0),
            StatusItem::Message | StatusItem::Workspace => continue,
        };
        set(&mut t, &want);
    }
    let role = dot_role(phase.0, status);
    for mut fill in &mut dots {
        fill.set_if_neq(Fill(role));
    }
}

/// Drops items on narrow windows (§3.6).
pub fn narrow_rules(layout: Res<ShellLayout>, mut items: Query<(&StatusItem, &mut Node)>) {
    if !layout.is_changed() {
        return;
    }
    for (which, mut node) in &mut items {
        let display = if which.shown_at(layout.window_w) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// The ground point under the cursor while it is over the view.
pub fn cursor_readout(
    windows: Query<&Window, With<PrimaryWindow>>,
    routing: Res<ViewRouting>,
    cameras: Query<(&Camera, Option<&ViewOrigin>, &Projection, &Transform), With<MainCamera>>,
    mut readout: ResMut<CursorReadout>,
) {
    let at = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .filter(|_| routing.pointer)
        .and_then(|c| {
            let (camera, origin, projection, t) = cameras.single().ok()?;
            cursor_on_ground(camera, origin, projection, t, c)
        })
        .map(|v| [v.x, v.y]);
    readout.set_if_neq(CursorReadout(at));
}
