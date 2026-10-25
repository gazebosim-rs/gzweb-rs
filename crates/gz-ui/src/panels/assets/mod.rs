//! The Assets panel (spec §3.3, §3.3.1; DECISIONS.md D-102): shapes,
//! lights, Fuel models and local models to insert into the world, and,
//! while a model is edited, the parts to add to it (the Parts tab, §6.6,
//! D-113: `parts`).
//!
//! A tile, a Fuel result's Insert or the Local tab's Insert arms it:
//! clicks in the views then place it on the surface under the pointer
//! (`gz_bevy::view::place`), with a preview, Shift to place several and
//! Esc to stop. Each place is a `create` request to the server (it needs
//! one that takes edits, D-76), and its row says how it goes until it
//! shows in the scene: a Fuel model downloads on the server first.
//!
//! The panel contract (DECISIONS.md D-58): what the user set is
//! [`model::AssetsState`], the inserts on their way [`model::Inserts`];
//! the body is built from them ([`view::build_assets`]) and can be
//! respawned at any time.

mod model;
mod parts;

pub(crate) use model::AssetsState;
pub(crate) use parts::{PartAction, REPARENT_HINT};
mod systems;
mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, PanelView, RegisterPanel, WORLD_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems};

/// The Assets panel's id.
pub const ASSETS: PanelId = PanelId::new("assets");

/// How the panel starts, for a capture (`gzweb --assets TAB[:QUERY]`):
/// it opens in the primary slot on `tab`, and on the Fuel tab searches
/// `query`; then the resource is removed.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct AssetsStart {
    tab: model::AssetTab,
    query: String,
}

impl std::str::FromStr for AssetsStart {
    type Err = String;

    /// `shapes`, `lights`, `local`, `parts` (showing while a model is
    /// edited), `fuel` or `fuel:QUERY`.
    fn from_str(s: &str) -> Result<Self, String> {
        let (tab, query) = s.split_once(':').unwrap_or((s, ""));
        let tab = model::AssetTab::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(tab))
            .ok_or_else(|| format!("not a tab (shapes, lights, fuel, local, parts): {tab}"))?;
        if !query.is_empty() && tab != model::AssetTab::Fuel {
            return Err("only the Fuel tab takes a query".into());
        }
        Ok(AssetsStart {
            tab,
            query: query.to_string(),
        })
    }
}

/// The Assets panel's content marker: its body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct AssetsBody;

/// Registers the Assets panel.
pub struct AssetsPanelPlugin;

impl Plugin for AssetsPanelPlugin {
    fn build(&self, app: &mut App) {
        gz_bevy::fuel::FuelPlugin::ensure(app);
        app.register_panel(
            PanelDef::new(ASSETS, "Assets", IconId::ActivityAssets)
                .place(WORLD_GROUP, 10)
                .binding("panel.assets")
                .view(PanelView::Ready)
                .content::<AssetsBody>(),
        )
        .init_resource::<AssetsState>()
        .init_resource::<model::Inserts>()
        .init_resource::<systems::BuiltRows>()
        .init_resource::<gz_bevy::view::place::Placing>()
        .add_message::<gz_bevy::view::place::PlaceAt>()
        .init_resource::<gz_bevy::view::place::PlaceHover>()
        .add_systems(
            Update,
            (
                (
                    systems::apply_start,
                    systems::arm,
                    systems::place,
                    systems::replies,
                    systems::settle,
                    systems::search_typed,
                    systems::ghost,
                    parts::parts_tab,
                    // After the draft's parts are back in the extras (a
                    // reply drops them for a moment): a draft joint to
                    // reparent is still there.
                    parts::forget_parent.after(gz_bevy::view::editing::parts::patch_extras),
                    parts::place_part,
                    parts::part_action_states,
                    parts::part_ghost,
                    parts::joint_band,
                    parts::parts_discarded,
                )
                    .chain(),
                (
                    systems::query_text,
                    systems::tab_states,
                    systems::tile_states,
                    systems::note,
                    systems::fuel_states,
                    systems::sync_fuel_rows,
                    systems::row_bars,
                    systems::sync_inserts,
                    systems::move_bars,
                    parts::tab_systems(),
                )
                    .chain()
                    .after(systems::ghost)
                    .run_if(panel_shown(ASSETS)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_assets)
        .add_observer(systems::on_tab)
        .add_observer(systems::on_tile)
        .add_observer(systems::on_fuel_insert)
        .add_observer(systems::label_insert)
        .add_observer(systems::on_fuel_button)
        .add_observer(systems::on_local_insert)
        .add_observer(systems::on_dismiss)
        .add_observer(systems::on_submit);
        parts::observers(app);
    }
}

#[cfg(test)]
mod tests;
