//! Selecting in the views (spec §6.1, §6.2, §6.4): a click picks the
//! top-level model under the pointer, a marquee the models it overlaps,
//! and [`SelectMode`] says how either changes the [`Selection`].

use bevy::prelude::*;

use super::{viewport_ray, Selection, Skip, ViewScene};

/// How a pick changes the selection: a plain click replaces it, Shift adds
/// and ⌘/Ctrl toggles (spec §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Toggle,
}

impl SelectMode {
    /// The mode the held modifiers ask for: ⌘/Ctrl (`command`) toggles
    /// before Shift adds.
    pub fn from_keys(shift: bool, command: bool) -> SelectMode {
        if command {
            SelectMode::Toggle
        } else if shift {
            SelectMode::Add
        } else {
            SelectMode::Replace
        }
    }
}

impl Selection {
    /// Applies a pick of `picked` (in order, without repeats): replace
    /// selects exactly them, so picking nothing clears; add appends the
    /// new ones; toggle removes the selected ones and appends the others.
    pub fn apply(&mut self, mode: SelectMode, picked: &[Entity]) {
        match mode {
            SelectMode::Replace => {
                if self.entities != picked {
                    self.entities = picked.to_vec();
                }
            }
            SelectMode::Add => {
                for e in picked {
                    if !self.entities.contains(e) {
                        self.entities.push(*e);
                    }
                }
            }
            SelectMode::Toggle => {
                for e in picked {
                    match self.entities.iter().position(|s| s == e) {
                        Some(i) => {
                            self.entities.remove(i);
                        }
                        None => self.entities.push(*e),
                    }
                }
            }
        }
    }
}

/// The marquee a left drag draws (the Box select tool, or ⌘/Ctrl+drag in
/// Navigate), in the view's logical pixels, for the UI to show.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct Marquee(pub Option<MarqueeRect>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarqueeRect {
    pub view: Entity,
    pub from: Vec2,
    pub to: Vec2,
}

/// Set by the full UI while its Box select tool is on: a plain left drag
/// draws a marquee instead of moving the camera.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoxSelectTool(pub bool);

impl ViewScene<'_, '_> {
    /// The top-level model the mesh under `ray` belongs to, if it is in
    /// the Gazebo tree.
    pub(super) fn model_under(&mut self, ray: Ray3d) -> Option<Entity> {
        let (hit, _) = self.hit(ray)?;
        let model = self.model_of(hit);
        let parent = self.parents.get(model).ok()?.parent();
        self.roots.contains(parent).then_some(model)
    }

    /// The top-level models whose bounds, as `camera` sees them, overlap
    /// the screen rectangle between `a` and `b`. Hidden models, ground
    /// planes and what has run away don't count, as for focus.
    pub(super) fn models_in_rect(
        &self,
        camera: (&Transform, &Projection),
        size: Vec2,
        a: Vec2,
        b: Vec2,
    ) -> Vec<Entity> {
        let rect = Rect::from_corners(a, b);
        let Some(root) = self.roots.iter().next() else {
            return Vec::new();
        };
        let Ok((.., Some(children), _, _)) = self.nodes.get(root) else {
            return Vec::new();
        };
        children
            .iter()
            .filter(|model| {
                self.bounds_of(*model, Skip::RUNAWAY_AND_PLANES)
                    .and_then(|bounds| screen_rect(camera, size, bounds.corners()))
                    .is_some_and(|r| !r.intersect(rect).is_empty())
            })
            .collect()
    }
}

/// The screen rectangle (logical pixels, y down) that `corners` cover, from
/// the corners in front of the camera; `None` if none is.
fn screen_rect((t, p): (&Transform, &Projection), size: Vec2, corners: [Vec3; 8]) -> Option<Rect> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let mut p = p.clone();
    p.update(size.x, size.y);
    let clip_from_world = p.get_clip_from_view() * t.to_matrix().inverse();
    corners
        .into_iter()
        .filter_map(|c| {
            let clip = clip_from_world * c.extend(1.0);
            (clip.w > f32::EPSILON).then(|| {
                let ndc = clip.truncate() / clip.w;
                Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
            })
        })
        .map(|q| Rect::from_corners(q, q))
        .reduce(|a, b| a.union(b))
}

/// The model a click at `at` picks, through the view's own projection.
pub(super) fn pick_at(
    scene: &mut ViewScene,
    camera: (&Transform, &Projection),
    size: Vec2,
    at: Vec2,
) -> Option<Entity> {
    let (t, p) = camera;
    viewport_ray(p, t, size, at).and_then(|r| scene.model_under(r))
}

/// Drops selected entities that no longer exist (a deleted or respawned
/// model), so nothing downstream sees a stale one.
pub fn prune_selection(mut selection: ResMut<Selection>, entities: Query<()>) {
    if selection.entities.iter().any(|e| entities.get(*e).is_err()) {
        selection.entities.retain(|e| entities.get(*e).is_ok());
    }
}
