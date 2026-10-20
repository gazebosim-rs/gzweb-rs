//! The World inspector's readouts and quick actions.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::spawn::GzIndex;

use super::view::{WorldAction, WorldField};
use crate::shell::connection::{world_name, CurrentPhase, Phase};
use crate::shell::format;
use crate::theme::{ColorRole, Ink};
use crate::widgets::text::set_text;

/// The world's name, its source (the server) and the step size, when the
/// connection or the scene changes.
pub(super) fn world_fields(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut texts: Query<(&WorldField, &mut Text, &mut Ink)>,
) {
    let changed =
        status.as_ref().is_some_and(Res::is_changed) || index.as_ref().is_some_and(Res::is_changed);
    if !changed {
        return;
    }
    let status = status.as_deref();
    let name = world_name(status, index.as_deref());
    let step = status
        .and_then(|s| s.sim)
        .map(|s| s.step_size)
        .filter(|s| *s > 0.0);
    for (which, mut t, mut ink) in &mut texts {
        let (want, role) = match which {
            WorldField::Name => match &name {
                Some(n) => (n.clone(), ColorRole::Text),
                None => ("No world".into(), ColorRole::Text3),
            },
            WorldField::Source => (
                status.map_or_else(String::new, |s| format::address(&s.url)),
                ColorRole::Text3,
            ),
            WorldField::Step => match step {
                Some(s) => (format::step(s), ColorRole::Text),
                None => ("—".into(), ColorRole::Text3),
            },
        };
        set_text(&mut t, &want);
        ink.set_if_neq(Ink(role));
    }
}

/// Reset needs a live world, as the top bar's (DECISIONS.md D-56).
pub(super) fn reset_availability(
    phase: Res<CurrentPhase>,
    buttons: Query<(Entity, &WorldAction, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if !phase.is_changed() {
        return;
    }
    let live = phase.0 == Phase::Live;
    for (e, which, disabled) in &buttons {
        if *which == WorldAction::Reset && live == disabled {
            if live {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}

/// Reset resets the world (Save and Statistics do nothing yet).
pub(super) fn on_world_action(
    ev: On<Activate>,
    buttons: Query<(&WorldAction, Has<InteractionDisabled>)>,
    mut sim: MessageWriter<GzCommand>,
) {
    if let Ok((WorldAction::Reset, false)) = buttons.get(ev.entity) {
        sim.write(GzCommand::Reset);
    }
}
