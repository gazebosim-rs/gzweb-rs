//! Editing a model in place (spec §6.6, decision Q6; DECISIONS.md D-112).
//! Entering (Edit model in the inspector or the context menu, or a double
//! click on a model with Translate or Rotate) pauses the sim if it runs,
//! turns every view's joints and frames on, and keeps the model as
//! [`EditingModel`]: its links are what views pick and what the
//! selection holds, and every other entity renders at
//! [`DIMMED_OPACITY`]. Done turns off the overlays entering turned on
//! and resumes the sim if entering paused it. The UI draws the EDITING pill and limits the Scene
//! tree; adding parts and writing the model back come later.

use std::collections::HashMap;

use bevy::ecs::message::MessageCursor;
use bevy::prelude::*;

use super::Selection;
use crate::connection::{GzCommand, GzStatus};
use crate::heightmap::TerrainMaterial;
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

    /// `view`'s overlays without those editing turned on: what a
    /// workspace saves of it.
    pub fn own(&self, view: Entity, set: &OverlaySet) -> OverlaySet {
        let mut out = *set;
        for o in self.lit(view) {
            out.set(o, false);
        }
        out
    }

    /// Forget what editing turned on in `view`: a workspace has just set
    /// its overlays. Editing turns joints and frames on there again, and
    /// Done turns off only those it did.
    pub fn relight(&mut self, view: Entity) {
        if let Some(e) = self.0.as_mut() {
            e.lit.retain(|(v, _)| *v != view);
        }
    }

    /// The overlays editing turned on in `view`.
    fn lit(&self, view: Entity) -> impl Iterator<Item = Overlay> + '_ {
        self.0
            .iter()
            .flat_map(|e| &e.lit)
            .filter(move |(v, _)| *v == view)
            .flat_map(|(_, os)| os.iter().copied())
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
    /// The overlays editing turned on in each view (joints and frames, if
    /// they were off), turned off on Done.
    lit: Vec<(Entity, Vec<Overlay>)>,
}

/// What editing turns on in every view (§6.6).
const EDIT_OVERLAYS: [Overlay; 2] = [Overlay::Joints, Overlay::Frames];

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

/// Runs the [`EditCommand`]s. Entering needs a top-level model; it pauses
/// a running sim, turns the views' joints and frames on and clears the
/// selection. Entering another while one is edited moves to it, the sim
/// as it is. Done undoes them.
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
                *selection = Selection::default();
                // From one model to another: the sim stays paused, the
                // views' overlays stay on, and Done resumes as the first
                // would have.
                if let Some(e) = editing.0.as_mut() {
                    e.model = model;
                    continue;
                }
                let sim = status.as_ref().and_then(|s| s.sim.as_ref());
                let running = sim.is_some_and(|s| !s.paused);
                if running {
                    gz.write(GzCommand::Pause);
                }
                editing.0 = Some(Editing {
                    model,
                    resume: running,
                    undecided: sim.is_none(),
                    lit: Vec::new(),
                });
                light_views(&mut editing, &mut sets);
            }
            EditCommand::Done => {
                if let Some(was) = editing.0.take() {
                    leave(was, &mut sets, &mut gz);
                }
            }
        }
    }
}

/// Puts back what entering changed: the overlays it turned on, and a sim
/// it paused.
fn leave(was: Editing, sets: &mut ViewSets, gz: &mut MessageWriter<GzCommand>) {
    let resume = was.resume;
    unlight(was, sets);
    if resume {
        gz.write(GzCommand::Play);
    }
}

/// Turns off the overlays editing turned on (those still on: one the
/// user turned off stays off; the others the user turned on stay on).
fn unlight(was: Editing, sets: &mut ViewSets) {
    for (view, lit) in was.lit {
        if let Ok((_, mut set)) = sets.get_mut(view) {
            for o in lit {
                if set.is_on(o) {
                    set.set(o, false);
                }
            }
        }
    }
}

/// Turns joints and frames on in each view editing hasn't yet (entering,
/// a view that comes while editing, one a workspace just set), keeping
/// which it turned on.
fn light_views(editing: &mut ResMut<EditingModel>, sets: &mut ViewSets) {
    let known = |editing: &EditingModel, v: Entity| {
        editing
            .0
            .as_ref()
            .is_some_and(|e| e.lit.iter().any(|(l, _)| *l == v))
    };
    for (view, mut set) in sets.iter_mut() {
        if editing.0.is_none() || known(editing, view) {
            continue;
        }
        let off: Vec<Overlay> = EDIT_OVERLAYS
            .into_iter()
            .filter(|o| !set.is_on(*o))
            .collect();
        for o in &off {
            set.set(*o, true);
        }
        if let Some(e) = editing.0.as_mut() {
            e.lit.push((view, off));
        }
    }
}

