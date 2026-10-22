//! Undo and redo (spec §7.1, M5d): what dirties the world (a move by a
//! tool or the inspector, a hide or show) is [`Record`]ed onto the
//! [`UndoStack`] as one edit, by scene path so a respawn doesn't lose it;
//! an [`UndoCommand`] puts it back or does it again (a move as a
//! `set_pose`, so physics carries on from the pose put back). The stack is
//! the world's: a new server or world clears it, a Reset (sim time going
//! back) forgets the moves it undid, and a new scene from a reconnect
//! forgets the hides and Isolate it undid (DECISIONS.md D-89). The world is
//! dirty while the stack isn't where it was at load.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::connection::{GzCommand, GzStatus};
use crate::edit::{edits_allowed, Held, SetPose};
use crate::spawn::{GzEntity, GzIndex};

/// One change an edit makes, as recorded (by entity).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A pose in the entity's parent's frame, before and after.
    Pose {
        entity: Entity,
        before: Transform,
        after: Transform,
    },
    Visibility {
        entity: Entity,
        before: Visibility,
        after: Visibility,
    },
    /// Isolate turned on (`after` holds the visibility it saved) or off.
    Isolated {
        before: Option<Saved>,
        after: Option<Saved>,
    },
}

/// Visibilities by scene path.
pub type Saved = Vec<(String, Visibility)>;

/// While Isolate is on: each top-level model's visibility before it (by
/// scene path), to put back when it goes off (DECISIONS.md D-74).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Isolated(pub Option<Saved>);

/// Something the user did, to put on the stack: `verb` and what it acted
/// on name it ("move cart", "hide 3 models"; `named: false` for one that
/// names itself, "hide others"). An inspector scrub under way is `open`
/// on its field (`scrub`): the field's next step (its release, no longer
/// `open`) joins it, and anything else closes it.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Record {
    pub verb: &'static str,
    pub named: bool,
    pub changes: Vec<Change>,
    pub scrub: Option<Entity>,
    pub open: bool,
}

/// ⌘Z and ⇧⌘Z.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoCommand {
    Undo,
    Redo,
}

/// A change as kept: by scene path.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Pose {
        path: String,
        before: Transform,
        after: Transform,
    },
    Visibility {
        path: String,
        before: Visibility,
        after: Visibility,
    },
    Isolated {
        before: Option<Saved>,
        after: Option<Saved>,
    },
}

impl Step {
    /// Whether it changes anything.
    fn changes(&self) -> bool {
        match self {
            Step::Pose { before, after, .. } => before != after,
            Step::Visibility { before, after, .. } => before != after,
            Step::Isolated { before, after } => before != after,
        }
    }

    fn path(&self) -> Option<&str> {
        match self {
            Step::Pose { path, .. } | Step::Visibility { path, .. } => Some(path),
            Step::Isolated { .. } => None,
        }
    }
}

/// One undoable edit.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// What the World menu calls it: "move cart" ("Undo move cart").
    pub label: String,
    steps: Vec<Step>,
    scrub: Option<Entity>,
    open: bool,
}

impl Edit {
    fn paths(&self) -> Vec<Option<&str>> {
        self.steps.iter().map(Step::path).collect()
    }

    /// The next step of the same scrub.
    fn takes(&self, next: &Edit) -> bool {
        self.open
            && self.scrub.is_some()
            && self.scrub == next.scrub
            && self.label == next.label
            && self.paths() == next.paths()
    }

    fn changes(&self) -> bool {
        self.steps.iter().any(Step::changes)
    }
}

/// The world's edits: done (the last on top) and undone.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct UndoStack {
    done: Vec<Edit>,
    undone: Vec<Edit>,
    /// How many edits were done when the world was last saved or loaded;
    /// `None` once that state can't come back.
    saved: Option<usize>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            saved: Some(0),
        }
    }
}

