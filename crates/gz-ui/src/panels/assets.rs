//! The Assets panel (spec §3.3; M7): shapes, lights and Fuel models to
//! insert. Until then its body says what it will do. (§3.3.1's "Fuel is
//! unreachable" + Retry needs the Fuel client, M7: its systems will write
//! that view.)

use bevy::prelude::*;

use super::{PanelDef, PanelId, PanelView, RegisterPanel, WORLD_GROUP};
use crate::assets::IconId;

/// The Assets panel's id.
pub const ASSETS: PanelId = PanelId::new("assets");

/// Registers the Assets panel.
pub struct AssetsPanelPlugin;

impl Plugin for AssetsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(ASSETS, "Assets", IconId::ActivityAssets)
                .place(WORLD_GROUP, 10)
                .binding("panel.assets")
                .view(PanelView::empty("Search Fuel for models and worlds")),
        );
    }
}
