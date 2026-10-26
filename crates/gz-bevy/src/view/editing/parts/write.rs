//! Done writes the draft into its model (spec §6.6; DECISIONS.md D-114).
//! It asks for the world's SDFormat, cuts the model out
//! (`world_file::model_sdf`), writes the parts into it
//! (`parts::write_parts`) and checks it (`parts::check_model`). Then the
//! model is removed and, once it has gone, created again from the new
//! SDFormat at its pose ([`Replacing`]), and editing ends. The whole
//! session is one edit on the undo stack ("edit amr"): undo puts the model
//! back as it was the same way. A model that can't be written (it fails
//! the check, the server doesn't answer, doesn't take edits or cuts long
//! messages, or its SDFormat is longer than the server takes) stays in
//! editing, saying why.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::parts::{check_model, write_parts};
use gz_core::sdf::{generate_request, generate_service};
use gz_core::world_file::model_sdf;
use prost::Message as _;

use super::{EditNote, EditParts, WriteParts};
use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::edit::{create_copy_request, edits_allowed, remove_request, too_long};
use crate::spawn::GzEntity;
use crate::undo::{Change, ModelCopy, Record, Replacement, Replacing};
use crate::view::editing::EditCommand;
use crate::view::model_edits::EDIT_TIMEOUT_SECS;

/// "3 changes", "1 change" (a reparent is one, not a part).
fn changes_count(n: usize) -> String {
    if n == 1 {
        "1 change".into()
    } else {
        format!("{n} changes")
    }
}

/// A [`WriteParts`] asks for the world's SDFormat, once (Done again while
/// it is on its way does nothing more).
pub fn start_writes(
    mut asks: MessageReader<WriteParts>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    entities: Query<&GzEntity>,
    mut parts: ResMut<EditParts>,
    mut out: MessageWriter<GzCommand>,
    mut notes: MessageWriter<EditNote>,
) {
    if asks.read().count() == 0 || parts.writing.is_some() || !parts.holds() {
        return;
    }
    let Some((model, _)) = parts.model.as_ref() else {
        return;
    };
    let name = entities
        .get(*model)
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let world = status.as_deref().and_then(|s| s.world.clone());
    let Some(world) = world.filter(|_| edits_allowed(status.as_deref())) else {
        notes.write(EditNote(format!(
            "Can't write {name}: the server doesn't take edits · Discard the parts, or undo them"
        )));
        return;
    };
    // Write-back removes the model and creates it from its SDFormat, which
    // a server that cuts long messages loses (the remove goes through).
    if !status
        .as_deref()
        .is_some_and(|s| s.long_frames == Some(true))
    {
        notes.write(EditNote(format!(
            "Can't write {name}: this server cuts long messages, so it would lose the model \
             · Discard the parts"
        )));
        return;
    }
    out.write(GzCommand::Request {
        service: generate_service(&world),
        msg_type: "gz.msgs.SdfGeneratorConfig".into(),
        payload: generate_request(),
    });
    parts.writing = Some(time.elapsed_secs_f64());
    notes.write(EditNote(format!("Writing {name}…")));
}

/// What writing reads of the world.
#[derive(bevy::ecs::system::SystemParam)]
pub struct WriteWorld<'w, 's> {
    time: Res<'w, Time<Real>>,
    status: Option<Res<'w, GzStatus>>,
    models: Query<'w, 's, (&'static GzEntity, &'static Transform)>,
    loaded: Option<Res<'w, crate::world_save::LoadedPoses>>,
}

/// The world's SDFormat finishes a write: the model with its parts, if it
/// checks out and the server can take it, replaces the model as one edit,
/// and editing ends. Otherwise editing goes on, saying why. One with no
/// reply in [`EDIT_TIMEOUT_SECS`] fails.
#[allow(clippy::too_many_arguments)]
pub fn finish_writes(
    mut replies: MessageReader<GzServiceReply>,
    world: WriteWorld,
    mut parts: ResMut<EditParts>,
    mut replacing: ResMut<Replacing>,
    mut out: MessageWriter<GzCommand>,
    mut records: MessageWriter<Record>,
    mut edits: MessageWriter<EditCommand>,
    mut notes: MessageWriter<EditNote>,
) {
    let Some(asked) = parts.writing else {
        replies.clear();
        return;
    };
    let Some((model, path)) = parts.model.clone() else {
        return;
    };
    let Ok((g, pose)) = world.models.get(model) else {
        return;
    };
    let name = g.name.clone();
    let status = world.status.as_deref();
    let Some(world_name) = status.and_then(|s| s.world.clone()) else {
        return;
    };
    let service = generate_service(&world_name);
    let Some(reply) = replies.read().filter(|r| r.service == service).last() else {
        if world.time.elapsed_secs_f64() - asked > EDIT_TIMEOUT_SECS {
            parts.writing = None;
            notes.write(EditNote(format!(
                "Can't write {name}: the server didn't answer"
            )));
        }
        return;
    };
    parts.writing = None;
    let xml = msgs::StringMsg::decode(reply.payload.as_slice())
        .map(|m| m.data)
        .unwrap_or_default();
    let loaded = world
        .loaded
        .as_deref()
        .and_then(|l| l.0.get(&path).copied());
    let copy = |sdf: String| ModelCopy {
        name: name.clone(),
        sdf,
        pose: *pose,
        loaded,
    };
    let written = model_sdf(&xml, &name).and_then(|before| {
        let after = write_parts(&before, &path, parts.draft.parts())?;
        check_model(&after)?;
        let create = create_copy_request(&world_name, &after, &name, pose);
        if let Some(why) = too_long(&create, status) {
            return Err(format!(
                "{why} · Discard the parts, or undo some (the server takes models that size \
                 only)"
            ));
        }
        Ok((before, after))
    });
    let (before, after) = match written {
        Ok(w) => w,
        Err(why) => {
            notes.write(EditNote(format!("Can't write {name}: {why}")));
            return;
        }
    };
    out.write(remove_request(&world_name, &name, Some(g.id)));
    let n = parts.draft.parts().len();
    // Written once its new self shows ("Writing amr…" until then).
    replacing.0.push(Replacement {
        done: Some(format!("Wrote {name}: {}", changes_count(n))),
        ..Replacement::new(copy(after.clone()), world.time.elapsed_secs_f64())
    });
    records.write(Record {
        verb: "edit",
        named: true,
        changes: vec![Change::Replaced {
            before: copy(before),
            after: copy(after),
        }],
        scrub: None,
        open: false,
    });
    parts.written = true;
    edits.write(EditCommand::Done);
}
