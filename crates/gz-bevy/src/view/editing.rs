//! Editing a model in place (spec §6.6, decision Q6; DECISIONS.md D-112).
//! Entering (Edit model in the inspector or the context menu, or a double
//! click on a model with Translate or Rotate) pauses the sim if it runs,
//! turns every view's joints and frames on, and keeps the model as
//! [`EditingModel`]: its links are what views pick and what the
//! selection holds, and every other entity renders at
//! [`DIMMED_OPACITY`]. Done puts the overlays back and resumes the sim if
//! entering paused it. The UI draws the EDITING pill and limits the Scene
//! tree; adding parts and writing the model back come later.

use std::collections::HashMap;

use bevy::prelude::*;

use super::Selection;
use crate::connection::{GzCommand, GzStatus};
use crate::overlays::{Overlay, OverlaySet};
use crate::spawn::{GzEntity, GzVisual, GzWorldRoot};

/// How opaque the other entities are while a model is edited (§6.6).
pub const DIMMED_OPACITY: f32 = 0.25;

/// The model being edited, if one is.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct EditingModel(pub Option<Editing>);

impl EditingModel {
    /// The model, while one is edited.
    pub fn model(&self) -> Option<Entity> {
        self.0.as_ref().map(|e| e.model)
    }
}

/// A model being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Editing {
    pub model: Entity,
    /// Entering paused the sim: Done resumes it.
    pub resume: bool,
    /// Whether the sim ran wasn't known yet (no stats): the first that
    /// come decide.
    pub undecided: bool,
    /// Each view's overlays before entering, put back on Done.
    overlays: Vec<(Entity, OverlaySet)>,
}

/// Enter editing a top-level model, or leave it.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    Enter(Entity),
    /// Leave editing (Done, or Esc with nothing selected in the model).
    Done,
}

/// `e` is `model` or below it.
pub fn is_under(parents: &Query<&ChildOf>, e: Entity, model: Entity) -> bool {
    e == model || parents.iter_ancestors(e).any(|a| a == model)
}

/// The child of `model` that `e` is part of (a link, or a nested model):
/// what a pick in the model picks. None outside it, and for the model.
pub fn part_of(parents: &Query<&ChildOf>, e: Entity, model: Entity) -> Option<Entity> {
    let mut at = e;
    while let Ok(p) = parents.get(at) {
        if p.parent() == model {
            return Some(at);
        }
        at = p.parent();
    }
    None
}

/// A view camera's overlays.
type ViewSets<'w, 's> = Query<'w, 's, (Entity, &'static mut OverlaySet)>;

/// Runs the [`EditCommand`]s. Entering needs a top-level model (another
/// model being edited is left first); it pauses a running sim, turns the
/// views' joints and frames on and clears the selection. Done undoes them.
#[allow(clippy::too_many_arguments)]
pub fn run_edit_commands(
    mut commands: MessageReader<EditCommand>,
    mut editing: ResMut<EditingModel>,
    status: Option<Res<GzStatus>>,
    models: Query<(&GzEntity, &ChildOf)>,
    roots: Query<(), With<GzWorldRoot>>,
    mut sets: ViewSets,
    mut selection: ResMut<Selection>,
    mut gz: MessageWriter<GzCommand>,
) {
    for cmd in commands.read() {
        match *cmd {
            EditCommand::Enter(model) => {
                let top = models.get(model).is_ok_and(|(g, p)| {
                    g.kind == gz_core::scene::NodeKind::Model && roots.contains(p.parent())
                });
                if !top || editing.model() == Some(model) {
                    continue;
                }
                if let Some(was) = editing.0.take() {
                    leave(was, &mut sets, &mut gz);
                }
                let sim = status.as_ref().and_then(|s| s.sim.as_ref());
                let running = sim.is_some_and(|s| !s.paused);
                if running {
                    gz.write(GzCommand::Pause);
                }
                let mut overlays = Vec::new();
                for (view, mut set) in &mut sets {
                    overlays.push((view, *set));
                    for o in [Overlay::Joints, Overlay::Frames] {
                        if !set.is_on(o) {
                            set.set(o, true);
                        }
                    }
                }
                *selection = Selection::default();
                editing.0 = Some(Editing {
                    model,
                    resume: running,
                    undecided: sim.is_none(),
                    overlays,
                });
            }
            EditCommand::Done => {
                if let Some(was) = editing.0.take() {
                    leave(was, &mut sets, &mut gz);
                }
            }
        }
    }
}

/// Puts back what entering changed: the views' overlays (the views it
/// knew), and a sim it paused.
fn leave(was: Editing, sets: &mut ViewSets, gz: &mut MessageWriter<GzCommand>) {
    for (view, before) in was.overlays {
        if let Ok((_, mut set)) = sets.get_mut(view) {
            set.set_if_neq(before);
        }
    }
    if was.resume {
        gz.write(GzCommand::Play);
    }
}

/// Entered before the sim's first stats (`gzweb --edit`): once they come,
/// a running sim pauses, and Done resumes it.
pub fn pause_when_known(
    mut editing: ResMut<EditingModel>,
    status: Option<Res<GzStatus>>,
    mut gz: MessageWriter<GzCommand>,
) {
    let Some(sim) = status.as_ref().and_then(|s| s.sim.as_ref()) else {
        return;
    };
    let Some(e) = editing.0.as_mut().filter(|e| e.undecided) else {
        return;
    };
    e.undecided = false;
    if !sim.paused {
        e.resume = true;
        gz.write(GzCommand::Pause);
    }
}

/// A model that goes (deleted, or a new scene) ends its editing; the sim
/// stays as it is.
pub fn end_editing_when_gone(
    mut editing: ResMut<EditingModel>,
    models: Query<(), With<GzEntity>>,
    mut sets: ViewSets,
) {
    let Some(model) = editing.model() else {
        return;
    };
    if models.contains(model) {
        return;
    }
    if let Some(was) = editing.0.take() {
        for (view, before) in was.overlays {
            if let Ok((_, mut set)) = sets.get_mut(view) {
                set.set_if_neq(before);
            }
        }
    }
}

/// While a model is edited the selection holds its parts only: its links
/// and nested models, and their joints and sensors (from a tree row, the
/// palette, wherever).
pub fn limit_selection(
    editing: Res<EditingModel>,
    parents: Query<&ChildOf>,
    mut selection: ResMut<Selection>,
) {
    let Some(model) = editing.model() else {
        return;
    };
    if !selection.is_changed() && !editing.is_changed() {
        return;
    }
    let inside = |e: Entity| e != model && is_under(&parents, e, model);
    let keep = selection.entities.iter().all(|e| inside(*e))
        && selection
            .parts
            .iter()
            .all(|p| is_under(&parents, p.owner, model));
    if keep {
        return;
    }
    let entities = selection
        .entities
        .iter()
        .copied()
        .filter(|e| inside(*e))
        .collect();
    let parts = selection
        .parts
        .iter()
        .filter(|p| is_under(&parents, p.owner, model))
        .cloned()
        .collect();
    *selection = Selection { entities, parts };
}

/// A mesh drawn faded while a model is edited: its own material, and the
/// faded one it shows.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Dimmed {
    pub original: Handle<StandardMaterial>,
    pub faded: Handle<StandardMaterial>,
}

