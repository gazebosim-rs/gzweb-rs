//! The workbench shell (spec §2, §3; milestone M2): top bar, activity bar,
//! sidebar with two slots, viewport area, inspector and status bar, the
//! splitters and collapse rules, the 3D view inside the viewport area, and
//! the connect card and disconnected banner.
//!
//! [`ShellPlugin`] is added on top of [`GzUiPlugin`](crate::GzUiPlugin) in
//! the full UI mode. The layout state is [`ShellLayout`]; everything the
//! user asks for goes through [`UiCommand`] messages, which
//! [`apply_ui_commands`](commands::apply_ui_commands) applies, so buttons,
//! keys and tests share one path. Sim requests are `gz-bevy` messages
//! (`GzCommand`, `GzReconnect`).

pub mod activity_bar;
pub mod collapsed_bar;
pub mod commands;
pub mod connection;
pub mod format;
pub mod inspector;
pub mod layout;
pub mod regions;
pub mod routing;
pub mod sidebar;
pub mod splitter;
pub mod status_bar;
pub mod top_bar;
pub mod viewport;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use gz_bevy::connection::{GzCommand, GzReconnect};
use gz_bevy::view::controls::{ViewPlacementSystems, ViewRouting};
use gz_bevy::view::ViewPlacement;

pub use collapsed_bar::ExpandButton;
pub use commands::UiCommand;
pub use layout::{Panel, ShellLayout, Side, Slot};
pub use regions::Region;
pub use splitter::ShellSplitter;
pub use top_bar::tools::Tool;
pub use top_bar::transport::StepsPerClick;

use crate::GzUiSystems;

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
            .init_resource::<Tool>()
            .init_resource::<StepsPerClick>()
            .init_resource::<connection::ConnectionView>()
            .init_resource::<connection::CurrentPhase>()
            .init_resource::<ViewRouting>()
            .init_resource::<routing::ViewFocus>()
            .init_resource::<status_bar::CursorReadout>()
            .add_systems(PreStartup, regions::init_layout)
            .add_systems(Startup, regions::spawn_shell)
            .add_systems(
                PreUpdate,
                routing::route_pointer.after(bevy::picking::PickingSystems::Hover),
            )
            .add_systems(
                Update,
                (
                    connection::track_connection,
                    commands::shortcut_keys,
                    commands::apply_ui_commands,
                    regions::fit_to_window,
                    regions::apply_layout,
                    sidebar::rebuild_slots,
                )
                    .chain()
                    .in_set(GzUiSystems::Behaviour),
            )
            .add_systems(
                Update,
                (
                    activity_bar::activity_states,
                    top_bar::tools::tool_states,
                    top_bar::transport::transport_states,
                    top_bar::top_bar_texts,
                    inspector::inspector_texts,
                    status_bar::status_texts,
                    status_bar::narrow_rules,
                    status_bar::cursor_readout,
                    viewport::overlays,
                    splitter::splitter_strokes,
                )
                    .in_set(GzUiSystems::Behaviour)
                    .after(sidebar::rebuild_slots),
            )
            .add_systems(
                Update,
                viewport::connect_card::fade_card.after(GzUiSystems::Visuals),
            )
            .init_resource::<ViewPlacement>()
            .add_systems(
                PostUpdate,
                viewport::view::place_view
                    .after(UiSystems::Layout)
                    .before(ViewPlacementSystems)
                    .before(VisibilitySystems::UpdateFrusta),
            );
        splitter::build(app);
        collapsed_bar::build(app);
        activity_bar::build(app);
        top_bar::build(app);
        sidebar::build(app);
        inspector::build(app);
        viewport::build(app);
        if self.camera {
            app.add_systems(Startup, regions::spawn_ui_camera);
        }
    }
}

#[cfg(test)]
mod tests;
