//! What the panel and inspector registries share (DECISIONS.md D-57,
//! D-58): an open id type each, and a [`Registry`] of definitions that
//! plugins add to while the app is built and that closes when the shell
//! spawns from it.

use std::fmt;

use bevy::prelude::*;

/// Declares an open id: a short, stable name (`"scene"`) that any crate
/// can declare as a constant, like Rerun's `ViewClassIdentifier`.
macro_rules! ui_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, bevy::reflect::Reflect,
        )]
        pub struct $name(&'static str);

        impl $name {
            pub const fn new(name: &'static str) -> Self {
                Self(name)
            }

            pub const fn name(self) -> &'static str {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
    };
}
pub(crate) use ui_id;

/// A definition a [`Registry`] holds: a [`PanelDef`](crate::panels::PanelDef)
/// or an [`InspectorDef`](crate::inspectors::InspectorDef).
pub trait Registered: Clone + Send + Sync + 'static {
    type Id: Copy + Eq + fmt::Display + Send + Sync + 'static;
    /// What it is, for messages: "panel".
    const KIND: &'static str;

    fn id(&self) -> Self::Id;

    /// Where it goes: the registry keeps its definitions sorted by this,
    /// equal keys in registration order.
    fn sort_key(&self) -> (u32, u32) {
        (0, 0)
    }
}

/// The registered definitions of one kind, sorted by
/// [`Registered::sort_key`]. Plugins add to it while the app is built
/// (`App::register_panel`, `App::register_inspector`); once the shell has
/// spawned from it, it is closed.
#[derive(Resource, Clone, Debug)]
pub struct Registry<D: Registered> {
    defs: Vec<D>,
    /// The shell has spawned from it.
    closed: bool,
}

impl<D: Registered> Default for Registry<D> {
    fn default() -> Self {
        Self {
            defs: Vec::new(),
            closed: false,
        }
    }
}

impl<D: Registered> Registry<D> {
    /// Every definition, in order.
    pub fn iter(&self) -> impl Iterator<Item = &D> {
        self.defs.iter()
    }

    pub fn get(&self, id: D::Id) -> Option<&D> {
        self.defs.iter().find(|d| d.id() == id)
    }

    /// The first definition, if any.
    pub fn first(&self) -> Option<&D> {
        self.defs.first()
    }

    /// Adds `def` in its place. Panics once the registry is closed, and on
    /// an id already registered: replacing a built-in means disabling its
    /// plugin, since the earlier plugin's systems would otherwise keep
    /// running and writing the same id's state.
    pub(crate) fn register(&mut self, def: D) {
        let (kind, id) = (D::KIND, def.id());
        assert!(
            !self.closed,
            "register_{kind}('{id}'): {kind}s register while the app is built (in a plugin's \
             build); the shell has already spawned"
        );
        assert!(
            self.get(id).is_none(),
            "{kind} '{id}' is registered twice; to replace a built-in {kind}, disable its plugin \
             (ShellPlugins.build().disable::<…>()) and register yours under its id"
        );
        let key = def.sort_key();
        let at = self.defs.partition_point(|d| d.sort_key() <= key);
        self.defs.insert(at, def);
    }

    /// No more definitions: the shell has spawned from them.
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }
}

/// Adds `def` to its [`Registry`], creating the registry if needed.
pub(crate) fn register<D: Registered>(app: &mut App, def: D) {
    app.world_mut()
        .get_resource_or_init::<Registry<D>>()
        .register(def);
}
