//! Selecting in the views (spec §6.1, §6.2, §6.4): a click picks the
//! top-level model under the pointer, a marquee the models it overlaps,
//! and [`SelectMode`] says how either changes the [`Selection`].

use bevy::prelude::*;

use super::{viewport_ray, Part, Selection, Skip, ViewScene};

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
    /// The selection after a pick of `picked`: replace selects exactly
    /// them (without repeats), so picking nothing clears; add appends the
    /// new ones; toggle removes the selected ones and appends the others.
    pub fn applied(&self, mode: SelectMode, picked: &[Entity]) -> Vec<Entity> {
        let mut out = match mode {
            SelectMode::Replace => Vec::new(),
            _ => self.entities.clone(),
        };
        for e in picked {
            match out.iter().position(|s| s == e) {
                Some(i) if mode == SelectMode::Toggle => {
                    out.remove(i);
                }
                Some(_) => {}
                None => out.push(*e),
            }
        }
        out
    }
}

/// Applies a pick to the selection, changing it (and so its change
/// detection) only if the pick changes what is selected. Replacing also
/// drops the selected parts.
pub fn select(selection: &mut Mut<Selection>, mode: SelectMode, picked: &[Entity]) {
    let next = selection.applied(mode, picked);
    let clear_parts = mode == SelectMode::Replace && !selection.parts.is_empty();
    if next != selection.entities || clear_parts {
        selection.entities = next;
        if clear_parts {
            selection.parts.clear();
        }
    }
}

/// Applies a pick of parts as [`select`] does entities: replacing drops
/// the selected entities.
pub fn select_parts(selection: &mut Mut<Selection>, mode: SelectMode, picked: &[Part]) {
    let mut parts = match mode {
        SelectMode::Replace => Vec::new(),
        _ => selection.parts.clone(),
    };
    for p in picked {
        match parts.iter().position(|q| q == p) {
            Some(i) if mode == SelectMode::Toggle => {
                parts.remove(i);
            }
            Some(_) => {}
            None => parts.push(p.clone()),
        }
    }
    let clear_entities = mode == SelectMode::Replace && !selection.entities.is_empty();
    if parts != selection.parts || clear_entities {
        selection.parts = parts;
        if clear_entities {
            selection.entities.clear();
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

/// Whether clicks and marquees in the views select: in the full UI only,
/// which shows the selection (minimal mode has none, DECISIONS.md D-14).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectInViews(pub bool);

/// Set by the full UI while its Box select tool is on: a plain left drag
/// draws a marquee instead of moving the camera.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoxSelectTool(pub bool);

impl ViewScene<'_, '_> {
    /// The top-level model the mesh under `ray` belongs to, if it is in
    /// the Gazebo tree; while a model is edited, its link (or nested model)
    /// instead (spec §6.6). A model of only plane visuals (the ground) is
    /// empty space, as for a marquee and focus.
    pub(super) fn model_under(&mut self, ray: Ray3d) -> Option<Entity> {
        let (hit, _) = self.hit(ray)?;
        if let Some(model) = self.editing() {
            let part = super::editing::part_of(&self.parents, hit, model)?;
            return self
                .bounds_of(part, Skip::RUNAWAY_AND_PLANES)
                .is_some()
                .then_some(part);
        }
        let model = self.model_of(hit);
        let parent = self.parents.get(model).ok()?.parent();
        let solid = self.bounds_of(model, Skip::RUNAWAY_AND_PLANES).is_some();
        (self.roots.contains(parent) && solid).then_some(model)
    }

    /// The top-level models whose bounds, as `camera` sees them, overlap
    /// the screen rectangle between `a` and `b` (while a model is edited,
    /// its links and nested models). Hidden models, ground planes and what
    /// has run away don't count, as for focus.
    pub(super) fn models_in_rect(
        &self,
        camera: (&Transform, &Projection),
        size: Vec2,
        a: Vec2,
        b: Vec2,
    ) -> Vec<Entity> {
        let rect = Rect::from_corners(a, b);
        let Some(root) = self.editing().or_else(|| self.roots.iter().next()) else {
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
                    .is_some_and(|r| overlap(r, rect))
            })
            .collect()
    }
}

/// Whether two rectangles overlap, touching edges included (a marquee
/// dragged straight across is a line).
fn overlap(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && b.min.x <= a.max.x && a.min.y <= b.max.y && b.min.y <= a.max.y
}

/// The box's twelve edges, as pairs of [`Bounds::corners`](super::Bounds::corners)
/// indices.
const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// The screen rectangle (logical pixels, y down) that a box with these
/// corners covers, clipped where it passes behind the camera (its edges
/// cut at a plane just in front of the eye, so a long model that reaches
/// behind still covers the screen it fills); `None` if it is all behind.
pub(super) fn screen_rect(
    (t, p): (&Transform, &Projection),
    size: Vec2,
    corners: [Vec3; 8],
) -> Option<Rect> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let mut p = p.clone();
    p.update(size.x, size.y);
    let clip_from_world = p.get_clip_from_view() * t.to_matrix().inverse();
    let clip = corners.map(|c| clip_from_world * c.extend(1.0));
    let near = 1e-4;
    let cut = EDGES.iter().filter_map(|&(a, b)| {
        let (a, b) = (clip[a], clip[b]);
        ((a.w > near) != (b.w > near)).then(|| a + (b - a) * ((near - a.w) / (b.w - a.w)))
    });
    clip.iter()
        .copied()
        .filter(|c| c.w > near)
        .chain(cut)
        .map(|c| {
            let ndc = c.truncate() / c.w;
            let q = Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y);
            Rect::from_corners(q, q)
        })
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
    let gone = |e: &Entity| entities.get(*e).is_err();
    if selection.entities.iter().any(gone) || selection.parts.iter().any(|p| gone(&p.owner)) {
        selection.entities.retain(|e| !gone(e));
        selection.parts.retain(|p| !gone(&p.owner));
    }
}
