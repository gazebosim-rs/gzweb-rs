//! The world menu button (spec §3.1, §7.1): the world's name in `text`
//! 500, `.sdf` in `text_3` 400, the unsaved dot when dirty (never yet) and
//! a chevron. It opens the World menu in M8.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;
use gz_bevy::connection::GzStatus;
use gz_bevy::spawn::GzIndex;

use super::TopText;
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::Visuals;
use crate::theme::{ColorRole, FontFamily, Theme, TypeStyle, SPACE};
use crate::widgets::button::ICON_BUTTON_RADIUS;
use crate::widgets::{clickable, rounded, Tooltip};

/// The world menu button (opens the World menu in M8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WorldButton;

/// The world menu button: name in `text` 500, `.sdf` in `text_3` 400, the
/// unsaved dot when dirty (never yet), chevron.
pub(super) fn world_button(theme: &Theme) -> impl Bundle {
    let name = TypeStyle::new(FontFamily::Ui, 15.0, 500);
    let ext = TypeStyle::new(FontFamily::Ui, 15.0, 400);
    let icon_px = theme.s.icon_sm;
    (
        WorldButton,
        Node {
            height: px(theme.s.text_btn_h),
            padding: UiRect::horizontal(px(9)),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            border_radius: rounded(ICON_BUTTON_RADIUS),
            ..default()
        },
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text),
            Role::Button,
            "World menu",
        ),
        Tooltip::new("World").detail("World menu"),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(Node {
                column_gap: px(SPACE[2] + 1.0),
                align_items: AlignItems::Baseline,
                ..default()
            })
            .with_children(|n| {
                n.spawn((text(name, ColorRole::Text, "No world"), TopText::WorldName));
                n.spawn((text(ext, ColorRole::Text3, ""), TopText::WorldExt));
            });
            p.spawn(content_icon(IconId::ChevronDown, icon_px, ColorRole::Text));
        })),
    )
}

/// The world's name: the server's world, else the scene's.
pub fn world_name(status: Option<&GzStatus>, index: Option<&GzIndex>) -> Option<String> {
    status
        .and_then(|s| s.world.clone())
        .or_else(|| index.and_then(|i| i.scene.as_ref().map(|s| s.name.clone())))
        .filter(|n| !n.is_empty())
}
