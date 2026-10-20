//! The World inspector (spec §3.5): with nothing selected, the WORLD tag
//! and the world's name, the source line, the quick actions (Save, Reset,
//! Statistics), and the Physics, Scene and Plugins groups, as
//! placeholders until M5 fills them.

mod systems;
mod view;

use bevy::prelude::*;

use super::{InspectorDef, InspectorId, RegisterInspector};
use crate::shell::ShellSystems;

/// The World inspector's id.
pub const WORLD: InspectorId = InspectorId::new("world");

/// Registers the World inspector and its systems.
pub struct WorldInspectorPlugin;

impl Plugin for WorldInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef {
            id: WORLD,
            body: view::world_inspector,
        })
        .add_systems(
            Update,
            (systems::world_fields, systems::reset_availability).in_set(ShellSystems::Readouts),
        )
        .add_observer(systems::on_world_action);
    }
}

#[cfg(test)]
mod tests;
