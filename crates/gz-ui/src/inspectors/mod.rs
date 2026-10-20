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
//! disabling its plugin and registering its own under the same id.
//! Inspectors register while the app is built: the shell closes the
//! registry when it spawns.

mod world;

pub use world::{WorldInspectorPlugin, WORLD};

use std::fmt;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

use crate::theme::Theme;

/// An inspector's id: a short, stable name ("world"), like
/// [`PanelId`](crate::panels::PanelId).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct InspectorId(&'static str);

impl InspectorId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn name(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for InspectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Spawns an inspector's body under the frame's header.
pub type BodyFn = fn(&mut ChildSpawner, &Theme);

/// What an inspector registers.
#[derive(Clone, Debug)]
pub struct InspectorDef {
    pub id: InspectorId,
    pub body: BodyFn,
}

/// The registered inspectors. Only
/// [`RegisterInspector::register_inspector`] adds to it, while the app is
/// built.
#[derive(Resource, Clone, Debug, Default)]
pub struct InspectorRegistry {
    inspectors: Vec<InspectorDef>,
    /// The shell has spawned from it.
    closed: bool,
}

impl InspectorRegistry {
    /// Adds an inspector; an id registered again replaces the earlier one,
    /// with a warning.
    pub(crate) fn register(&mut self, def: InspectorDef) {
        if self.get(def.id).is_some() {
            warn!(
                "inspector '{}' is registered twice; the later registration replaces the \
                 earlier (to replace a built-in inspector, disable its plugin in ShellPlugins)",
                def.id
            );
            self.inspectors.retain(|i| i.id != def.id);
        }
        self.inspectors.push(def);
    }

    /// No more inspectors: the shell has spawned.
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }

    pub fn get(&self, id: InspectorId) -> Option<&InspectorDef> {
        self.inspectors.iter().find(|i| i.id == id)
    }

    /// The inspector for the current selection: always the world's until
    /// there is a selection (M5).
    pub fn for_selection(&self) -> Option<&InspectorDef> {
        self.get(WORLD)
    }
}

/// Registering an inspector from its plugin.
pub trait RegisterInspector {
    /// Adds `def` to the [`InspectorRegistry`]. Panics once the shell has
    /// spawned (the inspector frame is built from the registry).
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self;
}

impl RegisterInspector for App {
    fn register_inspector(&mut self, def: InspectorDef) -> &mut Self {
        let mut registry = self.world_mut().get_resource_or_init::<InspectorRegistry>();
        assert!(
            !registry.closed,
            "register_inspector('{}'): inspectors register while the app is built (in a \
             plugin's build); the shell has already spawned",
            def.id
        );
        registry.register(def);
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