impl UndoStack {
    /// Whether an authored property changed since load (§7.1 dirty state).
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.done.len())
    }

    /// What ⌘Z would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.done.last().map(|e| e.label.as_str())
    }

    /// What ⇧⌘Z would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.undone.last().map(|e| e.label.as_str())
    }

    /// Forgets everything: the world as it is is the world as loaded.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Puts `edit` on top (and forgets what was undone), or into the top
    /// edit while that is the same scrub's. Anything else closes the top.
    fn push(&mut self, edit: Edit) {
        self.undone.clear();
        if self.saved.is_some_and(|s| s > self.done.len()) {
            self.saved = None;
        }
        if let Some(top) = self.done.last_mut() {
            if top.takes(&edit) {
                for (kept, new) in top.steps.iter_mut().zip(edit.steps) {
                    match (kept, new) {
                        (Step::Pose { after, .. }, Step::Pose { after: a, .. }) => *after = a,
                        (Step::Visibility { after, .. }, Step::Visibility { after: a, .. }) => {
                            *after = a;
                        }
                        (Step::Isolated { after, .. }, Step::Isolated { after: a, .. }) => {
                            *after = a;
                        }
                        _ => {}
                    }
                }
                top.open = edit.open;
                self.close_top();
                return;
            }
            // A scrub cut short (its field gone before its release).
            top.open = false;
            self.close_top();
        }
        if edit.changes() || edit.open {
            self.done.push(edit);
        }
    }

    /// Drops the top edit if it is closed and changes nothing (scrubbed
    /// back to where it began).
    fn close_top(&mut self) {
        if self.done.last().is_some_and(|e| !e.open && !e.changes()) {
            self.done.pop();
            // What it left is what it found.
            if let Some(s) = self.saved.as_mut() {
                *s = (*s).min(self.done.len());
            }
        }
    }

    /// Forgets the steps `gone` picks (the world put back what they did,
    /// so undoing them would do it again); an edit left with nothing is
    /// dropped. The world as loaded stays where it was, at the bottom.
    fn forget(&mut self, gone: impl Fn(&Step) -> bool) {
        for list in [&mut self.done, &mut self.undone] {
            for e in list.iter_mut() {
                e.steps.retain(|s| !gone(s));
            }
            list.retain(|e| !e.steps.is_empty());
        }
        // A save part way up may not be there any more.
        self.saved = self.saved.filter(|s| *s == 0);
    }

    /// Forgets the moves a Reset undid (gz-sim puts every pose back).
    fn forget_poses(&mut self) {
        self.forget(|s| matches!(s, Step::Pose { .. }));
    }

    /// Forgets the hides and Isolate a new scene undid (it comes with
    /// everything shown).
    fn forget_visibility(&mut self) {
        self.forget(|s| matches!(s, Step::Visibility { .. } | Step::Isolated { .. }));
    }
}

/// Where undo runs: after the gizmo and the keys, so an edit and an undo
/// in the same frame go in that order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UndoSystems;

pub struct UndoPlugin;

impl Plugin for UndoPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Record>()
            .add_message::<UndoCommand>()
            .add_message::<SetPose>()
            .add_message::<GzCommand>()
            .init_resource::<UndoStack>()
            .init_resource::<Isolated>()
            .init_resource::<Held>()
            .add_systems(
                Update,
                (forget_other_worlds, record, undo_redo)
                    .chain()
                    .in_set(UndoSystems)
                    .after(crate::view::gizmo::gizmo_input),
            );
    }
}

/// The edit's name: its verb and what it acted on.
fn label(verb: &str, named: bool, names: &[&str]) -> String {
    match names {
        _ if !named => verb.to_string(),
        [one] => format!("{verb} {one}"),
        many => format!("{verb} {} entities", many.len()),
    }
}

/// Puts each [`Record`] on the stack, by path.
fn record(
    mut records: MessageReader<Record>,
    entities: Query<&GzEntity>,
    mut stack: ResMut<UndoStack>,
) {
    for r in records.read() {
        let mut names: Vec<&str> = Vec::new();
        let mut steps = Vec::new();
        for c in &r.changes {
            let entity = match c {
                Change::Pose { entity, .. } | Change::Visibility { entity, .. } => {
                    entities.get(*entity).ok()
                }
                Change::Isolated { .. } => None,
            };
            if let Some(g) = entity {
                if !names.contains(&g.name.as_str()) {
                    names.push(&g.name);
                }
            }
            steps.push(match (c, entity) {
                (Change::Pose { before, after, .. }, Some(g)) => Step::Pose {
                    path: g.path.clone(),
                    before: *before,
                    after: *after,
                },
                (Change::Visibility { before, after, .. }, Some(g)) => Step::Visibility {
                    path: g.path.clone(),
                    before: *before,
                    after: *after,
                },
                (Change::Isolated { before, after }, _) => Step::Isolated {
                    before: before.clone(),
                    after: after.clone(),
                },
                // Gone already: nothing to put back.
                _ => continue,
            });
        }
        if steps.is_empty() {
            continue;
        }
        stack.push(Edit {
            label: label(r.verb, r.named, &names),
            steps,
            scrub: r.scrub,
            open: r.open,
        });
    }
}

