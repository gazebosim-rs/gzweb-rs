//! Copy, Paste, Duplicate and Delete of the selected top-level models
//! (spec §7.9, §7.1; DECISIONS.md D-109). Each asks the server for the
//! world's SDFormat (`generate_world_sdf`) first: a copy is created from
//! its model's element (`create`, named by the client so undo can remove
//! it), and a model deleted (`remove`) can be created again by undo. A
//! copy goes beside its model, along the world's Y past its bounds, and
//! is selected once it appears. Spawns and deletes are edits on the undo
//! stack.

use bevy::prelude::*;
use gz_core::msgs;
use gz_core::sdf::{generate_request, generate_service};
use gz_core::transport::Status;
use gz_core::world_file::model_sdf;
use prost::Message as _;

use super::align::gz_bounds;
use super::select::{select, SelectMode};
use super::ViewScene;
use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::edit::{create_copy_request, edits_allowed, remove_request};
use crate::spawn::{GzEntity, GzIndex, GzWorldRoot};
use crate::undo::{Change, ModelCopy, Record};
use gz_core::scene::NodeKind;

/// How long a copy or delete waits for the server's SDFormat.
pub const EDIT_TIMEOUT_SECS: f64 = 15.0;

/// The gap between a model and its copy (m).
const GAP: f32 = 0.5;

/// How long a copy waits to show before it isn't selected.
const SELECT_SECS: f64 = 10.0;

/// What to do with the selection's models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelEdit {
    /// Onto the client's clipboard ([`ModelClipboard`]).
    Copy,
    /// The clipboard's models, beside where they were copied from.
    Paste,
    Duplicate,
    Delete,
}

impl ModelEdit {
    /// The verb its edit and its note use.
    fn verb(self) -> &'static str {
        match self {
            ModelEdit::Copy => "copy",
            ModelEdit::Paste => "paste",
            ModelEdit::Duplicate => "duplicate",
            ModelEdit::Delete => "delete",
        }
    }

    /// What its note says it did.
    fn done(self) -> &'static str {
        match self {
            ModelEdit::Copy => "Copied",
            ModelEdit::Paste => "Pasted",
            ModelEdit::Duplicate => "Duplicated",
            ModelEdit::Delete => "Deleted",
        }
    }

    /// Whether it changes the world (and so needs a server that takes
    /// edits).
    pub fn edits(self) -> bool {
        self != ModelEdit::Copy
    }
}

/// Copy, Paste, Duplicate or Delete the selection's top-level models.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditModels(pub ModelEdit);

/// How an [`EditModels`] went: what it did ("Deleted cart"), or why it
/// couldn't.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct ModelsEdited {
    pub edit: ModelEdit,
    pub result: Result<String, String>,
}

/// A model as an edit takes it: its entity and id, its name, its pose in
/// the world and as loaded (for Save, D-108), and its bounds' size along
/// the world's axes.
#[derive(Clone, Debug, PartialEq)]
struct Taken {
    entity: Entity,
    id: u32,
    name: String,
    pose: Transform,
    loaded: Option<Transform>,
    size: Vec3,
}

/// The models copied, with their SDFormat and size; and how many times
/// they were pasted (each paste goes one step further along).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ModelClipboard {
    models: Vec<(ModelCopy, Vec3)>,
    pastes: u32,
}

impl ModelClipboard {
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

/// The edits waiting for the server's SDFormat, with when they asked; and
/// the models a delete is under way for (waiting, or removed and not yet
/// gone), which another Delete leaves alone.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct EditJobs {
    waiting: Vec<(ModelEdit, Vec<Taken>, f64)>,
    deleting: Vec<(Entity, f64)>,
}

/// The copies asked for, to select once they appear, until a time; the
/// first to appear replaces the selection, the rest join it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SelectWhenSpawned {
    names: Vec<String>,
    until: f64,
    joined: bool,
}

/// "cart", "2 models".
fn names(models: &[String]) -> String {
    match models {
        [one] => one.clone(),
        many => format!("{} models", many.len()),
    }
}

