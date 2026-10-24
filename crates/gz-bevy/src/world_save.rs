//! Saving the world (spec §7.1; DECISIONS.md D-108): a [`SaveWorld`] asks
//! the server for its SDFormat (`generate_world_sdf`, included models
//! expanded) and saves it ([`save_text`]: the browser downloads it,
//! natively it goes to the working directory). Save writes the authored
//! world: each top-level model where the user last placed it (an edit on
//! the undo stack), else where it was when this client loaded the world,
//! not where physics has taken it since; "Use current poses" keeps the
//! server's. A save that marks the world saved clears its dirty state.
//! [`WorldSaved`] says how it went.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gz_core::msgs;
use gz_core::scene::Pose;
use gz_core::sdf::{generate_request, generate_service};
use gz_core::transport::Status;
use gz_core::world_file::with_model_poses;
use prost::Message as _;

use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::save::save_text;
use crate::spawn::{GzEntity, GzIndex, GzWorldRoot};
use crate::undo::UndoStack;

/// How long a save waits for the server's SDFormat.
pub const SAVE_TIMEOUT_SECS: f64 = 15.0;

/// Save the world as `name`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct SaveWorld {
    pub name: String,
    /// The server's poses, where physics has them (Save as… "Use current
    /// poses", Export world as SDF…), not the authored ones.
    pub current_poses: bool,
    /// The world counts as saved after (Save, Save as…; not Export).
    pub mark_saved: bool,
}

/// How a save went: where the file went, or why it didn't; and whether
/// the world now counts as saved.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct WorldSaved {
    pub result: Result<String, String>,
    pub marked: bool,
}

impl WorldSaved {
    fn failed(why: &str) -> Self {
        Self {
            result: Err(why.into()),
            marked: false,
        }
    }
}

/// Each top-level model's pose when this client loaded the world, by
/// scene path: kept through a reconnect to the same world (its scene puts
/// models where physics has them), forgotten for another server or world.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct LoadedPoses(pub HashMap<String, Transform>);

/// The saves waiting for the server's SDFormat, with when they asked.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct SaveJobs(Vec<(SaveWorld, f64)>);

/// What [`note_loaded_poses`] last saw: the world, the scenes that had
/// come, and the top-level models' paths by entity.
#[derive(Default)]
struct SeenModels {
    world: Option<(String, String)>,
    scenes: u32,
    paths: HashMap<Entity, String>,
}

/// Notes each top-level model's pose as it first spawns in this world. A
/// model deleted (not respawned with a new scene) is forgotten, so a new
/// one under its name starts from its own pose.
fn note_loaded_poses(
    added: Query<(Entity, &GzEntity, &Transform, &ChildOf), Added<GzEntity>>,
    roots: Query<(), With<GzWorldRoot>>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut gone: RemovedComponents<GzEntity>,
    mut loaded: ResMut<LoadedPoses>,
    mut seen: Local<SeenModels>,
) {
    let scenes = index.as_deref().map_or(0, |i| i.scenes);
    let respawned = scenes != seen.scenes;
    seen.scenes = scenes;
    for e in gone.read() {
        if let Some(path) = seen.paths.remove(&e) {
            if !respawned {
                loaded.0.remove(&path);
            }
        }
    }
    let seen_world = &mut seen.world;
    let now = status
        .as_deref()
        .and_then(|s| Some((s.url.clone(), s.world.clone()?)));
    // A connection on its way (no world yet) isn't another world.
    if now.is_some() && *seen_world != now {
        if seen_world.is_some() {
            loaded.0.clear();
        }
        *seen_world = now;
    }
    for (e, g, t, parent) in &added {
        if roots.contains(parent.parent()) {
            loaded.0.entry(g.path.clone()).or_insert(*t);
            seen.paths.insert(e, g.path.clone());
        }
    }
}

/// A [`SaveWorld`] asks for the world's SDFormat; with no live world it
/// fails at once.
fn request_saves(
    mut saves: MessageReader<SaveWorld>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    mut jobs: ResMut<SaveJobs>,
    mut out: MessageWriter<GzCommand>,
    mut done: MessageWriter<WorldSaved>,
) {
    let world = status
        .as_deref()
        .filter(|s| s.status == Status::Ready)
        .and_then(|s| s.world.clone());
    for save in saves.read() {
        let Some(world) = world.as_deref() else {
            done.write(WorldSaved::failed("Not connected"));
            continue;
        };
        out.write(GzCommand::Request {
            service: generate_service(world),
            msg_type: "gz.msgs.SdfGeneratorConfig".into(),
            payload: generate_request(),
        });
        jobs.0.push((save.clone(), time.elapsed_secs_f64()));
    }
}

