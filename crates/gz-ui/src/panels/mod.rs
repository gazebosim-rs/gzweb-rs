//! The sidebar's panels (spec §3.3; contents in M4 and M7), one plugin
//! each.
//!
//! A panel is a [`PanelDef`] registered by its plugin with
//! `app.register_panel(…)`, the way Rerun registers view classes and Zed
//! panels: the activity bar, the sidebar's slots and the panel shortcuts
//! host whatever the [`PanelRegistry`] holds, and know nothing about a
//! particular panel. A panel is known by its [`PanelId`], an open id: a
//! panel in another crate brings its own, and
//! [`ShellLayout`](crate::shell::ShellLayout) keeps ids in its slots.
//!
//! The contract (DECISIONS.md D-58):
//!
//! - **Static:** the [`PanelDef`]: title, icon, binding, place in the
//!   activity bar, its [`HeaderAction`]s, the [`PanelView`] it starts
//!   with, the marker component its content is built under, and its
//!   footer (D-98: [`PanelAction`]s and a footer content marker, drawn by
//!   the shell under every state).
//! - **What changes:** its own systems write its view, activity badge,
//!   header count and actions' enabled state into the shell's [`PanelStatus`](crate::shell::PanelStatus)
//!   (through [`SetPanelStatus`](crate::shell::SetPanelStatus), which marks
//!   a change only), in [`ShellSystems::Panels`](crate::shell::ShellSystems).
//!   The shell rebuilds a body only when its view differs.
//! - **Content:** a [`PanelView::Ready`] body gets the panel's marker, and
//!   the panel's own observer on `Add<Marker>` fills it with whatever
//!   system parameters it needs. The body is full-bleed; the shell pads
//!   only the empty, loading and error states.
//! - **Actions:** buttons in an empty or error state or the footer
//!   ([`PanelAction`]) and in the header ([`HeaderAction`]) trigger
//!   [`PanelActionActivated`], which the panel observes. Reconnect
//!   ([`PanelAction::reconnect`]) is the shell's own. One unavailable for
//!   good says why (`unavailable`).
//! - **Hosts:** a panel shows in a host (D-98: a sidebar slot, a pop-out
//!   in M7e); gate a system that only feeds what it shows with
//!   [`panel_shown`](crate::shell::panel_shown).
//! - **Bodies are disposable views.** A view change, a swap or (M7) a
//!   pop-out respawns a body; the shell keeps it across a second slot
//!   opening or closing and moves it on a swap, but a panel never relies
//!   on that. Its state (tree expansion, filter text, scroll offset, plot
//!   series) lives in its own resources, which its content is built from.
//!
//! The built-in panels come as [`PanelPlugins`]; an app replaces one by
//! disabling its plugin and registering its own panel under the same id
//! (registering an id twice panics: the earlier plugin's systems would
//! still run). Panels register while the app is built: once the shell has
//! spawned, the registry is closed (DECISIONS.md D-57).
//!
//! A panel can move to its own crate when it grows heavy (or compile times
//! ask for it): it depends on this module and the widgets, and its plugin
//! is the whole of its interface (DECISIONS.md D-51).

mod assets;
mod console;
mod plot;
mod scene;
pub(crate) mod sensors;
mod teleop;
mod topics;

use std::borrow::Cow;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

use crate::assets::IconId;
use crate::registry::{self, ui_id, Registered, Registry};
pub use assets::{AssetsPanelPlugin, AssetsStart, ASSETS};
pub use console::{ConsolePanelPlugin, ConsoleUnread, CONSOLE};
pub use plot::{PlotCharts, PlotPanelPlugin, PlotPick, PlotStart, PLOT};
pub use scene::{ScenePanelPlugin, SceneTreeStart, SCENE};
pub use sensors::{SensorsPanelPlugin, SensorsStart, ViewSensor, SENSORS};
pub use teleop::{TeleopPanelPlugin, TeleopStart, TELEOP};
pub use topics::{TopicsPanelPlugin, TopicsStart, TOPICS};

