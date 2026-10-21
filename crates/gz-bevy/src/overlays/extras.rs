//! Collisions, inertials and joints for the overlays, from the world's
//! SDFormat (`gz_core::sdf`, DECISIONS.md D-62).
//!
//! The scene message has none of them, so the first time an overlay that
//! draws one is turned on, the client calls the world's
//! `generate_world_sdf` service (included models expanded) and parses the
//! reply. It asks again when the set of models changes (a spawn, a
//! reconnect) while one of those overlays is on, or while something else
//! wants it ([`SdfWanted`]: the UI's scene tree lists the joints). One
//! request is in flight at a time, so a burst of spawns asks once more
//! after the reply rather than once per model. A server without the
//! service, or a reply that isn't SDFormat, is logged once and left at
//! that.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::sdf::{self, WorldExtras};
use prost::Message as _;

use super::Overlays;
use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::spawn::GzIndex;

/// What the overlays know about the world beyond the scene message.
#[derive(Resource, Debug, Default)]
pub struct SdfExtras {
    /// The last parsed reply.
    pub extras: Option<WorldExtras>,
    /// Bumped on every new reply, so the collision meshes respawn.
    pub generation: u32,
    /// The world and model count the last request was for.
    asked: Option<(String, usize)>,
    /// The service the last request went to, while its reply is due.
    pending: Option<String>,
}

/// The world's SDFormat is wanted beyond the overlays (the UI's scene tree
/// sets it while it shows, for the joints).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SdfWanted(pub bool);

/// Asks for the world's SDFormat when an overlay (or [`SdfWanted`]) needs
/// it and the world (or its set of models) is new since the last request,
/// unless a request is still waiting for its reply. A connection that
/// isn't ready drops the waiting request and asks again once it is.
pub fn request_extras(
    overlays: Res<Overlays>,
    wanted: Option<Res<SdfWanted>>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut state: ResMut<SdfExtras>,
    mut out: MessageWriter<GzCommand>,
) {
    let ready = status
        .as_ref()
        .is_some_and(|s| s.status == gz_core::transport::Status::Ready);
    if !ready {
        if state.pending.is_some() || state.asked.is_some() {
            state.pending = None;
            state.asked = None;
        }
        return;
    }
    if !overlays.need_sdf() && !wanted.is_some_and(|w| w.0) || state.pending.is_some() {
        return;
    }
    let (Some(world), Some(index)) = (status.and_then(|s| s.world.clone()), index) else {
        return;
    };
    if index.scene.is_none() {
        return;
    }
    let key = (world, index.model_names.len());
    if state.asked.as_ref() == Some(&key) {
        return;
    }
    let service = sdf::generate_service(&key.0);
    out.write(GzCommand::Request {
        service: service.clone(),
        msg_type: "gz.msgs.SdfGeneratorConfig".into(),
        payload: sdf::generate_request(),
    });
    state.pending = Some(service);
    state.asked = Some(key);
}

/// Parses the `generate_world_sdf` reply into [`SdfExtras`].
pub fn receive_extras(mut replies: MessageReader<GzServiceReply>, mut state: ResMut<SdfExtras>) {
    for reply in replies.read() {
        if state.pending.as_deref() != Some(reply.service.as_str()) {
            continue;
        }
        state.pending = None;
        let xml = msgs::StringMsg::decode(reply.payload.as_slice())
            .map(|m| m.data)
            .unwrap_or_default();
        match WorldExtras::parse(&xml) {
            Ok(extras) => {
                state.extras = Some(extras);
                state.generation += 1;
            }
            Err(e) => warn!(
                "{}: no collisions, inertials or joints to draw ({e}; reply: {:.60})",
                reply.service, xml
            ),
        }
    }
}
