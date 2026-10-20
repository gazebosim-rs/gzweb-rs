//! The Assets panel (spec §3.3; M7): shapes, lights and Fuel models to
//! insert. Until then its body says what it will do.

mod view;

use bevy::prelude::*;

use super::{PanelDef, PanelId, RegisterPanel, WORLD_GROUP};
use crate::assets::icons::IconId;

/// The Assets panel's id.
pub const ASSETS: PanelId = PanelId::new("assets");

/// Registers the Assets panel.
pub struct AssetsPanelPlugin;

impl Plugin for AssetsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(ASSETS, "Assets", IconId::ActivityAssets, WORLD_GROUP, 10)
                .binding("panel.assets")
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
