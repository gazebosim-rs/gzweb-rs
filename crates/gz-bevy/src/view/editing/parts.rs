//! The parts added to the model being edited (spec §6.6; DECISIONS.md
//! D-113): a [`PartCommand`] adds one to the [`EditParts`] draft, and
//! undo and redo take the last back or put it back while a model is
//! edited (the world's undo waits until Done). What a part shows:
//!
//! - a link, and a visual, as entities under the model or the link, built
//!   the way the scene message's are ([`DraftPart`]);
//! - a sensor in its link's [`GzSensors`];
//! - every part in the world's SDFormat extras the overlays, the scene
//!   tree and the inspectors read ([`SdfExtras`]): the server's, with the
//!   draft's parts added (a reply that comes while editing gets them
//!   too); leaving puts the server's back.
//!
//! Nothing reaches the server yet: leaving drops the draft (writing it back
//! is M9c's), saying how many parts went ([`PartsDiscarded`]).

use bevy::prelude::*;
use gz_core::parts::{shape_visual, Draft, Part, LINK_SIZE, PART_SIZE};
use gz_core::scene::{Node, NodeKind, Pose, VisualDesc};
use gz_core::sdf::WorldExtras;

use super::EditingModel;
use crate::overlays::extras::SdfExtras;
use crate::spawn::{pose_transform, GeometryAssets, GzEntity, GzSensors, GzVisual};
use crate::undo::UndoCommand;
use crate::view::{Part as SelectedPart, PartKind, Selection};

/// An entity spawned for a part of the draft (a link, a visual).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DraftPart;

/// Adds a part to the model being edited.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum PartCommand {
    Add(Part),
}

/// Leaving editing dropped this many parts (they aren't written back
/// yet).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartsDiscarded(pub usize);

/// The parts added to the model being edited.
#[derive(Resource, Debug, Default)]
pub struct EditParts {
    pub draft: Draft,
    /// The model they're for, and its scene path.
    model: Option<(Entity, String)>,
    /// Each part's entity (a link's, a visual's), as the draft's parts.
    spawned: Vec<Option<Entity>>,
    /// The extras the server sent, while the draft's are added to them.
    base: Option<Option<WorldExtras>>,
    /// The extras' generation the draft last set.
    ours: Option<u32>,
    /// The draft changed since the extras were set.
    dirty: bool,
}

impl EditParts {
    /// The edited model's scene path, while one is edited.
    pub fn model_path(&self) -> Option<&str> {
        self.model.as_ref().map(|(_, p)| p.as_str())
    }

    /// The server's SDFormat came (`extras` holds it, the draft's parts
    /// added): its joints and names are known.
    pub fn server_known(&self, extras: &SdfExtras) -> bool {
        match &self.base {
            Some(base) => base.is_some(),
            None => extras.extras.is_some(),
        }
    }
}

/// What a part spawn reads and writes.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PartScene<'w, 's> {
    entities: Query<'w, 's, (Entity, &'static GzEntity)>,
    sensors: Query<'w, 's, &'static mut GzSensors>,
    assets: Option<GeometryAssets<'w>>,
    commands: Commands<'w, 's>,
}

impl PartScene<'_, '_> {
    /// The entity at scene path `path`.
    fn at(&self, path: &str) -> Option<Entity> {
        self.entities
            .iter()
            .find(|(_, g)| g.path == path)
            .map(|(e, _)| e)
    }

    /// Spawns `node` under `parent` (without a renderer, its entities
    /// without their meshes).
    fn spawn(&mut self, node: &Node, parent: Entity) -> Entity {
        let e = match self.assets.as_mut() {
            Some(a) => a.spawn_node(&mut self.commands, node, parent),
            None => bare(&mut self.commands, node, parent),
        };
        self.commands.entity(e).insert(DraftPart);
        e
    }

    /// Shows `part` of `model` (a scene path), returning its entity.
    fn show(&mut self, part: &Part, model: &str) -> Option<Entity> {
        let node = |name: &str, path: String, kind, pose: Pose, visual: Option<VisualDesc>| Node {
            name: name.to_string(),
            id: 0,
            kind,
            path,
            pose,
            scale: [1.0; 3],
            visible: true,
            visual,
            light: None,
            sensors: Vec::new(),
            children: Vec::new(),
        };
        match part {
            Part::Link { name, shape, pose } => {
                let path = format!("{model}/{name}");
                let mut link = node(name, path.clone(), NodeKind::Link, *pose, None);
                link.children.push(node(
                    "visual",
                    format!("{path}/visual"),
                    NodeKind::Visual,
                    Pose::IDENTITY,
                    Some(shape_visual(*shape, LINK_SIZE)),
                ));
                let parent = self.at(model)?;
                Some(self.spawn(&link, parent))
            }
            Part::Visual {
                link,
                name,
                shape,
                pose,
            } => {
                let visual = node(
                    name,
                    format!("{link}/{name}"),
                    NodeKind::Visual,
                    *pose,
                    Some(shape_visual(*shape, PART_SIZE)),
                );
                let parent = self.at(link)?;
                Some(self.spawn(&visual, parent))
            }
            Part::Sensor { link, .. } => {
                let (Some(e), Some(desc)) = (self.at(link), part.sensor()) else {
                    return None;
                };
                match self.sensors.get_mut(e) {
                    Ok(mut s) => s.0.push(desc),
                    Err(_) => {
                        self.commands.entity(e).insert(GzSensors(vec![desc]));
                    }
                }
                None
            }
            Part::Collision { .. } | Part::Joint { .. } | Part::Plugin { .. } => None,
        }
    }

