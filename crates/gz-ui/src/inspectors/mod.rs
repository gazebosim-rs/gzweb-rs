//! The inspector's bodies (spec §3.5), one folder and one plugin per kind
//! of selection, the same way as the [panels](crate::panels): each
//! registers an [`InspectorDef`] in the [`InspectorRegistry`], and the
//! inspector frame (`shell::inspector`: header, pin, collapse, splitter)
//! hosts the body for what is selected. With nothing selected that is the
//! World inspector, the only one until M5 adds Model, Link, Joint, Light,
//! Sensor and multi-select (DECISIONS.md D-51).

mod world;

pub use world::WorldInspectorPlugin;

use bevy::prelude::*;

use crate::theme::Theme;

/// What the inspector is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InspectorKind {
    /// Nothing selected: the world (§3.5).
    World,
}

/// Spawns an inspector's body under the frame's header.
pub type BodyFn = fn(&mut ChildSpawner, &Theme);

/// What an inspector registers.
#[derive(Clone, Debug)]
pub struct InspectorDef {
    pub kind: InspectorKind,
    pub body: BodyFn,
}

/// The registered inspectors.
#[derive(Resource, Clone, Debug, Default)]
pub struct InspectorRegistry {
    inspectors: Vec<InspectorDef>,
}

impl InspectorRegistry {
    /// Adds (or replaces) the inspector for its kind.
    pub fn register(&mut self, def: InspectorDef) {
        self.inspectors.retain(|i| i.kind != def.kind);
        self.inspectors.push(def);
    }

    pub fn get(&self, kind: InspectorKind) -> Option<&InspectorDef> {
        self.inspectors.iter().find(|i| i.kind == kind)
    }

    /// The inspector for the current selection: always the world's until
    /// there is a selection (M5).
    pub fn for_selection(&self) -> Option<&InspectorDef> {
        self.get(InspectorKind::World)
    }
}

/// Registering an inspector from its plugin.
pub trait RegisterInspector {
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self;
}

impl RegisterInspector for App {
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<InspectorRegistry>()
            .register(def);
        self
    }
}

/// Adds every inspector (the shell adds this).
pub struct InspectorsPlugin;

impl Plugin for InspectorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorRegistry>()
            .add_plugins(WorldInspectorPlugin);
    }
}

#[cfg(test)]
mod tests;
