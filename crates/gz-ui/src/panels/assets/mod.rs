//! The Assets panel (spec §3.3; M7): shapes, lights and Fuel models to
//! insert. Until then its body says what it will do.

mod view;

use bevy::prelude::*;

use super::{Panel, PanelDef, RegisterPanel};
use crate::assets::icons::IconId;

/// Registers the Assets panel.
pub struct AssetsPanelPlugin;

impl Plugin for AssetsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(Panel::Assets, "Assets", IconId::ActivityAssets, 10)
                .binding("panel.assets")
                .separator_after()
                .view(view::state),
        );
    }
}

#[cfg(test)]
mod tests;