/// Undoes or redoes the top edit. A move needs a server that takes edits:
/// without one the edit stays where it is. An entity that is gone is
/// skipped.
#[allow(clippy::too_many_arguments)]
fn undo_redo(
    mut commands: MessageReader<UndoCommand>,
    status: Option<Res<GzStatus>>,
    time: Res<Time<Real>>,
    entities: Query<(Entity, &GzEntity)>,
    mut places: Query<(&mut Transform, &mut Visibility), With<GzEntity>>,
    mut stack: ResMut<UndoStack>,
    mut isolated: ResMut<Isolated>,
    mut held: ResMut<Held>,
    mut edits: MessageWriter<SetPose>,
) {
    for cmd in commands.read() {
        let undo = *cmd == UndoCommand::Undo;
        let top = if undo {
            stack.done.last()
        } else {
            stack.undone.last()
        };
        let Some(edit) = top else {
            continue;
        };
        let moves = edit.steps.iter().any(|s| matches!(s, Step::Pose { .. }));
        if moves && !edits_allowed(status.as_deref()) {
            warn!(
                "can't {} {}: the server doesn't take edits",
                if undo { "undo" } else { "redo" },
                edit.label
            );
            continue;
        }
        let mut edit = if undo {
            stack.done.pop()
        } else {
            stack.undone.pop()
        }
        .expect("the top edit");
        edit.open = false;
        let by_path: HashMap<&str, Entity> = entities
            .iter()
            .filter(|(_, g)| edit.steps.iter().any(|s| s.path() == Some(&g.path)))
            .map(|(e, g)| (g.path.as_str(), e))
            .collect();
        let now = time.elapsed_secs_f64();
        // Undone last to first.
        let order: Vec<&Step> = if undo {
            edit.steps.iter().rev().collect()
        } else {
            edit.steps.iter().collect()
        };
        for step in order {
            let found = step
                .path()
                .and_then(|p| by_path.get(p))
                .and_then(|e| places.get_mut(*e).ok().map(|p| (*e, p)));
            match (step, found) {
                (Step::Pose { before, after, .. }, Some((e, (mut t, _)))) => {
                    let pose = if undo { *before } else { *after };
                    *t = pose;
                    edits.write(SetPose { entity: e, pose });
                    // The stream's older poses don't pull it back.
                    held.release(e, now);
                }
                (Step::Visibility { before, after, .. }, Some((_, (_, mut v)))) => {
                    v.set_if_neq(if undo { *before } else { *after });
                }
                (Step::Isolated { before, after }, _) => {
                    isolated.0 = if undo { before.clone() } else { after.clone() };
                }
                _ => warn!("{}: an entity it changed is gone", edit.label),
            }
        }
        if undo {
            stack.undone.push(edit);
        } else {
            stack.done.push(edit);
        }
    }
}

/// What the world was last seen to be: its server and name, its
/// iterations, and how many scenes had come.
#[derive(Default)]
struct Seen {
    world: Option<(String, Option<String>)>,
    iterations: Option<u64>,
    scenes: u32,
}

/// A new server or world clears the stack (it is the world's, §7.1). A
/// Reset (ours, another client's, or a restarted server's: the sim's
/// iterations going back) forgets the moves it put back. A new scene for
/// the same world (a reconnect) comes with everything shown: it forgets
/// the hides and Isolate.
fn forget_other_worlds(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut sim: MessageReader<GzCommand>,
    mut stack: ResMut<UndoStack>,
    mut isolated: ResMut<Isolated>,
    mut seen: Local<Seen>,
) {
    if sim.read().any(|c| *c == GzCommand::Reset) {
        stack.forget_poses();
    }
    if let Some(index) = index.filter(|i| i.is_changed()) {
        if index.scenes > seen.scenes.max(1) {
            stack.forget_visibility();
            if isolated.0.is_some() {
                isolated.0 = None;
            }
        }
        seen.scenes = index.scenes;
    }
    let Some(status) = status.filter(|s| s.is_changed()) else {
        return;
    };
    let now = (status.url.clone(), status.world.clone());
    // A connection on its way (no world yet) isn't another world.
    if now.1.is_none() {
        return;
    }
    if seen.world.as_ref().is_some_and(|w| *w != now) {
        stack.clear();
        isolated.0 = None;
        seen.iterations = None;
    }
    seen.world = Some(now);
    if let Some(iterations) = status.sim.map(|s| s.iterations) {
        if seen.iterations.is_some_and(|i| iterations < i) {
            stack.forget_poses();
        }
        seen.iterations = Some(iterations);
    }
}

#[cfg(test)]
mod tests;
