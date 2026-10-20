//! The viewport area (spec §3.4, §9.3, §7.10): the 3D view's node, the
//! camera viewport synced to it, and the no-world card, the disconnected
//! banner and the dimming over the last frame.

pub mod banner;
pub mod connect_card;
pub mod view;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;

use super::connection::{banner, ConnectionView, CurrentPhase, Phase};
use super::regions::{pass_through, Region};
use crate::theme::{ColorRole, Fill, Ink, Opacity, Theme};
use crate::widgets::layout::{overlay, row, set_shown};
use crate::widgets::text::set_text;
use banner::{BannerIcon, BannerText, DisconnectBanner};
use connect_card::{card_status, fade_secs, CardStatus, ConnectButton, ConnectCard, Fading};
use view::{DimOverlay, ViewNode, DIM_OPACITY};

/// The viewport area.
pub fn viewport_area(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        Region::ViewportArea,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            justify_content: JustifyContent::Center,
            ..row(0.0)
        },
        // `bg` (§2.1) shows while no world is loaded; the view covers it.
        Fill(ColorRole::Bg),
        pass_through(),
        Children::spawn(SpawnWith(move |a: &mut ChildSpawner| {
            a.spawn((ViewNode, overlay(), pass_through()));
            a.spawn((
                DimOverlay,
                Node {
                    display: Display::None,
                    ..overlay()
                },
                Fill(ColorRole::Bg),
                Opacity(DIM_OPACITY),
                pass_through(),
            ));
            a.spawn(banner::disconnect_banner(&t));
            a.spawn(connect_card::connect_card(&t));
        })),
    )
}

/// The banner, the dimming and the card, from the connection phase.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn overlays(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    view: Res<ConnectionView>,
    time: Res<Time<Real>>,
    mut banners: Query<(&mut Node, &mut Fill), (With<DisconnectBanner>, Without<ConnectCard>)>,
    mut dims: Query<
        &mut Node,
        (
            With<DimOverlay>,
            Without<DisconnectBanner>,
            Without<ConnectCard>,
        ),
    >,
    mut banner_texts: Query<(&mut Text, &mut Ink), With<BannerText>>,
    mut banner_icons: Query<&mut Ink, (With<BannerIcon>, Without<BannerText>)>,
    mut elsewhere: Query<
        (&ConnectButton, &mut Node),
        (
            Without<DisconnectBanner>,
            Without<DimOverlay>,
            Without<ConnectCard>,
        ),
    >,
    mut cards: Query<(Entity, &mut Node, Option<&Fading>), With<ConnectCard>>,
    mut card_status_q: Query<&mut Text, (With<CardStatus>, Without<BannerText>)>,
    mut commands: Commands,
) {
    let status = status.as_deref();
    let now = time.elapsed_secs_f64();
    let b = banner(phase.0, status, now);
    for (mut node, mut fill) in &mut banners {
        set_shown(&mut node, b.is_some());
        if let Some(b) = &b {
            fill.set_if_neq(Fill(b.roles().0));
        }
    }
    if let Some(b) = &b {
        let ink = b.roles().1;
        for (mut t, mut i) in &mut banner_texts {
            set_text(&mut t, &b.text);
            i.set_if_neq(Ink(ink));
        }
        for mut i in &mut banner_icons {
            i.set_if_neq(Ink(ink));
        }
    }
    let long = b.as_ref().is_some_and(|b| b.long);
    for (which, mut node) in &mut elsewhere {
        if *which == ConnectButton::Elsewhere {
            set_shown(&mut node, long);
        }
    }
    let lost = matches!(phase.0, Phase::Lost { .. });
    for mut node in &mut dims {
        set_shown(&mut node, lost);
    }
    let card = phase.0.no_world() || (lost && view.card_requested);
    for (e, mut node, fading) in &mut cards {
        let shown = node.display != Display::None;
        if card {
            if fading.is_some() {
                commands.entity(e).remove::<Fading>();
            }
            set_shown(&mut node, true);
        } else if shown && fading.is_none() {
            commands.entity(e).insert(Fading(fade_secs()));
        }
    }
    let line = card_status(phase.0, status);
    for mut t in &mut card_status_q {
        set_text(&mut t, &line);
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (connect_card::prefill_address, view::show_view_image)
            .in_set(crate::GzUiSystems::Behaviour),
    )
    .add_observer(connect_card::on_connect_button)
    .add_observer(connect_card::on_address_submit);
}