    /// Takes `part` back: its entity, or its sensor.
    fn hide(&mut self, part: &Part, entity: Option<Entity>) {
        if let Some(e) = entity {
            self.commands.entity(e).despawn();
        }
        if let Part::Sensor { link, name, .. } = part {
            if let Some(mut s) = self.at(link).and_then(|e| self.sensors.get_mut(e).ok()) {
                s.0.retain(|d| d.name != *name);
            }
        }
    }
}

/// `node` and its children as entities, without meshes (an app without a
/// renderer).
fn bare(commands: &mut Commands, node: &Node, parent: Entity) -> Entity {
    let e = commands
        .spawn((
            Name::new(node.name.clone()),
            GzEntity {
                name: node.name.clone(),
                id: node.id,
                kind: node.kind,
                path: node.path.clone(),
            },
            pose_transform(&node.pose, node.scale),
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    if let Some(v) = &node.visual {
        commands.entity(e).insert(GzVisual(v.clone()));
    }
    for child in &node.children {
        bare(commands, child, e);
    }
    e
}

/// Runs the [`PartCommand`]s, and undo and redo while a model is edited.
/// A new part is selected when it is an entity. Leaving (or editing
/// another model) drops the draft.
pub fn run_parts(
    editing: Res<EditingModel>,
    mut parts: ResMut<EditParts>,
    mut adds: MessageReader<PartCommand>,
    mut undos: MessageReader<UndoCommand>,
    mut scene: PartScene,
    mut selection: ResMut<Selection>,
    mut discarded: MessageWriter<PartsDiscarded>,
) {
    let model = editing.model();
    if parts.model.as_ref().map(|(e, _)| *e) != model {
        let gone: Vec<(Part, Option<Entity>)> = parts
            .draft
            .parts()
            .iter()
            .cloned()
            .zip(parts.spawned.iter().copied())
            .collect();
        if !gone.is_empty() {
            for (part, e) in gone.iter().rev() {
                scene.hide(part, *e);
            }
            discarded.write(PartsDiscarded(gone.len()));
        }
        let path = model.and_then(|m| scene.entities.get(m).ok().map(|(_, g)| g.path.clone()));
        let parts = &mut *parts;
        parts.draft = Draft::default();
        parts.spawned.clear();
        parts.model = model.zip(path);
        parts.dirty = true;
    }
    let Some(path) = parts.model_path().map(str::to_string) else {
        adds.clear();
        undos.clear();
        return;
    };
    for PartCommand::Add(part) in adds.read() {
        let e = scene.show(part, &path);
        if let Some(e) = e {
            *selection = Selection {
                entities: vec![e],
                parts: Vec::new(),
            };
        }
        let parts = &mut *parts;
        parts.draft.add(part.clone());
        parts.spawned.push(e);
        parts.dirty = true;
    }
    for cmd in undos.read() {
        let parts = &mut *parts;
        match cmd {
            UndoCommand::Undo => {
                let Some(part) = parts.draft.undo().cloned() else {
                    continue;
                };
                let e = parts.spawned.pop().flatten();
                scene.hide(&part, e);
                if e.is_some_and(|e| selection.entities.contains(&e)) {
                    *selection = Selection::default();
                }
                // A joint or sensor selected as a part goes with it.
                let kind = match &part {
                    Part::Joint { .. } => Some((PartKind::Joint, path.as_str())),
                    Part::Sensor { link, .. } => Some((PartKind::Sensor, link.as_str())),
                    _ => None,
                };
                if let Some((kind, owner)) = kind {
                    let owner = scene.at(owner);
                    let gone = |p: &SelectedPart| {
                        p.kind == kind && Some(p.owner) == owner && p.name == part.name()
                    };
                    if selection.parts.iter().any(gone) {
                        selection.parts.retain(|p| !gone(p));
                    }
                }
            }
            UndoCommand::Redo => {
                let Some(part) = parts.draft.redo().cloned() else {
                    continue;
                };
                let e = scene.show(&part, &path);
                parts.spawned.push(e);
            }
        }
        parts.dirty = true;
    }
}

/// The world's extras with the draft's parts added, while there are any;
/// the server's back once there are none.
pub fn patch_extras(mut parts: ResMut<EditParts>, extras: Option<ResMut<SdfExtras>>) {
    let Some(mut extras) = extras else {
        return;
    };
    let server_new = parts.ours != Some(extras.generation);
    let patched = parts.model.is_some() && !parts.draft.is_empty();
    if !patched {
        // Read first: a take through `ResMut` would mark it changed every
        // frame.
        if parts.base.is_some() {
            let base = parts.base.take().flatten();
            // A reply since is the server's own, newer.
            if !server_new {
                extras.extras = base;
                extras.generation += 1;
            }
            parts.ours = None;
        }
        return;
    }
    if !server_new && !parts.dirty {
        return;
    }
    if parts.base.is_none() || server_new {
        parts.base = Some(extras.extras.clone());
    }
    let parts = &mut *parts;
    let mut e = parts.base.clone().flatten().unwrap_or_default();
    if let Some(model) = parts.model_path() {
        for p in parts.draft.parts() {
            p.add_to(model, &mut e);
        }
    }
    extras.extras = Some(e);
    extras.generation += 1;
    parts.ours = Some(extras.generation);
    parts.dirty = false;
}

#[cfg(test)]
mod tests;
