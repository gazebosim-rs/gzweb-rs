//! The workbench shell (spec §2, §3; milestone M2): top bar, activity bar,
//! sidebar with two slots, viewport area, inspector and status bar, the
//! splitters and collapse rules, the 3D view inside the viewport area, and
//! the connect card and disconnected banner.
//!
//! [`ShellPlugins`] (the shell and the built-in panels and inspectors) is
//! added on top of [`GzUiPlugin`](crate::GzUiPlugin) in the full UI mode.
//! The layout state is [`ShellLayout`]; everything the user asks for goes
//! through [`UiCommand`] messages, which `commands::apply_ui_commands`
//! applies, so buttons, keys and tests share one path. Sim requests are
//! `gz-bevy` messages (`GzCommand`, `GzReconnect`).
//!
//! A file or folder per region (`top_bar/`, `activity_bar/`, `sidebar/`,
//! `viewport/`, `status_bar/`, the inspector frame), each registering its
//! own systems; the sidebar and the inspector host the
//! [panels](crate::panels) and [inspectors](crate::inspectors) registered
//! by their plugins, and show what each panel writes into [`PanelStatus`].
//! Each region is a tab group, in the spec's order (§6.5).

pub(crate) mod activity_bar;
pub(crate) mod collapsed_bar;
pub(crate) mod commands;
pub(crate) mod connection;
pub(crate) mod context_menu;
pub(crate) mod host;
pub(crate) mod inspector;
pub(crate) mod layout;
pub(crate) mod palette;
pub(crate) mod panel_status;
pub(crate) mod popout;
pub(crate) mod regions;
pub(crate) mod routing;
pub(crate) mod sidebar;
pub(crate) mod splitter;
pub(crate) mod status_bar;
pub(crate) mod top_bar;
pub(crate) mod viewport;
pub(crate) mod workspaces;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use gz_bevy::connection::{GzCommand, GzReconnect};

pub use commands::UiCommand;
pub use connection::{world_name, CurrentPhase, Phase};
pub use host::ShownPanels;
pub use layout::{
    Edges, PopOut, ShellLayout, Side, Slot, COLLAPSED_BAR_W, DEFAULT_SPLIT, INSPECTOR_START_W,
    MIN_SLOT_H, MIN_WINDOW, POPOUT_MIN, POPOUT_SIZE, SIDEBAR_START_W, STARTUP_FRAMES, STARTUP_SECS,
};
pub use palette::StartPalette;
pub use panel_status::{PanelState, PanelStatus, SetPanelStatus};
pub use popout::StartPopOuts;
pub use top_bar::tools::Tool;
pub use top_bar::StartMenu;
pub use viewport::views::{LayoutPreset, StartLayout};
pub use workspaces::{
    CurrentLayout, Workspace, WorkspaceCommand, WorkspaceStore, Workspaces, WORKSPACES_PANEL,
};

use crate::inspectors::{InspectorPlugins, InspectorRegistry};
use crate::panels::{PanelId, PanelPlugins, PanelRegistry};
use crate::theme::Theme;
use crate::GzUiSystems;

/// The shell's Update systems, in [`GzUiSystems::Behaviour`] and in this
/// order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ShellSystems {
    /// The connection phase, keys, commands and the layout, in a chain.
    Layout,
    /// The panels' own systems: they write their views, badges, counts and
    /// header actions' state into [`PanelStatus`] (gate one that only
    /// feeds a shown panel with [`panel_shown`]).
    Panels,
    /// The sidebar's slots rebuild what changed (headers, bodies), then
    /// their headers' counts and actions follow [`PanelStatus`]; before
    /// the readouts, so those find this frame's entities.
    Slots,
    /// Everything else that shows state: readouts, button states,
    /// overlays. Each region adds its own.
    Readouts,
}

/// A run condition: `panel` is on screen in a host ([`ShownPanels`]: a
/// slot of the open sidebar). For a panel's systems that only feed what
/// it shows (a topic echo's subscription); what it must keep while
/// closed (Plot's series, the Console's log) runs unconditionally, and so
/// do the systems writing its [`PanelStatus`], which the shell reads when
/// the panel opens.
pub fn panel_shown(panel: PanelId) -> impl FnMut(Option<Res<ShownPanels>>) -> bool + Clone {
    move |shown| shown.is_some_and(|s| s.is_shown(panel))
}

/// The shell with the built-in panels and inspectors: [`ShellPlugin`],
/// then [`PanelPlugins`] and [`InspectorPlugins`], flattened, so an app can
/// leave one out or replace it:
/// `ShellPlugins.build().disable::<TeleopPanelPlugin>()`, then its own
/// plugin registering a panel under the same id (DECISIONS.md D-57).
pub struct ShellPlugins;

impl PluginGroup for ShellPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(ShellPlugin::default())
            .add_group(PanelPlugins)
            .add_group(InspectorPlugins)
    }
}

/// Adds the shell, with no panels or inspectors (see [`ShellPlugins`]).
/// `camera` spawns the UI camera (tests bring their own).
pub struct ShellPlugin {
    pub camera: bool,
}

impl Default for ShellPlugin {
    fn default() -> Self {
        Self { camera: true }
    }
}

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UiCommand>()
            .add_message::<gz_bevy::view::controls::RunBinding>()
            .add_message::<gz_bevy::overlays::OverlayCommand>()
            .add_message::<GzCommand>()
            .add_message::<GzReconnect>()
            .init_resource::<PanelRegistry>()
            .init_resource::<InspectorRegistry>()
            .init_resource::<PanelStatus>()
            .init_resource::<ShownPanels>()
            .init_resource::<connection::ConnectionView>()
            .init_resource::<CurrentPhase>()
            .configure_sets(
                Update,
                (
                    ShellSystems::Layout,
                    ShellSystems::Panels,
                    ShellSystems::Slots,
                    ShellSystems::Readouts,
                )
                    .chain()
                    .in_set(GzUiSystems::Behaviour),
            )
            .add_systems(PreStartup, regions::init_layout)
            .add_systems(Startup, regions::spawn_shell)
            .add_observer(regions::add_tab_groups)
            .add_systems(
                Update,
                (
                    connection::track_connection,
                    commands::shortcut_keys,
                    commands::header_action_keys,
                    commands::apply_ui_commands,
                    regions::fit_to_window,
                    regions::apply_layout,
                )
                    .chain()
                    .in_set(ShellSystems::Layout)
                    // Before the views see Esc cancel a marquee.
                    .before(gz_bevy::view::controls::mouse_input)
                    // ⌘Z after an edit in the same frame undoes that edit.
                    .before(gz_bevy::undo::UndoSystems),
            );
        routing::build(app);
        splitter::build(app);
        collapsed_bar::build(app);
        activity_bar::build(app);
        top_bar::build(app);
        sidebar::build(app);
        popout::build(app);
        palette::build(app);
        context_menu::build(app);
        workspaces::build(app);
        inspector::build(app);
        status_bar::build(app);
        viewport::build(app);
        if self.camera {
            app.add_systems(Startup, regions::spawn_ui_camera);
        }
    }

    fn finish(&self, app: &mut App) {
        assert!(
            app.world().contains_resource::<Theme>(),
            "ShellPlugin needs GzUiPlugin (the theme, fonts, icons and widgets): add \
             GzUiPlugin alongside ShellPlugins"
        );
    }
}

#[cfg(test)]
mod tests;
