//! Collisions, inertials and joints for the overlays, from the world's
//! SDFormat (`gz_core::sdf`, DECISIONS.md D-62).
//!
//! The scene message has none of them, so the first time an overlay that
//! draws one is turned on, the client calls the world's
//! `generate_world_sdf` service (included models expanded) and parses the
//! reply. It asks again when the set of models changes (a spawn, a
//! deletion) or the connection comes back, while one of those overlays is
//! on or something else wants it ([`SdfWanted`]: the UI's scene tree lists
//! the joints). One request is in flight at a time: a change while it is out is
//! asked for when its reply lands, so the last reply is always for the
//! latest set. A server without the service, or a reply that isn't
//! SDFormat, is logged once per world and model set.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::sdf::{self, WorldExtras};
use gz_core::transport::Status;
use prost::Message as _;
use std::collections::HashSet;

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
    /// The world and model set the last request was for.
    asked: Option<(String, HashSet<String>)>,
    /// The service the last request went to, while its reply is due.
    pending: Option<String>,
}

/// The world's SDFormat is wanted beyond the overlays, by these holders
/// (the UI's scene tree while it shows, for the joints; the Sensors panel,
/// for the sensors' topics).
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct SdfWanted(pub(crate) HashSet<&'static str>);

impl SdfWanted {
    /// Whether anything wants it.
    pub fn any(&self) -> bool {
        !self.0.is_empty()
    }

    /// Whether `holder` wants it.
    pub fn by(&self, holder: &str) -> bool {
        self.0.contains(holder)
    }
}

/// Setting a holder's want on `ResMut<SdfWanted>`, marking it changed
/// only on a change.
pub fn want_sdf(wanted: &mut ResMut<SdfWanted>, holder: &'static str, on: bool) {
    if wanted.bypass_change_detection().by(holder) != on {
        if on {
            wanted.0.insert(holder);
        } else {
            wanted.0.remove(holder);
        }
    }
}

/// Asks for the world's SDFormat when an overlay (or [`SdfWanted`]) needs
/// it and the world
/// (or its set of models) is new since the last request, once no request
/// is in flight. While the connection isn't ready, it forgets what it
/// asked: a reply lost to a disconnect is asked for again, and so is the
/// world after a reconnect.
pub fn request_extras(
    overlays: Res<Overlays>,
    wanted: Option<Res<SdfWanted>>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut state: ResMut<SdfExtras>,
    mut out: MessageWriter<GzCommand>,
) {
    let Some(status) = status else { return };
    if status.status != Status::Ready {
        if state.asked.is_some() || state.pending.is_some() {
            state.asked = None;
            state.pending = None;
        }
        return;
    }
    let needed = overlays.need_sdf() || wanted.is_some_and(|w| w.any());
    if !needed || state.pending.is_some() {
        return;
    }
    let (Some(world), Some(index)) = (status.world.as_deref(), index) else {
        return;
    };
    if index.scene.is_none() {
        return;
    }
    if state
        .asked
        .as_ref()
        .is_some_and(|(w, names)| w == world && *names == index.model_names)
    {
        return;
    }
    let service = sdf::generate_service(world);
    out.write(GzCommand::Request {
        service: service.clone(),
        msg_type: "gz.msgs.SdfGeneratorConfig".into(),
        payload: sdf::generate_request(),
    });
    state.pending = Some(service);
    state.asked = Some((world.to_owned(), index.model_names.clone()));
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