ui_id! {
    /// A panel's id: a short, stable name ("scene"), unique among the
    /// registered panels. Open, like Rerun's `ViewClassIdentifier`: a panel
    /// in any crate declares its own as a constant. The layout keeps ids in
    /// its slots, and a saved layout (M8) keys by [`name`](Self::name).
    PanelId
}

ui_id! {
    /// A panel action's id: a short name unique within its panel
    /// ("add_field"), declared as a constant beside the panel's id.
    ActionId
}

/// What a panel's body shows (§3.3.1). A panel's systems write it into
/// [`PanelStatus`](crate::shell::PanelStatus); until they do, the body
/// shows [`PanelDef::view`].
#[derive(Clone, Debug, PartialEq)]
pub enum PanelView {
    /// The panel's content, built by its own observer
    /// ([`PanelDef::content`]); full-bleed.
    Ready,
    /// Centred: the panel's icon, the message and the actions.
    Empty {
        message: Cow<'static, str>,
        actions: Vec<PanelAction>,
    },
    /// Skeleton rows.
    Loading,
    /// Centred: a warning icon, the message and the actions.
    Error {
        message: Cow<'static, str>,
        actions: Vec<PanelAction>,
    },
}

impl PanelView {
    /// The empty state with `message` and no buttons.
    pub fn empty(message: impl Into<Cow<'static, str>>) -> Self {
        PanelView::Empty {
            message: message.into(),
            actions: vec![],
        }
    }

    /// The error state with `message` and no buttons.
    pub fn error(message: impl Into<Cow<'static, str>>) -> Self {
        PanelView::Error {
            message: message.into(),
            actions: vec![],
        }
    }

    /// Adds a button to an empty or error state (other states have none).
    pub fn action(mut self, action: PanelAction) -> Self {
        if let PanelView::Empty { actions, .. } | PanelView::Error { actions, .. } = &mut self {
            actions.push(action);
        }
        self
    }
}

/// A button in an empty or error state (§3.3.1) or in a panel's footer
/// (§3.3). Activating it triggers [`PanelActionActivated`] with its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelAction {
    pub id: ActionId,
    pub label: Cow<'static, str>,
    /// A disabled action shows dimmed, with [`why`](Self::why). A footer
    /// action keeps this state until the panel's systems set one
    /// ([`SetPanelStatus::set_action_enabled`](crate::shell::SetPanelStatus::set_action_enabled)).
    pub enabled: bool,
    /// What a disabled action's tooltip says (none: "Not available yet").
    pub why: Option<&'static str>,
}

/// [`PanelAction::reconnect`]'s id: the shell retries the server.
pub const RECONNECT: ActionId = ActionId::new("shell.reconnect");

impl PanelAction {
    pub fn new(id: ActionId, label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            why: None,
        }
    }

    /// Shown, but not available yet (what it does comes in a later
    /// milestone).
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Shown, but unavailable for the reason `why` gives (the tooltip's
    /// detail while it is disabled).
    pub fn unavailable(mut self, why: &'static str) -> Self {
        self.enabled = false;
        self.why = Some(why);
        self
    }

    /// Retry the server now: the shell's own action, which it handles for
    /// every panel.
    pub fn reconnect() -> Self {
        Self::new(RECONNECT, "Reconnect")
    }
}

/// An icon button in a panel's section header (§3.3: up to three, before
/// the shell's slot buttons). Activating it triggers
/// [`PanelActionActivated`] with its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderAction {
    pub id: ActionId,
    pub icon: IconId,
    /// Its tooltip's title.
    pub tooltip: &'static str,
    /// The `shortcuts.json` binding that activates it while its panel is
    /// shown (its tooltip shows the keys), if any.
    pub binding: Option<&'static str>,
    /// A disabled action shows dimmed, with [`why`](Self::why). This is
    /// its state until the panel's systems set one
    /// ([`SetPanelStatus::set_action_enabled`](crate::shell::SetPanelStatus::set_action_enabled)).
    pub enabled: bool,
    /// What a disabled action's tooltip says (none: "Not available yet").
    pub why: Option<&'static str>,
    /// A toggle's icon and tooltip while it is on (Pause shows Resume),
    /// which its panel's systems set
    /// ([`SetPanelStatus::set_action_on`](crate::shell::SetPanelStatus::set_action_on)).
    pub on: Option<(IconId, &'static str)>,
}