/// `name` without a copy's `_N`.
fn base_name(name: &str) -> &str {
    match name.rsplit_once('_') {
        Some((base, n)) if !base.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => base,
        _ => name,
    }
}

/// The first `<base>_N` (from 1) neither the scene nor `chosen` has.
fn free_name(name: &str, index: Option<&GzIndex>, chosen: &[String]) -> String {
    let base = base_name(name);
    (1..)
        .map(|n| format!("{base}_{n}"))
        .find(|n| !index.is_some_and(|i| i.by_name.contains_key(n)) && !chosen.contains(n))
        .expect("a free name")
}

/// The live world's name, on a ready connection.
fn live_world(status: Option<&GzStatus>) -> Option<String> {
    status
        .filter(|s| s.status == Status::Ready)
        .and_then(|s| s.world.clone())
}

/// Creates `models` (copies) `steps` along, each beside its model, as one
/// `verb` edit; they're selected once they appear.
#[allow(clippy::too_many_arguments)]
fn spawn_copies(
    world: &str,
    edit: ModelEdit,
    models: &[(ModelCopy, Vec3)],
    steps: u32,
    index: Option<&GzIndex>,
    now: f64,
    out: &mut MessageWriter<GzCommand>,
    records: &mut MessageWriter<Record>,
    pending: &mut SelectWhenSpawned,
) -> String {
    // Copies still on their way have their names.
    let waiting = if now <= pending.until {
        pending.names.clone()
    } else {
        vec![]
    };
    let mut chosen: Vec<String> = vec![];
    let mut changes = vec![];
    for (model, size) in models {
        let taken: Vec<String> = waiting.iter().chain(&chosen).cloned().collect();
        let name = free_name(&model.name, index, &taken);
        let mut pose = model.pose;
        pose.translation.y += steps as f32 * (size.y + GAP);
        out.write(create_copy_request(world, &model.sdf, &name, &pose));
        changes.push(Change::Exists {
            model: ModelCopy {
                name: name.clone(),
                sdf: model.sdf.clone(),
                pose,
                loaded: None,
            },
            after: true,
        });
        chosen.push(name);
    }
    records.write(Record {
        verb: edit.verb(),
        named: true,
        changes,
        scrub: None,
        open: false,
    });
    let label = format!("{} {}", edit.done(), names(&chosen));
    *pending = SelectWhenSpawned {
        names: waiting.into_iter().chain(chosen.iter().cloned()).collect(),
        until: now + SELECT_SECS,
        joined: false,
    };
    label
}

/// The parts of the world the edits read.
type Models<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GzEntity,
        &'static ChildOf,
        &'static Transform,
    ),
>;

