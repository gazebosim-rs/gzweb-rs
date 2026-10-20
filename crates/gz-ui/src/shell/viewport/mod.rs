//! The viewport area (spec §3.4, §9.3, §7.10): the 3D view's node, the
//! camera viewport synced to it, and the no-world card, the disconnected
//! banner and the dimming over the last frame.

pub(crate) mod banner;
pub(crate) mod connect_card;
pub(crate) mod view;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use gz_bevy::view::controls::ViewPlacementSystems;
use gz_bevy::view::ViewPlacement;

use super::regions::{pass_through, Region};
use super::ShellSystems;
use crate::theme::{ColorRole, Fill, Opacity, Theme};
use crate::widgets::layout::{overlay, row};
use crate::GzUiSystems;
use view::{DimOverlay, ViewNode, DIM_OPACITY};

/// The viewport area.
pub(crate) fn viewport_area(theme: &Theme) -> impl Bundle {
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

pub(crate) fn build(app: &mut App) {
    app.init_resource::<ViewPlacement>()
        .add_systems(
            Update,
            (connect_card::prefill_address, view::show_view_image).in_set(GzUiSystems::Behaviour),
        )
        .add_systems(
            Update,
            (
                banner::banner_state,
                view::dim_state,
                connect_card::card_state,
            )
                .in_set(ShellSystems::Readouts),
        )
        .add_systems(Update, connect_card::fade_card.after(GzUiSystems::Visuals))
        .add_systems(
            PostUpdate,
            view::place_view
                .after(UiSystems::Layout)
                .before(ViewPlacementSystems)
                .before(VisibilitySystems::UpdateFrusta),
        )
        .add_observer(connect_card::on_connect_button)
        .add_observer(connect_card::on_address_submit);
}

#[cfg(test)]
mod tests;
