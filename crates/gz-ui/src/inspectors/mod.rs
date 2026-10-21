//! The inspector's bodies (spec §3.5), one folder and one plugin per kind
//! of selection, the same way as the [panels](crate::panels): each
//! registers an [`InspectorDef`] under its [`InspectorId`] (an open id),
//! and the inspector frame (`shell::inspector`: header, pin, collapse,
//! splitter) hosts the body for what is selected. With nothing selected
//! that is the World inspector, the only one until M5 adds Model, Link,
//! Joint, Light, Sensor and multi-select (DECISIONS.md D-51, D-57).
//!
//! The built-in inspectors come as [`InspectorPlugins`], part of
//! [`ShellPlugins`](crate::shell::ShellPlugins); an app replaces one by
//! disabling its plugin and registering its own under the same id (an id
//! registered twice panics). Inspectors register while the app is built:
//! the shell closes the registry when it spawns.

mod world;

pub use world::{WorldInspectorPlugin, WORLD};

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

use crate::registry::{self, ui_id, Registered, Registry};
use crate::theme::Theme;

ui_id! {
    /// An inspector's id: a short, stable name ("world"), like
    /// [`PanelId`](crate::panels::PanelId).
    InspectorId
}

/// Spawns an inspector's body under the frame's header.
pub type BodyFn = fn(&mut ChildSpawner, &Theme);

/// What an inspector registers.
#[derive(Clone, Debug)]
pub struct InspectorDef {
    pub id: InspectorId,
    pub body: BodyFn,
}

impl Registered for InspectorDef {
    type Id = InspectorId;
    const KIND: &'static str = "inspector";

    fn id(&self) -> InspectorId {
        self.id
    }
}

/// The registered inspectors, in registration order. Only
/// [`RegisterInspector::register_inspector`] adds to it, while the app is
/// built.
pub type InspectorRegistry = Registry<InspectorDef>;

impl Registry<InspectorDef> {
    /// The inspector for the current selection: always the world's until
    /// there is a selection (M5).
    pub fn for_selection(&self) -> Option<&InspectorDef> {
        self.get(WORLD)
    }
}

/// Registering an inspector from its plugin.
pub trait RegisterInspector {
    /// Adds `def` to the [`InspectorRegistry`]. Panics on an id already
    /// registered, and once the shell has spawned (the inspector frame is
    /// built from the registry).
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self;
}

impl RegisterInspector for App {
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self {
        registry::register(self, def);
        self
    }
}

/// The built-in inspectors. Part of
/// [`ShellPlugins`](crate::shell::ShellPlugins).
pub struct InspectorPlugins;

impl PluginGroup for InspectorPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>().add(WorldInspectorPlugin)
    }
}

#[cfg(test)]
mod tests;
