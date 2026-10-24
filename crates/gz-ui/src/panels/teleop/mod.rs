//! The Teleop panel (spec §3.3, §3.3.1; DECISIONS.md D-104): drive a
//! robot from the keyboard, sliders or a gamepad, and command its joints.
//!
//! Driving publishes `gz.msgs.Twist` on the target's drive topic, so it
//! needs a server that takes publications (the patched server's `pub_in`;
//! `GzStatus::publishes`). The targets are the drives the world's
//! SDFormat names (DiffDrive and the like, with their
//! JointPositionControllers) and any other Twist topic the server lists.
//! Armed (publishing), the drive keys and X are Teleop's: the views'
//! fly keys and the gizmo stand down (`DriveArmed`).
//!
//! The panel contract (DECISIONS.md D-58): what the user set is
//! [`model::TeleopState`]; the body is built from it and follows in
//! place.

mod model;
mod systems;
mod view;

use bevy::prelude::*;

use super::{ActionId, PanelAction, PanelDef, PanelId, RegisterPanel, DATA_GROUP};
use crate::assets::IconId;
use crate::shell::{panel_shown, ShellSystems};

/// The Teleop panel's id.
pub const TELEOP: PanelId = PanelId::new("teleop");
/// Its footer's Stop · X: stop and disarm.
pub(crate) const STOP: ActionId = ActionId::new("stop");
/// Its footer's Apply force… (waits).
pub(crate) const APPLY_FORCE: ActionId = ActionId::new("apply_force");

/// Opens the panel at start, for a capture (`gzweb --teleop`, with
/// `--select MODEL` for its drive); then the resource is removed.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TeleopStart;

/// The Teleop panel's content marker: its ready body.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TeleopBody;

/// Registers the Teleop panel.
pub struct TeleopPanelPlugin;

impl Plugin for TeleopPanelPlugin {
    fn build(&self, app: &mut App) {
        gz_bevy::topics::TopicsPlugin::ensure(app);
        app.register_panel(
            PanelDef::new(TELEOP, "Teleop", IconId::ActivityTeleop)
                .place(DATA_GROUP, 50)
                .view(systems::view(false, None, false))
                .content::<TeleopBody>()
                .footer_action(PanelAction::new(STOP, "Stop · X").disabled())
                .footer_action(PanelAction::new(APPLY_FORCE, "Apply force…").disabled()),
        )
        .init_resource::<model::TeleopState>()
        .init_resource::<systems::Targets>()
        .init_resource::<systems::Built>()
        .init_resource::<gz_bevy::view::DriveArmed>()
        .add_systems(
            Update,
            (
                (
                    systems::apply_start,
                    systems::want_sdf,
                    systems::collect_targets,
                    systems::choose_target,
                    systems::drive,
                    systems::teleop_status,
                )
                    .chain(),
                (
                    systems::sync_target,
                    systems::sync_sliders,
                    systems::mode_states,
                )
                    .chain()
                    .after(systems::teleop_status)
                    .run_if(panel_shown(TELEOP)),
            )
                .in_set(ShellSystems::Panels),
        )
        .add_observer(view::build_teleop)
        .add_observer(systems::on_mode)
        .add_observer(systems::on_target)
        .add_observer(systems::on_slider)
        .add_observer(systems::on_limit)
        .add_observer(systems::on_action);
    }
}

#[cfg(test)]
mod tests;