impl HeaderAction {
    pub fn new(id: ActionId, icon: IconId, tooltip: &'static str) -> Self {
        Self {
            id,
            icon,
            tooltip,
            binding: None,
            enabled: true,
            why: None,
            on: None,
        }
    }

    /// A toggle: while its panel says it is on, it shows `icon` and
    /// `tooltip` instead (and is pressed, to AccessKit).
    pub fn toggle(mut self, icon: IconId, tooltip: &'static str) -> Self {
        self.on = Some((icon, tooltip));
        self
    }

    /// Its icon and tooltip, on or off.
    pub(crate) fn look(&self, on: bool) -> (IconId, &'static str) {
        match self.on {
            Some(look) if on => look,
            _ => (self.icon, self.tooltip),
        }
    }

    pub fn binding(mut self, id: &'static str) -> Self {
        self.binding = Some(id);
        self
    }

    /// Shown, but not available yet.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Shown, but unavailable for the reason `why` gives.
    pub fn unavailable(mut self, why: &'static str) -> Self {
        self.enabled = false;
        self.why = Some(why);
        self
    }
}

/// A panel's action button was activated: a [`PanelAction`] in its body or
/// footer, or a [`HeaderAction`]. `entity` is the button (an anchor for a popover);
/// a panel observes the event globally and matches its `panel` id.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelActionActivated {
    pub entity: Entity,
    pub panel: PanelId,
    pub action: ActionId,
}

/// A panel's body in the sidebar, on every state. A ready body also
/// carries the panel's content marker ([`PanelDef::content`]).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelBody(pub PanelId);

/// Inserts a panel's content marker on its ready body.
pub type ContentMarker = fn(&mut EntityCommands);

/// What a panel registers: everything the shell shows of it that doesn't
/// change at run time. What does (its view, activity badge, header count
/// and header actions' enabled state) its systems write into
/// [`PanelStatus`](crate::shell::PanelStatus).
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
    /// Its body until its systems write a view into
    /// [`PanelStatus`](crate::shell::PanelStatus): a panel whose state
    /// never changes (until a later milestone) needs no system.
    pub view: PanelView,
    /// Its section header's icon buttons, in order.
    pub header_actions: Vec<HeaderAction>,
    /// Marks its body while [`PanelView::Ready`], for its observer to fill
    /// ([`content`](Self::content)).
    pub content: Option<ContentMarker>,
    /// Its footer's buttons, in order (§3.3: equal widths).
    pub footer: Vec<PanelAction>,
    /// Marks its footer, for its observer to fill
    /// ([`footer_content`](Self::footer_content)).
    pub footer_content: Option<ContentMarker>,
}

impl PanelDef {
    /// A panel with no binding, header actions or content, closed at
    /// start, ready until its systems say otherwise, after every placed
    /// panel in the activity bar until [`place`](Self::place)d.
    pub fn new(id: PanelId, title: &'static str, icon: IconId) -> Self {
        Self {
            id,
            title,
            icon,
            binding: None,
            group: u32::MAX,
            order: 0,
            default_open: false,
            view: PanelView::Ready,
            header_actions: Vec::new(),
            content: None,
            footer: Vec::new(),
            footer_content: None,
        }
    }