/// An [`EditModels`] takes the selection's top-level models and asks for
/// the world's SDFormat; a Paste creates the clipboard's copies at once.
#[allow(clippy::too_many_arguments)]
pub(crate) fn request_model_edits(
    mut asks: MessageReader<EditModels>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    scene: ViewScene,
    roots: Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
    models: Models,
    loaded: Option<Res<crate::world_save::LoadedPoses>>,
    mut clipboard: ResMut<ModelClipboard>,
    mut jobs: ResMut<EditJobs>,
    mut pending: ResMut<SelectWhenSpawned>,
    mut out: MessageWriter<GzCommand>,
    mut records: MessageWriter<Record>,
    mut done: MessageWriter<ModelsEdited>,
) {
    let now = time.elapsed_secs_f64();
    // A delete's models are gone, or its remove went unheeded.
    if !jobs.deleting.is_empty() {
        jobs.deleting
            .retain(|(e, at)| models.contains(*e) && now - at < 2.0 * EDIT_TIMEOUT_SECS);
    }
    for EditModels(edit) in asks.read() {
        let fail = |why: &str| ModelsEdited {
            edit: *edit,
            result: Err(why.to_string()),
        };
        let Some(world) = live_world(status.as_deref()) else {
            done.write(fail("Not connected"));
            continue;
        };
        if edit.edits() && !edits_allowed(status.as_deref()) {
            done.write(fail("This server doesn't take edits"));
            continue;
        }
        if *edit == ModelEdit::Paste {
            if clipboard.is_empty() {
                done.write(fail("Nothing copied"));
                continue;
            }
            clipboard.pastes += 1;
            let label = spawn_copies(
                &world,
                *edit,
                &clipboard.models,
                clipboard.pastes,
                index.as_deref(),
                now,
                &mut out,
                &mut records,
                &mut pending,
            );
            done.write(ModelsEdited {
                edit: *edit,
                result: Ok(label),
            });
            continue;
        }
        let Ok((root, root_at)) = roots.single() else {
            continue;
        };
        let to_root = root_at.affine().inverse();
        let deleting = |e: &Entity| jobs.deleting.iter().any(|(d, _)| d == e);
        let mut picked = 0;
        let taken: Vec<Taken> = scene
            .selection
            .entities
            .iter()
            .filter_map(|e| {
                let (_, g, parent, t) = models.get(*e).ok()?;
                if parent.parent() != root || g.kind != NodeKind::Model {
                    return None;
                }
                picked += 1;
                if *edit == ModelEdit::Delete && deleting(e) {
                    return None;
                }
                let b = gz_bounds(&scene, &to_root, *e)?;
                Some(Taken {
                    entity: *e,
                    id: g.id,
                    name: g.name.clone(),
                    pose: *t,
                    loaded: loaded.as_deref().and_then(|l| l.0.get(&g.path).copied()),
                    size: b.max - b.min,
                })
            })
            .collect();
        if picked == 0 {
            done.write(fail("Select a model"));
            continue;
        }
        // Already on their way out (Delete pressed again).
        if taken.is_empty() {
            continue;
        }
        if *edit == ModelEdit::Delete {
            jobs.deleting.extend(taken.iter().map(|t| (t.entity, now)));
        }
        out.write(GzCommand::Request {
            service: generate_service(&world),
            msg_type: "gz.msgs.SdfGeneratorConfig".into(),
            payload: generate_request(),
        });
        jobs.waiting.push((*edit, taken, now));
    }
}

