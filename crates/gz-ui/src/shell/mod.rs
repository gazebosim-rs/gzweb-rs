//! The workbench shell (spec §2, §3; milestone M2): top bar, activity bar,
//! sidebar with two slots, viewport area, inspector and status bar, the
//! splitters and collapse rules, the 3D view inside the viewport area, and
//! the connect card and disconnected banner.
//!
//! [`ShellPlugin`] is added on top of [`GzUiPlugin`](crate::GzUiPlugin) in
//! the full UI mode. The layout state is [`ShellLayout`]; everything the
//! user asks for goes through [`UiCommand`] messages, which
//! `commands::apply_ui_commands` applies, so buttons, keys and tests share
//! one path. Sim requests are `gz-bevy` messages (`GzCommand`,
//! `GzReconnect`).
//!
//! A file or folder per region (`top_bar/`, `activity_bar/`, `sidebar/`,
//! `viewport/`, `status_bar/`, the inspector frame), each registering its
//! own systems; the sidebar and the inspector host the
//! [panels](crate::panels) and [inspectors](crate::inspectors) registered
//! by their plugins, which this plugin adds.

pub(crate) mod activity_bar;
pub(crate) mod collapsed_bar;
pub(crate) mod commands;
pub mod connection;
pub(crate) mod format;
pub(crate) mod inspector;
pub mod layout;
pub(crate) mod regions;
pub(crate) mod routing;
pub(crate) mod sidebar;
pub(crate) mod splitter;
pub(crate) mod status_bar;
pub(crate) mod top_bar;
pub(crate) mod viewport;

use bevy::prelude::*;
use gz_bevy::connection::{GzCommand, GzReconnect};

pub use activity_bar::ActivityBadges;
pub use commands::UiCommand;
pub use layout::{Panel, ShellLayout, Side, Slot};
pub use top_bar::tools::Tool;

use crate::inspectors::InspectorsPlugin;
use crate::panels::PanelsPlugin;
use crate::GzUiSystems;

/// The shell's Update systems, in [`GzUiSystems::Behaviour`] and in this
/// order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ShellSystems {
    /// The connection phase, keys, commands and the layout, then the
    /// sidebar's slots, in a chain.
    Layout,
    /// Everything that shows state: readouts, button states, overlays.
    /// Each region adds its own.
    Readouts,
}

/// Adds the shell. `camera` spawns the UI camera (tests bring their own).
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
            .add_message::<GzCommand>()
            .add_message::<GzReconnect>()
            .init_resource::<connection::ConnectionView>()
            .init_resource::<connection::CurrentPhase>()
            .configure_sets(
                Update,
                (ShellSystems::Layout, ShellSystems::Readouts)
                    .chain()
                    .in_set(GzUiSystems::Behaviour),
            )
            .add_systems(PreStartup, regions::init_layout)
            .add_systems(Startup, regions::spawn_shell)
            .add_systems(
                Update,
                (
                    connection::track_connection,
                    commands::shortcut_keys,
                    commands::apply_ui_commands,
                    regions::fit_to_window,
                    regions::apply_layout,
                    sidebar::apply_slots,
                    sidebar::rebuild_slots,
                )
                    .chain()
                    .in_set(ShellSystems::Layout),
            );
        app.add_plugins((PanelsPlugin, InspectorsPlugin));
        routing::build(app);
        splitter::build(app);
        collapsed_bar::build(app);
        activity_bar::build(app);
        top_bar::build(app);
        sidebar::build(app);
        inspector::build(app);
        status_bar::build(app);
        viewport::build(app);
        if self.camera {
            app.add_systems(Startup, regions::spawn_ui_camera);
        }
    }
}

#[cfg(test)]
mod tests;
