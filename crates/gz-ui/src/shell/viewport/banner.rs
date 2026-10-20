//! The disconnected banner (spec §7.10): across the top of the viewport
//! area while a loaded world's connection is lost, `warn_soft` and then
//! `danger_soft` after a minute, with Retry now and, once it's red,
//! "Connect to another server…".

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;

use super::connect_card::ConnectButton;
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::{ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{flex_spacer, row};
use crate::widgets::{button, ButtonVariant};

/// The banner's height (§7.10).
pub const BANNER_H: f32 = 36.0;

/// The disconnected banner and its parts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DisconnectBanner;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BannerText;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BannerIcon;

pub(super) fn disconnect_banner(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        DisconnectBanner,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            height: px(BANNER_H),
            padding: UiRect::horizontal(px(SPACE[6])),
            display: Display::None,
            ..row(SPACE[4])
        },
        Fill(ColorRole::WarnSoft),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((icon(IconId::Warning, 15.0, ColorRole::WarnText), BannerIcon));
            b.spawn((
                text(
                    TypeStyle::new(crate::theme::FontFamily::Ui, 12.0, 500),
                    ColorRole::WarnText,
                    "",
                ),
                BannerText,
                TextLayout::no_wrap(),
            ));
            b.spawn(flex_spacer());
            b.spawn((
                button(&t, "Connect to another server…", ButtonVariant::Normal),
                ConnectButton::Elsewhere,
            ));
            b.spawn((
                button(&t, "Retry now", ButtonVariant::Normal),
                ConnectButton::Retry,
            ));
        })),
    )
}
