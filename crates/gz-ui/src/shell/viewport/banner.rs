//! The disconnected banner (spec §7.10): across the top of the viewport
//! area while a loaded world's connection is lost, `warn_soft` and then
//! `danger_soft` after a minute, with Retry now and, once it's red,
//! "Connect to another server…".

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;

use super::connect_card::ConnectButton;
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::shell::connection::{banner, CurrentPhase};
use crate::theme::{ColorRole, Fill, Ink, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{flex_spacer, row, set_shown};
use crate::widgets::text::set_text;
use crate::widgets::{button, ButtonVariant};

/// The banner's height (§7.10).
pub(crate) const BANNER_H: f32 = 36.0;

/// The disconnected banner and its parts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DisconnectBanner;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BannerText;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BannerIcon;

/// The banner shows while lost (§7.10): its text, `warn_soft` then
/// `danger_soft` after a minute, and then "Connect to another server…".
pub(crate) fn banner_state(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    time: Res<Time<Real>>,
    mut banners: Query<(&mut Node, &mut Fill), With<DisconnectBanner>>,
    mut texts: Query<(&mut Text, &mut Ink), With<BannerText>>,
    mut icons: Query<&mut Ink, (With<BannerIcon>, Without<BannerText>)>,
    mut buttons: Query<(&ConnectButton, &mut Node), Without<DisconnectBanner>>,
) {
    let b = banner(phase.0, status.as_deref(), time.elapsed_secs_f64());
    for (mut node, mut fill) in &mut banners {
        set_shown(&mut node, b.is_some());
        if let Some(b) = &b {
            fill.set_if_neq(Fill(b.roles().0));
        }
    }
    if let Some(b) = &b {
        let ink = b.roles().1;
        for (mut t, mut i) in &mut texts {
            set_text(&mut t, &b.text);
            i.set_if_neq(Ink(ink));
        }
        for mut i in &mut icons {
            i.set_if_neq(Ink(ink));
        }
    }
    let long = b.as_ref().is_some_and(|b| b.long);
    for (which, mut node) in &mut buttons {
        if *which == ConnectButton::Elsewhere {
            set_shown(&mut node, long);
        }
    }
}

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
