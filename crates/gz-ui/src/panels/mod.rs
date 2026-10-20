//! The sidebar's panels (spec §3.3; contents in M4 and M7), one folder
//! and one plugin each.
//!
//! A panel is a [`PanelDef`] registered by its plugin with
//! `app.register_panel(…)`, the way Rerun registers view classes and Zed
//! panels: the activity bar, the sidebar's slots and the panel shortcuts
//! host whatever the [`PanelRegistry`] holds, and know nothing about a
//! particular panel. A panel is known by its [`PanelId`], an open id: a
//! panel in another crate brings its own, and
//! [`ShellLayout`](crate::shell::ShellLayout) keeps ids in its slots.
//! What is a panel's own lives in its folder: its title, icon, binding and
//! place in the activity bar, its body in each connection phase, and its
//! systems, which write its activity badge and header count into the
//! shell's [`PanelStatus`](crate::shell::PanelStatus).
//!
//! The built-in panels come as [`PanelPlugins`]; an app replaces one by
//! disabling its plugin and registering its own panel under the same id
//! (registering an id twice warns, and the later registration wins). Panels
//! register while the app is built: once the shell has spawned, the
//! registry is closed (DECISIONS.md D-57).
//!
//! A panel can move to its own crate when it grows heavy (or compile times
//! ask for it): it depends on this module and the widgets, and its plugin
//! is the whole of its interface (DECISIONS.md D-51).

mod assets;
mod console;
mod plot;
mod scene;
mod sensors;
mod teleop;
mod topics;

use std::fmt;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

use crate::assets::IconId;
use crate::shell::Phase;
use crate::theme::Theme;
pub use assets::{AssetsPanelPlugin, ASSETS};
pub use console::{ConsolePanelPlugin, ConsoleUnread, CONSOLE};
pub use plot::{PlotCharts, PlotPanelPlugin, PLOT};
pub use scene::{ScenePanelPlugin, SCENE};
pub use sensors::{SensorsPanelPlugin, SENSORS};
pub use teleop::{TeleopPanelPlugin, TELEOP};
pub use topics::{TopicsPanelPlugin, TOPICS};

/// A panel's id: a short, stable name ("scene"), unique among the
/// registered panels. Open, like Rerun's `ViewClassIdentifier`: a panel in
/// any crate declares its own as a constant. The layout keeps ids in its
/// slots, and a saved layout (M8) keys by [`name`](Self::name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct PanelId(&'static str);

impl PanelId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn name(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for PanelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What a panel's body shows (§3.3.1).
#[derive(Clone, Debug, PartialEq)]
pub enum PanelView {
    /// Content ([`PanelDef::content`]; empty until M4/M7).
    Ready,
    Empty {
        message: &'static str,
        actions: Vec<PanelAction>,
    },
    /// Skeleton rows.
    Loading,
    Error {
        message: &'static str,
        actions: Vec<PanelAction>,
    },
}

impl PanelView {
    /// The empty state with `message` and no buttons.
    pub fn empty(message: &'static str) -> Self {
        PanelView::Empty {
            message,
            actions: vec![],
        }
    }
}

/// A button in an empty or error state.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    /// Retry the server now.
    Reconnect,
    /// Not available until the World menu (M8): shown disabled.
    OpenWorld,
    NewWorld,
    AddField,
}

impl PanelAction {
    pub fn label(self) -> &'static str {
        match self {
            PanelAction::Reconnect => "Reconnect",
            PanelAction::OpenWorld => "Open world…",
            PanelAction::NewWorld => "New world",
            PanelAction::AddField => "Add field…",
        }
    }

    /// Whether it does something yet.
    pub fn available(self) -> bool {
        self == PanelAction::Reconnect
    }
}

/// A panel's body in a connection phase (§3.3.1's table, as far as the
/// shell knows the world: the connection and whether the scene arrived).
pub type ViewFn = fn(Phase) -> PanelView;
/// Spawns a panel's content into its body while [`PanelView::Ready`].
pub type ContentFn = fn(&mut ChildSpawner, &Theme);

/// What a panel registers: everything the shell shows of it that doesn't
/// change at run time. What does (its activity badge, its header's count)
/// its systems write into [`PanelStatus`](crate::shell::PanelStatus).
#[derive(Clone, Debug)]
pub struct PanelDef {
    pub id: PanelId,
    /// Its section header and tooltip title.
    pub title: &'static str,
    /// Its activity button and empty states.
    pub icon: IconId,
    /// The `shortcuts.json` binding that opens it, if any.
    pub binding: Option<&'static str>,
    /// Its group in the activity bar (lower first); the bar draws a
    /// separator between groups.
    pub group: u32,
    /// Its place within the group (lower first).
    pub order: u32,
    /// It opens in the primary slot at start (the first such panel in the
    /// activity bar's order).
    pub default_open: bool,
    pub view: ViewFn,
    /// Its content once ready (none yet: M4 and M7 fill the panels).
    pub content: Option<ContentFn>,
}