    /// Its place in the activity bar: `group` (a separator between
    /// groups; [`WORLD_GROUP`], [`DATA_GROUP`]), then `order` in it.
    pub fn place(mut self, group: u32, order: u32) -> Self {
        (self.group, self.order) = (group, order);
        self
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

    /// Its body until its systems write one.
    pub fn view(mut self, view: PanelView) -> Self {
        self.view = view;
        self
    }

    /// Adds an icon button to its section header.
    pub fn header_action(mut self, action: HeaderAction) -> Self {
        self.header_actions.push(action);
        self
    }

    /// Its content: a ready body gets `C::default()`, and the panel's own
    /// observer on `Add<C>` builds the content into it with whatever
    /// system parameters it needs (`On<Add<C>>`, `Commands`, its
    /// resources). The body is respawned whenever its view changes, so the
    /// content is built from the panel's resources, never kept in it.
    pub fn content<C: Component + Default>(mut self) -> Self {
        self.content = Some(|body| {
            body.insert(C::default());
        });
        self
    }

    /// Adds a button to its footer, which the shell draws under the body
    /// in every state (§3.3).
    pub fn footer_action(mut self, action: PanelAction) -> Self {
        self.footer.push(action);
        self
    }

    /// Its footer's content: the shell's footer row gets `C::default()`,
    /// after any [`footer_action`](Self::footer_action)s, and the panel's
    /// own observer on `Add<C>` builds into it (a command input, buttons
    /// with an on state). Like the body it is disposable: it is respawned
    /// when its host changes panels.
    pub fn footer_content<C: Component + Default>(mut self) -> Self {
        self.footer_content = Some(|footer| {
            footer.insert(C::default());
        });
        self
    }

    /// Whether it has a footer.
    pub fn has_footer(&self) -> bool {
        !self.footer.is_empty() || self.footer_content.is_some()
    }

    /// Its header or footer action `id`, as registered: whether it starts
    /// enabled, and why not.
    pub fn action(&self, id: ActionId) -> Option<(bool, Option<&'static str>)> {
        let header = self.header_actions.iter().find(|a| a.id == id);
        let footer = self.footer.iter().find(|a| a.id == id);
        header
            .map(|a| (a.enabled, a.why))
            .or_else(|| footer.map(|a| (a.enabled, a.why)))
    }
}

impl Registered for PanelDef {
    type Id = PanelId;
    const KIND: &'static str = "panel";

    fn id(&self) -> PanelId {
        self.id
    }

    fn sort_key(&self) -> (u32, u32) {
        (self.group, self.order)
    }
}

/// The registered panels, in activity bar order. Only
/// [`RegisterPanel::register_panel`] adds to it, while the app is built.
pub type PanelRegistry = Registry<PanelDef>;

impl Registry<PanelDef> {
    /// The panel the binding `id` opens (the first in the activity bar).
    pub fn for_binding(&self, id: &str) -> Option<PanelId> {
        self.iter().find(|p| p.binding == Some(id)).map(|p| p.id)
    }

    /// The panel that opens at start: the first marked
    /// [`default_open`](PanelDef::default_open), else the first in the
    /// activity bar (an app may have disabled the default one).
    pub fn default_open(&self) -> Option<PanelId> {
        self.iter()
            .find(|p| p.default_open)
            .or_else(|| self.first())
            .map(|p| p.id)
    }

    /// `id` if it is registered, else the first panel (with a warning): a
    /// command or a restored layout naming a panel that isn't there.
    pub fn resolve(&self, id: PanelId) -> Option<PanelId> {
        if self.get(id).is_some() {
            return Some(id);
        }
        let first = self.first().map(|p| p.id);
        warn!("no panel '{id}' is registered; showing {first:?} instead");
        first
    }
}

/// Registering a panel from its plugin.
pub trait RegisterPanel {
    /// Adds `def` to the [`PanelRegistry`]. Panics on an id already
    /// registered (to replace a built-in panel, disable its plugin), and
    /// once the shell has spawned: the activity bar is built from the
    /// registry once, so a panel registered later would have a slot and a
    /// shortcut but no button. A binding another panel already has warns;
    /// the first panel in the activity bar keeps it.
    fn register_panel(&mut self, def: PanelDef) -> &mut Self;
}

impl RegisterPanel for App {
    fn register_panel(&mut self, def: PanelDef) -> &mut Self {
        if let Some(b) = def.binding {
            let other = self
                .world()
                .get_resource::<PanelRegistry>()
                .and_then(|r| r.for_binding(b));
            if let Some(other) = other {
                warn!(
                    "panels '{other}' and '{}' both bind '{b}'; the first in the activity bar \
                     opens",
                    def.id
                );
            }
        }
        registry::register(self, def);
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