/// While a model is edited, every view shows its joints and frames: one
/// that comes, or that a workspace sets, gets them too.
pub fn light_new_views(mut editing: ResMut<EditingModel>, mut sets: ViewSets) {
    let Some(e) = editing.0.as_ref() else {
        return;
    };
    if sets.iter().all(|(v, _)| e.lit.iter().any(|(l, _)| *l == v)) {
        return;
    }
    light_views(&mut editing, &mut sets);
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
    // Read first: a write marks the editing changed, and this runs every
    // frame.
    if !editing.0.as_ref().is_some_and(|e| e.undecided) {
        return;
    }
    let Some(e) = editing.0.as_mut() else {
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
        unlight(was, &mut sets);
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

/// A material [`dim_others`] can fade.
pub trait Fade: Material + Clone {
    /// Makes it `opacity` as opaque.
    fn fade(&mut self, opacity: f32);
}

impl Fade for StandardMaterial {
    fn fade(&mut self, opacity: f32) {
        crate::material::set_opacity(self, opacity);
    }
}

impl Fade for TerrainMaterial {
    fn fade(&mut self, opacity: f32) {
        // The layers blend over layer 0's colour and keep its alpha.
        crate::material::set_opacity(&mut self.base, opacity);
    }
}

/// A mesh drawn faded while a model is edited: its own material, and the
/// faded one it shows.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Dimmed<M: Material = StandardMaterial> {
    pub original: Handle<M>,
    pub faded: Handle<M>,
}

/// The scene's meshes, their material and whether they're dimmed.
type SceneMeshes<'w, 's, M> = Query<
    'w,
    's,
    (
        Entity,
        Ref<'static, MeshMaterial3d<M>>,
        Option<&'static Dimmed<M>>,
    ),
    Without<bevy::camera::visibility::RenderLayers>,
>;

/// Each material faded while editing (by its own), and the materials'
/// changes not yet read.
pub struct Faded<M: Material> {
    copies: HashMap<AssetId<M>, Handle<M>>,
    changes: MessageCursor<AssetEvent<M>>,
}

impl<M: Material> Default for Faded<M> {
    fn default() -> Self {
        Self {
            copies: HashMap::new(),
            changes: MessageCursor::default(),
        }
    }
}

/// The materials, and their changes.
type MaterialsOf<'w, M> = (
    Option<ResMut<'w, Assets<M>>>,
    Option<Res<'w, Messages<AssetEvent<M>>>>,
);

/// While a model is edited every other visual's mesh shows a faded copy
/// of its material ([`DIMMED_OPACITY`]; each material faded once, never
/// in place, and faded again when it changes or loads); leaving puts the
/// materials back. A mesh that comes, or gets a new material, while
/// editing is faded too. For [`StandardMaterial`] and the heightmaps'
/// [`TerrainMaterial`].
pub fn dim_others<M: Fade>(
    editing: Res<EditingModel>,
    meshes: SceneMeshes<M>,
    parents: Query<&ChildOf>,
    visuals: Query<(), With<GzVisual>>,
    (materials, changes): MaterialsOf<M>,
    mut faded: Local<Faded<M>>,
    mut commands: Commands,
) {
    // No renderer (a headless app): nothing to fade.
    let Some(mut materials) = materials else {
        return;
    };
    let faded = &mut *faded;
    // A faded material's own that changed (a texture combined into it,
    // say) or came: its copy follows.
    if let Some(changes) = changes {
        let changed: Vec<AssetId<M>> = faded
            .changes
            .read(&changes)
            .filter_map(|e| match e {
                AssetEvent::Added { id }
                | AssetEvent::Modified { id }
                | AssetEvent::LoadedWithDependencies { id } => Some(*id),
                _ => None,
            })
            .filter(|id| faded.copies.contains_key(id))
            .collect();
        for id in changed {
            let Some(mut m) = materials.get(id).cloned() else {
                continue;
            };
            m.fade(DIMMED_OPACITY);
            if let Some(copy) = faded.copies.get(&id) {
                let _ = materials.insert(copy, m);
            }
        }
    }
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
        faded.copies.clear();
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
                    .copies
                    .entry(current.id())
                    .or_insert_with(|| {
                        // Not loaded yet: faded as it loads (above).
                        let mut m = materials.get(&current).cloned();
                        if let Some(m) = m.as_mut() {
                            m.fade(DIMMED_OPACITY);
                        }
                        match m {
                            Some(m) => materials.add(m),
                            None => materials.reserve_handle(),
                        }
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
fn restore<M: Material>(commands: &mut Commands, e: Entity, current: &Handle<M>, d: &Dimmed<M>) {
    let mut c = commands.entity(e);
    if *current == d.faded {
        c.insert(MeshMaterial3d(d.original.clone()));
    }
    c.remove::<Dimmed<M>>();
}