impl PanelDef {
    /// A panel in activity bar `group` at `order`, with no binding or
    /// content, closed at start, ready in every phase.
    pub fn new(id: PanelId, title: &'static str, icon: IconId, group: u32, order: u32) -> Self {
        Self {
            id,
            title,
            icon,
            binding: None,
            group,
            order,
            default_open: false,
            view: |_| PanelView::Ready,
            content: None,
        }
    }

    pub fn binding(mut self, id: &'static str) -> Self {
        self.binding = Some(id);
        self
    }

    /// Opens in the primary slot at start.
    pub fn default_open(mut self) -> Self {
        self.default_open = true;
        self
    }

    pub fn view(mut self, view: ViewFn) -> Self {
        self.view = view;
        self
    }

    /// What its body shows once [`PanelView::Ready`].
    pub fn content(mut self, content: ContentFn) -> Self {
        self.content = Some(content);
        self
    }

    /// Its place in the activity bar.
    fn place(&self) -> (u32, u32) {
        (self.group, self.order)
    }
}

/// The registered panels, in activity bar order. Only
/// [`RegisterPanel::register_panel`] adds to it, while the app is built.
#[derive(Resource, Clone, Debug, Default)]
pub struct PanelRegistry {
    panels: Vec<PanelDef>,
    /// The shell has spawned from it.
    closed: bool,
}

impl PanelRegistry {
    /// Adds a panel, keeping the activity bar order. An id registered again
    /// replaces the earlier panel; both that and a binding another panel
    /// already has are warned about.
    pub(crate) fn register(&mut self, def: PanelDef) {
        if self.get(def.id).is_some() {
            warn!(
                "panel '{}' is registered twice; the later registration replaces the earlier \
                 (to replace a built-in panel, disable its plugin in ShellPlugins)",
                def.id
            );
            self.panels.retain(|p| p.id != def.id);
        }
        if let Some(b) = def.binding {
            if let Some(other) = self.for_binding(b) {
                warn!(
                    "panels '{other}' and '{}' both bind '{b}'; the first in the activity bar \
                     opens",
                    def.id
                );
            }
        }
        let at = self.panels.partition_point(|p| p.place() <= def.place());
        self.panels.insert(at, def);
    }

    /// No more panels: the shell has spawned its activity bar.
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }

    /// Every panel, in activity bar order.
    pub fn iter(&self) -> impl Iterator<Item = &PanelDef> {
        self.panels.iter()
    }

    pub fn get(&self, id: PanelId) -> Option<&PanelDef> {
        self.panels.iter().find(|p| p.id == id)
    }

    /// The panel the binding `id` opens (the first in the activity bar).
    pub fn for_binding(&self, id: &str) -> Option<PanelId> {
        self.panels
            .iter()
            .find(|p| p.binding == Some(id))
            .map(|p| p.id)
    }

    /// The panel that opens at start.
    pub fn default_open(&self) -> Option<PanelId> {
        self.panels.iter().find(|p| p.default_open).map(|p| p.id)
    }
}

/// Registering a panel from its plugin.
pub trait RegisterPanel {
    /// Adds `def` to the [`PanelRegistry`]. Panics once the shell has
    /// spawned: the activity bar is built from the registry once, so a
    /// panel registered later would have a slot and a shortcut but no
    /// button.
    fn register_panel(&mut self, def: PanelDef) -> &mut Self;
}

impl RegisterPanel for App {
    fn register_panel(&mut self, def: PanelDef) -> &mut Self {
        let mut registry = self.world_mut().get_resource_or_init::<PanelRegistry>();
        assert!(
            !registry.closed,
            "register_panel('{}'): panels register while the app is built (in a plugin's \
             build); the shell has already spawned",
            def.id
        );
        registry.register(def);
        self
    }
}

/// The built-in panels, in activity bar order. Part of
/// [`ShellPlugins`](crate::shell::ShellPlugins); disable one there to
/// leave it out or replace it.
pub struct PanelPlugins;

impl PluginGroup for PanelPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(ScenePanelPlugin)
            .add(AssetsPanelPlugin)
            .add(TopicsPanelPlugin)
            .add(PlotPanelPlugin)
            .add(SensorsPanelPlugin)
            .add(TeleopPanelPlugin)
            .add(ConsolePanelPlugin)
    }
}

/// The activity bar's groups (spec §3.2): the world (Scene, Assets), then
/// the data panels.
pub const WORLD_GROUP: u32 = 0;
/// See [`WORLD_GROUP`].
pub const DATA_GROUP: u32 = 1;

#[cfg(test)]
mod tests;