/// The authored pose of each top-level model, by name: where the user
/// last placed it, else where it was loaded.
fn authored_poses(
    models: &Query<(&GzEntity, &ChildOf)>,
    roots: &Query<(), With<GzWorldRoot>>,
    undo: Option<&UndoStack>,
    loaded: &LoadedPoses,
) -> Vec<(String, Pose)> {
    models
        .iter()
        .filter(|(_, p)| roots.contains(p.parent()))
        .filter_map(|(g, _)| {
            let t = undo
                .and_then(|u| u.placed_pose(&g.path))
                .or_else(|| loaded.0.get(&g.path).copied())?;
            let (p, q) = (t.translation, t.rotation);
            let pose = Pose {
                pos: [f64::from(p.x), f64::from(p.y), f64::from(p.z)],
                quat: [
                    f64::from(q.x),
                    f64::from(q.y),
                    f64::from(q.z),
                    f64::from(q.w),
                ],
            };
            Some((g.name.clone(), pose))
        })
        .collect()
}

/// The server's SDFormat finishes the oldest save: its models put at their
/// authored poses (unless it keeps the current ones), then saved. A save
/// with no reply in [`SAVE_TIMEOUT_SECS`] fails.
#[allow(clippy::too_many_arguments)]
fn finish_saves(
    mut replies: MessageReader<GzServiceReply>,
    time: Res<Time<Real>>,
    status: Option<Res<GzStatus>>,
    models: Query<(&GzEntity, &ChildOf)>,
    roots: Query<(), With<GzWorldRoot>>,
    loaded: Res<LoadedPoses>,
    mut undo: Option<ResMut<UndoStack>>,
    mut jobs: ResMut<SaveJobs>,
    mut done: MessageWriter<WorldSaved>,
) {
    let service = status
        .as_deref()
        .and_then(|s| s.world.as_deref())
        .map(generate_service);
    for reply in replies.read() {
        if Some(&reply.service) != service.as_ref() || jobs.0.is_empty() {
            continue;
        }
        let (save, _) = jobs.0.remove(0);
        let sdf = msgs::StringMsg::decode(reply.payload.as_slice())
            .map(|m| m.data)
            .map_err(|e| e.to_string())
            .and_then(|sdf| {
                if sdf.trim().is_empty() || sdf == "service_not_found" {
                    Err("The server didn't send the world".to_string())
                } else {
                    Ok(sdf)
                }
            });
        let text = sdf.and_then(|sdf| {
            if save.current_poses {
                Ok(sdf)
            } else {
                let poses = authored_poses(&models, &roots, undo.as_deref(), &loaded);
                let named: Vec<(&str, Pose)> =
                    poses.iter().map(|(n, p)| (n.as_str(), *p)).collect();
                with_model_poses(&sdf, &named)
            }
        });
        let result = text.and_then(|t| save_text(&save.name, "application/xml", &t));
        let marked = result.is_ok() && save.mark_saved;
        if marked {
            if let Some(undo) = undo.as_deref_mut() {
                undo.mark_saved();
            }
        }
        done.write(WorldSaved { result, marked });
    }
    let now = time.elapsed_secs_f64();
    let late = jobs
        .0
        .iter()
        .filter(|(_, at)| now - at > SAVE_TIMEOUT_SECS)
        .count();
    if late > 0 {
        jobs.0.retain(|(_, at)| now - at <= SAVE_TIMEOUT_SECS);
        for _ in 0..late {
            done.write(WorldSaved::failed("The server didn't answer"));
        }
    }
}

pub struct WorldSavePlugin;

impl Plugin for WorldSavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveWorld>()
            .add_message::<WorldSaved>()
            .add_message::<GzCommand>()
            .add_message::<GzServiceReply>()
            .init_resource::<LoadedPoses>()
            .init_resource::<SaveJobs>()
            .add_systems(
                Update,
                (note_loaded_poses, request_saves, finish_saves).chain(),
            );
    }
}

#[cfg(test)]
mod tests;