/// The scene's meshes, their material and whether they're dimmed.
type SceneMeshes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Ref<'static, MeshMaterial3d<StandardMaterial>>,
        Option<&'static Dimmed>,
    ),
    Without<bevy::camera::visibility::RenderLayers>,
>;

/// While a model is edited every other visual's mesh shows a faded copy
/// of its material ([`DIMMED_OPACITY`]; each material faded once, never
/// in place); leaving puts the materials back. A mesh that comes, or
/// gets a new material, while editing is faded too.
pub fn dim_others(
    editing: Res<EditingModel>,
    meshes: SceneMeshes,
    parents: Query<&ChildOf>,
    visuals: Query<(), With<GzVisual>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut faded: Local<HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>>,
    mut commands: Commands,
) {
    // No renderer (a headless app): nothing to fade.
    let Some(mut materials) = materials else {
        return;
    };
    let model = editing.model();
    let all = editing.is_changed();
    if model.is_none() {
        if !all {
            return;
        }
        for (e, material, dimmed) in &meshes {
            if let Some(d) = dimmed {
                restore(&mut commands, e, &material.0, d);
            }
        }
        faded.clear();
        return;
    }
    let model = model.unwrap_or(Entity::PLACEHOLDER);
    for (e, material, dimmed) in &meshes {
        if !all && !material.is_changed() {
            continue;
        }
        // A visual's mesh: it, or something under it, is a visual.
        let visual = visuals.contains(e) || parents.iter_ancestors(e).any(|a| visuals.contains(a));
        if !visual {
            continue;
        }
        let inside = is_under(&parents, e, model);
        let current = material.0.clone();
        match dimmed {
            // In the model now: its own back.
            Some(d) if inside => restore(&mut commands, e, &current, d),
            // Ours, already.
            Some(d) if current == d.faded => {}
            // Its own, or a new one given while dimmed.
            _ if !inside => {
                let fade = faded
                    .entry(current.id())
                    .or_insert_with(|| {
                        let mut m = materials.get(&current).cloned().unwrap_or_default();
                        crate::material::set_opacity(&mut m, DIMMED_OPACITY);
                        materials.add(m)
                    })
                    .clone();
                commands.entity(e).insert((
                    MeshMaterial3d(fade.clone()),
                    Dimmed {
                        original: current,
                        faded: fade,
                    },
                ));
            }
            _ => {}
        }
    }
}

/// Puts a dimmed mesh's own material back, unless it was given another
/// since (which stays).
fn restore(commands: &mut Commands, e: Entity, current: &Handle<StandardMaterial>, d: &Dimmed) {
    let mut c = commands.entity(e);
    if *current == d.faded {
        c.insert(MeshMaterial3d(d.original.clone()));
    }
    c.remove::<Dimmed>();
}
