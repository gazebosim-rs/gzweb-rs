//! Align (spec §7.9 › Align, like Gazebo's AlignTool; DECISIONS.md D-109):
//! the selected models move along one of the world's axes so their
//! bounds' min, centre or max lines up with the first selected one's. Each
//! move is a `set_pose` ([`SetPose`]), and all of them are one edit on the
//! undo stack.

use bevy::prelude::*;

use super::{Bounds, Skip, ViewScene};
use crate::connection::GzStatus;
use crate::edit::{edits_allowed, movable, HasPlane, SetPose};
use crate::spawn::{GzEntity, GzWorldRoot};
use crate::undo::{Change, Record};

/// Which part of the bounds lines up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignEdge {
    Min,
    Center,
    Max,
}

impl AlignEdge {
    pub const ALL: [AlignEdge; 3] = [AlignEdge::Min, AlignEdge::Center, AlignEdge::Max];

    pub fn label(self) -> &'static str {
        match self {
            AlignEdge::Min => "min",
            AlignEdge::Center => "center",
            AlignEdge::Max => "max",
        }
    }

    /// `b`'s value along `axis`.
    fn of(self, b: &Bounds, axis: usize) -> f32 {
        match self {
            AlignEdge::Min => b.min[axis],
            AlignEdge::Center => (b.min[axis] + b.max[axis]) / 2.0,
            AlignEdge::Max => b.max[axis],
        }
    }
}

/// Align the selected models to the first along `axis` (0, 1, 2: the
/// world's X, Y and Z) by `edge`.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignModels {
    pub axis: usize,
    pub edge: AlignEdge,
}

/// `e`'s bounds in the world's (Gazebo's) frame, `to_root` mapping Bevy's
/// world into it; an entity without meshes is the point it is at.
pub(super) fn gz_bounds(
    scene: &ViewScene,
    to_root: &bevy::math::Affine3A,
    e: Entity,
) -> Option<Bounds> {
    let world = scene.bounds_of(e, Skip::NOTHING).or_else(|| {
        let (_, global, ..) = scene.nodes.get(e).ok()?;
        Some(Bounds::point(global.translation()))
    })?;
    Some(in_frame(world, to_root))
}

/// `b` in the frame `to` maps world points into.
fn in_frame(b: Bounds, to: &bevy::math::Affine3A) -> Bounds {
    (0..8)
        .map(|i| {
            let p = Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            );
            let q = to.transform_point3(p);
            Bounds { min: q, max: q }
        })
        .reduce(Bounds::union)
        .unwrap_or(b)
}

/// The models that can move in the selection, in its order.
type Movers<'w, 's> = Query<
    'w,
    's,
    (
        &'static GzEntity,
        &'static ChildOf,
        &'static mut Transform,
        Has<HasPlane>,
    ),
>;

/// Runs [`AlignModels`]: on a server that takes edits, with two or more
/// movable models selected (the first is where the rest line up).
pub(crate) fn align_models(
    mut asks: MessageReader<AlignModels>,
    status: Option<Res<GzStatus>>,
    scene: ViewScene,
    roots: Query<(Entity, &GlobalTransform), With<GzWorldRoot>>,
    mut movers: Movers,
    mut poses: MessageWriter<SetPose>,
    mut records: MessageWriter<Record>,
) {
    for ask in asks.read() {
        if !edits_allowed(status.as_deref()) || ask.axis > 2 {
            continue;
        }
        let Ok((root, root_at)) = roots.single() else {
            continue;
        };
        let to_root = root_at.affine().inverse();
        // The selected models that can move, with their bounds in the
        // world's (Gazebo's) frame: a model without meshes is its origin.
        let boxes: Vec<(Entity, Bounds)> = scene
            .selection
            .entities
            .iter()
            .filter(|e| {
                movers
                    .get(**e)
                    .is_ok_and(|(g, parent, _, plane)| movable(g, parent, root, plane))
            })
            .filter_map(|e| Some((*e, gz_bounds(&scene, &to_root, *e)?)))
            .collect();
        let Some(((_, first), rest)) = boxes.split_first() else {
            continue;
        };
        let target = ask.edge.of(first, ask.axis);
        let mut changes = vec![];
        for (e, b) in rest {
            let delta = target - ask.edge.of(b, ask.axis);
            if delta.abs() < 1e-6 {
                continue;
            }
            let Ok((_, _, mut t, _)) = movers.get_mut(*e) else {
                continue;
            };
            let before = *t;
            t.translation[ask.axis] += delta;
            poses.write(SetPose {
                entity: *e,
                pose: *t,
            });
            changes.push(Change::Pose {
                entity: *e,
                before,
                after: *t,
            });
        }
        if !changes.is_empty() {
            records.write(Record {
                verb: "align",
                named: true,
                changes,
                scrub: None,
                open: false,
            });
        }
    }
}