/// The server's SDFormat finishes the oldest edit: Copy fills the
/// clipboard, Duplicate creates the copies, Delete removes the models
/// (keeping their SDFormat for undo). One with no reply in
/// [`EDIT_TIMEOUT_SECS`] fails.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_model_edits(
    mut replies: MessageReader<GzServiceReply>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut clipboard: ResMut<ModelClipboard>,
    mut jobs: ResMut<EditJobs>,
    mut pending: ResMut<SelectWhenSpawned>,
    mut out: MessageWriter<GzCommand>,
    mut records: MessageWriter<Record>,
    mut done: MessageWriter<ModelsEdited>,
) {
    let now = time.elapsed_secs_f64();
    let world = status.as_deref().and_then(|s| s.world.clone());
    let service = world.as_deref().map(generate_service);
    for reply in replies.read() {
        if Some(&reply.service) != service.as_ref() || jobs.waiting.is_empty() {
            continue;
        }
        let world = world.as_deref().unwrap_or_default();
        let (edit, taken, _) = jobs.waiting.remove(0);
        let sdf = msgs::StringMsg::decode(reply.payload.as_slice())
            .map(|m| m.data)
            .unwrap_or_default();
        let copies: Result<Vec<(ModelCopy, Vec3)>, String> = taken
            .iter()
            .map(|t| {
                Ok((
                    ModelCopy {
                        name: t.name.clone(),
                        sdf: model_sdf(&sdf, &t.name)?,
                        pose: t.pose,
                        loaded: t.loaded,
                    },
                    t.size,
                ))
            })
            .collect();
        let result = copies.map(|copies| match edit {
            ModelEdit::Copy => {
                let named: Vec<String> = copies.iter().map(|(m, _)| m.name.clone()).collect();
                *clipboard = ModelClipboard {
                    models: copies,
                    pastes: 0,
                };
                format!("{} {}", edit.done(), names(&named))
            }
            ModelEdit::Duplicate | ModelEdit::Paste => spawn_copies(
                world,
                edit,
                &copies,
                1,
                index.as_deref(),
                now,
                &mut out,
                &mut records,
                &mut pending,
            ),
            ModelEdit::Delete => {
                let named: Vec<String> = copies.iter().map(|(m, _)| m.name.clone()).collect();
                for ((m, _), t) in copies.iter().zip(&taken) {
                    out.write(remove_request(world, &m.name, Some(t.id)));
                }
                records.write(Record {
                    verb: edit.verb(),
                    named: true,
                    changes: copies
                        .into_iter()
                        .map(|(model, _)| Change::Exists {
                            model,
                            after: false,
                        })
                        .collect(),
                    scrub: None,
                    open: false,
                });
                format!("{} {}", edit.done(), names(&named))
            }
        });
        // A delete that failed can be asked again.
        if edit == ModelEdit::Delete && result.is_err() {
            jobs.deleting
                .retain(|(e, _)| !taken.iter().any(|t| t.entity == *e));
        }
        done.write(ModelsEdited { edit, result });
    }
    let late: Vec<(ModelEdit, Vec<Entity>)> = jobs
        .waiting
        .iter()
        .filter(|(.., at)| now - at > EDIT_TIMEOUT_SECS)
        .map(|(e, taken, _)| (*e, taken.iter().map(|t| t.entity).collect()))
        .collect();
    if !late.is_empty() {
        jobs.waiting
            .retain(|(.., at)| now - at <= EDIT_TIMEOUT_SECS);
        for (edit, entities) in late {
            // A delete that never went can be asked again.
            jobs.deleting.retain(|(e, _)| !entities.contains(e));
            done.write(ModelsEdited {
                edit,
                result: Err("The server didn't answer".into()),
            });
        }
    }
}

/// Copies asked for are selected as they appear; a failed edit says why
/// in the log.
pub(crate) fn select_copies(
    time: Res<Time<Real>>,
    added: Query<(Entity, &GzEntity), Added<GzEntity>>,
    mut pending: ResMut<SelectWhenSpawned>,
    mut selection: ResMut<super::Selection>,
    mut edited: MessageReader<ModelsEdited>,
) {
    for e in edited.read() {
        if let Err(why) = &e.result {
            warn!("can't {}: {why}", e.edit.verb());
        }
    }
    if pending.names.is_empty() {
        return;
    }
    if time.elapsed_secs_f64() > pending.until {
        pending.names.clear();
        return;
    }
    let mut found = vec![];
    for (e, g) in &added {
        let named = pending.names.iter().position(|n| *n == g.name);
        if let Some(i) = named.filter(|_| g.kind == NodeKind::Model) {
            pending.names.remove(i);
            found.push(e);
        }
    }
    if !found.is_empty() {
        let mode = if pending.joined {
            SelectMode::Add
        } else {
            SelectMode::Replace
        };
        pending.joined = true;
        select(&mut selection.reborrow(), mode, &found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_take_the_next_free_name() {
        assert_eq!(base_name("cart_2"), "cart");
        assert_eq!(base_name("cart"), "cart");
        assert_eq!(base_name("_3"), "_3");
        assert_eq!(base_name("box_a"), "box_a");
        let mut index = GzIndex::default();
        index.by_name.insert("cart_1".into(), vec![]);
        assert_eq!(free_name("cart", Some(&index), &[]), "cart_2");
        assert_eq!(
            free_name("cart_1", Some(&index), &["cart_2".into()]),
            "cart_3"
        );
        assert_eq!(free_name("box", None, &[]), "box_1");
        assert_eq!(names(&["a".into()]), "a");
        assert_eq!(names(&["a".into(), "b".into()]), "2 models");
    }
}
