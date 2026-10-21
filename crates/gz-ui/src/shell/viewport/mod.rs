//! The viewport area (spec §3.4, §9.3, §7.10): the 3D view's node, the
//! camera viewport synced to it, and the no-world card, the disconnected
//! banner and the dimming over the last frame.
//!
//! On the view's node (UI children of it, as §9.3 says) are its controls
//! (milestone M3): the [view cube](view_cube) at the top right, the
//! [rail](rail) below it, and the [HUD pills](hud) (the scale label and
//! the target chip). They show while a world is loaded, with the view.

pub(crate) mod banner;
pub(crate) mod connect_card;
pub(crate) mod hud;
pub(crate) mod rail;
pub(crate) mod view;
pub(crate) mod view_cube;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use gz_bevy::camera::MainCamera;
use gz_bevy::view::controls::ViewPlacementSystems;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{ViewCamera, ViewPlacement};

use super::regions::{pass_through, Region};
use super::ShellSystems;
use crate::theme::{ColorRole, Fill, Opacity, Theme};
use crate::widgets::layout::{overlay, row};
use crate::GzUiSystems;
use view::{DimOverlay, ViewNode, DIM_OPACITY};

/// The view the viewport's controls drive: the main camera's view (one
/// until split views, M6).
#[derive(SystemParam)]
pub(crate) struct MainView<'w, 's> {
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static ViewCamera,
            &'static Transform,
            &'static Projection,
        ),
        With<MainCamera>,
    >,
}

impl MainView<'_, '_> {
    /// The view's camera entity, state, placement and projection.
    pub(crate) fn get(&self) -> Option<(Entity, &ViewCamera, &Transform, &Projection)> {
        self.views.iter().next()
    }
}

/// The viewport area.
pub(crate) fn viewport_area(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
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
            a.spawn((ViewNode, overlay(), pass_through()))
                .with_children(|v| {
                    v.spawn(view_cube::view_cube(&t));
                    v.spawn(rail::rail(&t, &map));
                    v.spawn(hud::scale_label(&t));
                });
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
    rail::build(app);
    view_cube::build(app);
    hud::build(app);
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
