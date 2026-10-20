//! The sidebar's panels (spec §3.3; contents in M4 and M7), one folder
//! and one plugin each.
//!
//! A panel is a [`PanelDef`] registered in the [`PanelRegistry`] by its
//! plugin (`app.register_panel(…)`), the way Rerun registers view classes
//! and Zed panels: the activity bar, the sidebar's slots and the panel
//! shortcuts host whatever the registry holds, and know nothing about a
//! particular panel. What is a panel's own (its title, icon and binding,
//! its body in each connection phase, its header's count, its badge, and
//! later its systems) lives in its folder. [`Panel`] is the panel's id,
//! which [`ShellLayout`](crate::shell::ShellLayout) keeps in its slots.
//!
//! A panel can move to its own crate when it grows heavy (or compile times
//! ask for it): it depends on this module and the widgets, and its plugin
//! is the whole of its interface (DECISIONS.md D-51).

pub mod assets;
pub mod console;
pub mod plot;
pub mod scene;
pub mod sensors;
pub mod teleop;
pub mod topics;

use bevy::prelude::*;
use gz_bevy::spawn::GzIndex;

use crate::assets::icons::IconId;
use crate::shell::activity_bar::ActivityBadges;
use crate::shell::connection::Phase;
pub use crate::shell::layout::Panel;
use crate::theme::Theme;
use crate::widgets::BadgeKind;

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
/// A panel header's count ("19 entities").
pub type CountFn = fn(Phase, Option<&GzIndex>) -> Option<String>;
/// A panel's activity badge from the counts the panels keep.
pub type BadgeFn = fn(&ActivityBadges) -> Option<(BadgeKind, u32)>;
/// Spawns a panel's content into its body while [`PanelView::Ready`].
pub type ContentFn = fn(&mut ChildSpawner, &Theme);

/// What a panel registers: everything the shell shows of it.
#[derive(Clone, Debug)]
pub struct PanelDef {
    pub id: Panel,
    /// Its section header and tooltip title.
    pub title: &'static str,
    /// Its activity button and empty states.
    pub icon: IconId,
    /// The `shortcuts.json` binding that opens it, if any.
    pub binding: Option<&'static str>,
    /// Its place in the activity bar (lower first).
    pub order: u32,
    /// The activity bar draws a separator after it.
    pub separator_after: bool,
    pub view: ViewFn,
    pub count: CountFn,
    /// `Some` gives its activity button a badge (hidden at zero).
    pub badge: Option<BadgeFn>,
    /// Its content once ready (none yet: M4 and M7 fill the panels).
    pub content: Option<ContentFn>,
}

impl PanelDef {
    /// A panel at `order` with no binding, badge, count or content, ready
    /// in every phase.
    pub fn new(id: Panel, title: &'static str, icon: IconId, order: u32) -> Self {
        Self {
            id,
            title,
            icon,
            binding: None,
            order,
            separator_after: false,
            view: |_| PanelView::Ready,
            count: |_, _| None,
            badge: None,
            content: None,
        }
    }

    pub fn binding(mut self, id: &'static str) -> Self {
        self.binding = Some(id);
        self
    }

    pub fn separator_after(mut self) -> Self {
        self.separator_after = true;
        self
    }

    pub fn view(mut self, view: ViewFn) -> Self {
        self.view = view;
        self
    }

    pub fn count(mut self, count: CountFn) -> Self {
        self.count = count;
        self
    }

    pub fn badge(mut self, badge: BadgeFn) -> Self {
        self.badge = Some(badge);
        self
    }
}

/// The registered panels, in activity bar order.
#[derive(Resource, Clone, Debug, Default)]
pub struct PanelRegistry {
    panels: Vec<PanelDef>,
}

impl PanelRegistry {
    /// Adds (or replaces) a panel, keeping the activity bar order.
    pub fn register(&mut self, def: PanelDef) {
        self.panels.retain(|p| p.id != def.id);
        let at = self.panels.partition_point(|p| p.order <= def.order);
        self.panels.insert(at, def);
    }

    /// Every panel, in activity bar order.
    pub fn iter(&self) -> impl Iterator<Item = &PanelDef> {
        self.panels.iter()
    }

    pub fn get(&self, id: Panel) -> Option<&PanelDef> {
        self.panels.iter().find(|p| p.id == id)
    }

    /// The panel the binding `id` opens.
    pub fn for_binding(&self, id: &str) -> Option<Panel> {
        self.panels
            .iter()
            .find(|p| p.binding == Some(id))
            .map(|p| p.id)
    }

    /// The badge `id`'s activity button shows, if any.
    pub fn badge(&self, id: Panel, badges: &ActivityBadges) -> Option<(BadgeKind, u32)> {
        self.get(id).and_then(|p| p.badge).and_then(|f| f(badges))
    }
}

/// Registering a panel from its plugin.
pub trait RegisterPanel {
    fn register_panel(&mut self, def: PanelDef) -> &mut Self;
}

impl RegisterPanel for App {
    fn register_panel(&mut self, def: PanelDef) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<PanelRegistry>()
            .register(def);
        self
    }
}

/// Adds every panel (the shell adds this).
pub struct PanelsPlugin;

impl Plugin for PanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelRegistry>().add_plugins((
            scene::ScenePanelPlugin,
            assets::AssetsPanelPlugin,
            topics::TopicsPanelPlugin,
            plot::PlotPanelPlugin,
            sensors::SensorsPanelPlugin,
            teleop::TeleopPanelPlugin,
            console::ConsolePanelPlugin,
        ));
    }
}

#[cfg(test)]
mod tests;
