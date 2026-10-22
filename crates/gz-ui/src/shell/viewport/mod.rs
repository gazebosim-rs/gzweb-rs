//! The viewport area (spec §3.4, §9.3, §7.10): the 3D views' nodes in
//! their [layout](views), each camera synced to its node, and the no-world
//! card, the disconnected banner and the dimming over the last frame.
//!
//! On each view's node (UI children of it, as §9.3 says) are its controls
//! (milestone M3): the [view cube](view_cube) at the top right, the
//! [rail](rail) below it, and the [HUD pills](hud) (the scale label and
//! the target chip). They show while a world is loaded, with the view.

pub(crate) mod banner;
pub(crate) mod connect_card;
pub(crate) mod gizmo;
pub(crate) mod hud;
pub(crate) mod marquee;
pub(crate) mod outline;
pub(crate) mod rail;
pub(crate) mod view;
pub(crate) mod view_cube;
pub(crate) mod views;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use gz_bevy::view::controls::ViewPlacementSystems;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::ViewId;

use super::regions::{pass_through, Region};
use super::ShellSystems;
use crate::theme::{ColorRole, Fill, Opacity, Theme};
use crate::widgets::layout::{overlay, row};
use crate::GzUiSystems;
use view::{DimOverlay, ViewNode, DIM_OPACITY};
pub(crate) use views::ViewCameras;

/// View `id`'s node, growing by `grow` (its divider's share), with its
/// controls.
pub(crate) fn view_node(
    theme: &Theme,
    map: &InputMap,
    id: ViewId,
    share: Option<views::Share>,
    grow: f32,
) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        ViewNode(id),
        views::pane(share, grow, Node::default()),
        Children::spawn(SpawnWith(move |v: &mut ChildSpawner| {
            // First, under the view's controls.
            v.spawn(outline::outline());
            v.spawn(views::active_frame());
            v.spawn(view_cube::view_cube(&t));
            v.spawn(rail::rail(&t, &map));
            v.spawn(hud::scale_label(&t));
            v.spawn(marquee::marquee());
            v.spawn(gizmo::gizmo_readout());
        })),
    )
}

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
            a.spawn(views::view_grid());
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
    views::build(app);
    rail::build(app);
    view_cube::build(app);
    hud::build(app);
    outline::build(app);
    gizmo::build(app);
    app.add_systems(
        Update,
        (
            connect_card::prefill_address,
            view::show_view_image,
            marquee::marquee_box,
        )
            .in_set(GzUiSystems::Behaviour),
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
