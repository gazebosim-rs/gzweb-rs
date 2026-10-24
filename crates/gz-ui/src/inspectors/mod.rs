//! The inspector's bodies (spec §3.5), one folder and one plugin per kind
//! of selection, the same way as the [panels](crate::panels): each
//! registers an [`InspectorDef`] under its [`InspectorId`] (an open id)
//! for an [`InspectKind`], and the inspector frame (`shell::inspector`:
//! header, pin, collapse, splitter) hosts the body for what is
//! [`Inspected`]. With nothing selected that is the World inspector
//! (DECISIONS.md D-51, D-57).
//!
//! A body is built like a panel's content (D-58): the frame inserts the
//! inspector's marker on a fresh body whenever the subject changes, and the
//! inspector's observer on `Add<Marker>` builds it from [`Inspected`] and
//! whatever else it reads; its systems keep the values current.
//!
//! The built-in inspectors come as [`InspectorPlugins`], part of
//! [`ShellPlugins`](crate::shell::ShellPlugins); an app replaces one by
//! disabling its plugin and registering its own under the same id (an id
//! registered twice panics). Inspectors register while the app is built:
//! the shell closes the registry when it spawns.

pub(crate) mod common;
mod joint;
mod light;
mod link;
mod model;
mod multi;
pub(crate) mod sensor;
mod world;

pub use joint::{JointInspectorPlugin, JOINT};
pub use light::{LightInspectorPlugin, LIGHT};
pub use link::{LinkInspectorPlugin, LINK};
pub use model::{ModelInspectorPlugin, MODEL};
pub use multi::{MultiInspectorPlugin, MULTI};
pub use sensor::{SensorInspectorPlugin, SENSOR};
pub use world::{WorldInspectorPlugin, WORLD};

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use gz_bevy::view::Part;

use crate::panels::ContentMarker;
use crate::registry::{self, ui_id, Registered, Registry};

ui_id! {
    /// An inspector's id: a short, stable name ("world"), like
    /// [`PanelId`](crate::panels::PanelId).
    InspectorId
}

/// The kinds of selection an inspector can be for (spec §3.5's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InspectKind {
    /// Nothing selected.
    #[default]
    World,
    Model,
    Link,
    Light,
    Joint,
    Sensor,
    /// More than one thing selected: the common fields.
    Multi,
}

/// What the inspector shows: the kind, and the scene entity (none for the
/// world and a multi-selection). `removed` once that entity is gone and
/// nothing replaced it (§2 states: "(removed)", until dismissed).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Inspected {
    pub kind: InspectKind,
    pub entity: Option<Entity>,
    /// The entity's scene path, to follow it when it respawns.
    pub path: Option<String>,
    pub removed: bool,
    /// The header's Pin: keep showing this when the selection changes.
    pub pinned: bool,
    /// A joint or sensor subject: which one (`entity` is its model or
    /// link).
    pub part: Option<Part>,
}

/// What an inspector registers: its id, the kind of selection it is for,
/// and its content marker ([`InspectorDef::content`]).
#[derive(Clone, Debug)]
pub struct InspectorDef {
    pub id: InspectorId,
    pub kind: InspectKind,
    pub content: Option<ContentMarker>,
}

impl InspectorDef {
    pub fn new(id: InspectorId, kind: InspectKind) -> Self {
        Self {
            id,
            kind,
            content: None,
        }
    }

    /// Its body: a fresh body gets `C::default()`, and the inspector's own
    /// observer on `Add<C>` builds it (as a panel's content, D-58).
    pub fn content<C: Component + Default>(mut self) -> Self {
        self.content = Some(|body| {
            body.insert(C::default());
        });
        self
    }
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
    /// The first inspector registered for `kind`.
    pub fn for_kind(&self, kind: InspectKind) -> Option<&InspectorDef> {
        self.iter().find(|i| i.kind == kind)
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
        PluginGroupBuilder::start::<Self>()
            .add(WorldInspectorPlugin)
            .add(ModelInspectorPlugin)
            .add(LinkInspectorPlugin)
            .add(LightInspectorPlugin)
            .add(JointInspectorPlugin)
            .add(SensorInspectorPlugin)
            .add(MultiInspectorPlugin)
    }
}

#[cfg(test)]
mod tests;
