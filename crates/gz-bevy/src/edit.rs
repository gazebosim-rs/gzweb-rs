//! Pose edits (spec §6.1, M5c): a [`SetPose`] becomes a `set_pose`
//! request for the entity's Gazebo id, sent only to a server that takes
//! edits ([`GzStatus::edits`], DECISIONS.md D-76). While an entity is being
//! dragged, and until the server has answered its edits and a moment
//! after, the sim's poses for it are [`Held`], so the stream doesn't pull
//! it back under the pointer or flash the old pose before the edit lands.

use std::collections::VecDeque;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gz_core::msgs;
use gz_core::scene::{GeometryDesc, NodeKind};
use gz_core::topic::world_topic;
use prost::Message as _;

use crate::connection::{GzCommand, GzServiceReply, GzStatus};
use crate::spawn::{GzEntity, GzVisual, GzWorldRoot};

/// How long after an edit (and after the server answered it) the sim's
/// poses for its entity stay held.
pub const HOLD_AFTER_EDIT_SECS: f64 = 0.5;
/// How long a `set_pose` the server hasn't answered keeps its entity held.
pub const REPLY_TIMEOUT_SECS: f64 = 3.0;

/// Move an entity (a model or a light under the world root) to `pose`, in
/// its parent's frame (Gazebo's axes).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct SetPose {
    pub entity: Entity,
    pub pose: Transform,
}

/// Entities whose incoming poses are ignored: while dragged, until a time
/// (`Time<Real>` seconds), and while a `set_pose` for them is unanswered.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Held {
    /// While dragged (`None`), or until a time.
    pub until: HashMap<Entity, Option<f64>>,
    /// The `set_pose`s sent and not yet answered, oldest first, with when
    /// they went (the server answers in order; a reply doesn't name its
    /// entity).
    awaiting: VecDeque<(Entity, f64)>,
}

impl Held {
    /// Whether `entity`'s poses are held at `now`.
    pub fn holds(&self, entity: Entity, now: f64) -> bool {
        match self.until.get(&entity) {
            Some(None) => true,
            Some(Some(until)) => now < *until || self.awaits(entity, now),
            None => false,
        }
    }

    /// Whether a `set_pose` for `entity` is still to be answered.
    fn awaits(&self, entity: Entity, now: f64) -> bool {
        self.awaiting
            .iter()
            .any(|(e, at)| *e == entity && now - at < REPLY_TIMEOUT_SECS)
    }

    /// A `set_pose` for `entity` went out at `now`.
    fn sent(&mut self, entity: Entity, now: f64) {
        self.awaiting.push_back((entity, now));
    }

    /// The server answered the oldest `set_pose`: its entity's poses come
    /// through a moment later, once it isn't dragged.
    fn answered(&mut self, now: f64) {
        let Some((e, _)) = self.awaiting.pop_front() else {
            return;
        };
        if let Some(Some(until)) = self.until.get_mut(&e) {
            *until = until.max(now + HOLD_AFTER_EDIT_SECS);
        }
    }

    /// Holds `entity` while it is dragged.
    pub fn hold(&mut self, entity: Entity) {
        self.until.insert(entity, None);
    }

    /// Lets `entity`'s poses through again a moment after `now`.
    pub fn release(&mut self, entity: Entity, now: f64) {
        self.until.insert(entity, Some(now + HOLD_AFTER_EDIT_SECS));
    }
}

/// A model with a plane in it (the ground plane): gz-sim's physics refuses
/// to set its pose (`ModelContainsPlaneCollision`), so the tools leave it.
/// The scene has no collisions; a plane visual stands for one.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HasPlane;

/// Whether the tools and the inspector move `entity`: a model under the
/// world root without a plane (DECISIONS.md D-84). `set_pose` reaches only
/// models (gz-sim's physics applies it; a light's would reply true and
/// stay put), and a link moves with its model.
pub fn movable(entity: &GzEntity, parent: &ChildOf, root: Entity, plane: bool) -> bool {
    !plane && parent.parent() == root && entity.kind == NodeKind::Model
}

/// Whether the connected server takes edits.
pub fn edits_allowed(status: Option<&GzStatus>) -> bool {
    status.is_some_and(|s| s.edits == Some(true))
}

/// The `set_pose` request for `entity` at `pose`.
pub fn set_pose_request(world: &str, entity: &GzEntity, pose: &Transform) -> GzCommand {
    let (t, q) = (pose.translation, pose.rotation);
    let msg = msgs::Pose {
        name: entity.name.clone(),
        id: entity.id,
        position: Some(msgs::Vector3d {
            x: t.x as f64,
            y: t.y as f64,
            z: t.z as f64,
            ..Default::default()
        }),
        orientation: Some(msgs::Quaternion {
            x: q.x as f64,
            y: q.y as f64,
            z: q.z as f64,
            w: q.w as f64,
            ..Default::default()
        }),
        ..Default::default()
    };
    GzCommand::Request {
        service: world_topic(world, "set_pose"),
        msg_type: "gz.msgs.Pose".into(),
        payload: msg.encode_to_vec(),
    }
}

pub struct EditPlugin;

impl Plugin for EditPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SetPose>()
            .add_message::<GzCommand>()
            .add_message::<GzServiceReply>()
            .init_resource::<Held>()
            .add_systems(
                Update,
                (
                    mark_planes,
                    (send_set_poses, answered, expire_holds).chain(),
                ),
            );
    }
}

/// Sends each [`SetPose`] to a server that takes edits (its entity held
/// until the answer); drops it (with a warning) otherwise.
fn send_set_poses(
    mut edits: MessageReader<SetPose>,
    status: Option<Res<GzStatus>>,
    time: Res<Time<Real>>,
    entities: Query<&GzEntity>,
    mut held: ResMut<Held>,
    mut out: MessageWriter<GzCommand>,
) {
    for edit in edits.read() {
        let world = status.as_deref().and_then(|s| s.world.as_deref());
        let (true, Some(world)) = (edits_allowed(status.as_deref()), world) else {
            warn!("pose edit dropped: the server doesn't take edits");
            continue;
        };
        if let Ok(g) = entities.get(edit.entity) {
            out.write(set_pose_request(world, g, &edit.pose));
            held.sent(edit.entity, time.elapsed_secs_f64());
        }
    }
}

/// The server's answers to `set_pose`.
fn answered(
    mut replies: MessageReader<GzServiceReply>,
    time: Res<Time<Real>>,
    mut held: ResMut<Held>,
) {
    for r in replies.read() {
        if r.service.ends_with("/set_pose") {
            held.answered(time.elapsed_secs_f64());
        }
    }
}

/// Marks the top-level model of each new plane visual [`HasPlane`].
fn mark_planes(
    visuals: Query<(Entity, &GzVisual), Added<GzVisual>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<GzWorldRoot>>,
    mut commands: Commands,
) {
    for (mut e, v) in &visuals {
        if !matches!(v.0.geometry, GeometryDesc::Plane { .. }) {
            continue;
        }
        // Up to the world root's child.
        while let Ok(parent) = parents.get(e) {
            if roots.contains(parent.parent()) {
                commands.entity(e).insert(HasPlane);
                break;
            }
            e = parent.parent();
        }
    }
}

/// Forgets holds that have run out, those of entities that are gone, and
/// `set_pose`s the server never answered.
fn expire_holds(time: Res<Time<Real>>, entities: Query<()>, mut held: ResMut<Held>) {
    let now = time.elapsed_secs_f64();
    if held
        .awaiting
        .front()
        .is_some_and(|(_, at)| now - at >= REPLY_TIMEOUT_SECS)
    {
        held.awaiting
            .retain(|(_, at)| now - at < REPLY_TIMEOUT_SECS);
    }
    let stale = |held: &Held, e: &Entity, until: &Option<f64>| {
        entities.get(*e).is_err() || until.is_some_and(|u| now >= u) && !held.awaits(*e, now)
    };
    if held.until.iter().any(|(e, u)| stale(&held, e, u)) {
        let keep: Vec<Entity> = held
            .until
            .iter()
            .filter(|(e, u)| !stale(&held, e, u))
            .map(|(e, _)| *e)
            .collect();
        held.until.retain(|e, _| keep.contains(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gz_core::transport::Status;

    fn app(edits: Option<bool>) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(GzStatus {
                status: Status::Ready,
                world: Some("w".into()),
                edits,
                ..GzStatus::new("ws://test")
            })
            .add_plugins(EditPlugin);
        app
    }

    fn cart(app: &mut App) -> Entity {
        app.world_mut()
            .spawn(GzEntity {
                name: "cart".into(),
                id: 7,
                kind: NodeKind::Model,
                path: "cart".into(),
            })
            .id()
    }

    fn sent(app: &mut App) -> Vec<GzCommand> {
        app.world_mut()
            .resource_mut::<Messages<GzCommand>>()
            .drain()
            .collect()
    }

    #[test]
    fn a_pose_edit_is_a_set_pose_request_to_a_server_that_takes_edits() {
        let mut app = app(Some(true));
        let e = cart(&mut app);
        let pose = Transform::from_xyz(2.0, 0.0, 1.0);
        app.world_mut().write_message(SetPose { entity: e, pose });
        app.update();
        let GzCommand::Request {
            service,
            msg_type,
            payload,
        } = &sent(&mut app)[0]
        else {
            panic!("not a request");
        };
        assert_eq!(
            (service.as_str(), msg_type.as_str()),
            ("/world/w/set_pose", "gz.msgs.Pose")
        );
        let msg = msgs::Pose::decode(payload.as_slice()).unwrap();
        assert_eq!((msg.name.as_str(), msg.id), ("cart", 7));
        let p = msg.position.unwrap();
        assert_eq!((p.x, p.y, p.z), (2.0, 0.0, 1.0));
        assert_eq!(msg.orientation.unwrap().w, 1.0);
        // The payload has zero bytes: only a patched server keeps it whole.
        assert!(payload.contains(&0));
    }

    #[test]
    fn a_server_that_mangles_edits_gets_none() {
        for edits in [None, Some(false)] {
            let mut app = app(edits);
            let e = cart(&mut app);
            app.world_mut().write_message(SetPose {
                entity: e,
                pose: Transform::IDENTITY,
            });
            app.update();
            assert!(sent(&mut app).is_empty());
        }
    }

    #[test]
    fn the_tools_move_top_level_models_without_a_plane() {
        use crate::spawn::{GzVisual, GzWorldRoot};
        use gz_core::scene::{MaterialDesc, VisualDesc};
        let mut app = app(Some(true));
        let w = app.world_mut();
        let root = w.spawn(GzWorldRoot).id();
        let node = |w: &mut World, name: &str, kind, parent| {
            w.spawn((
                GzEntity {
                    name: name.into(),
                    id: 0,
                    kind,
                    path: name.into(),
                },
                ChildOf(parent),
            ))
            .id()
        };
        let cart = node(w, "cart", NodeKind::Model, root);
        let ground = node(w, "ground", NodeKind::Model, root);
        let sun = node(w, "sun", NodeKind::Light, root);
        let link = node(w, "link", NodeKind::Link, ground);
        let visual = |geometry| {
            GzVisual(VisualDesc {
                proto_type: String::new(),
                geometry,
                material: MaterialDesc::absent(),
                cast_shadows: true,
                transparency: 0.0,
            })
        };
        w.spawn((
            visual(GeometryDesc::Plane {
                normal: [0.0, 0.0, 1.0],
                size: [100.0, 100.0],
            }),
            ChildOf(link),
        ));
        w.spawn((visual(GeometryDesc::Box { size: [1.0; 3] }), ChildOf(cart)));
        app.update();
        let w = app.world_mut();
        let mut q = w.query::<(&GzEntity, &ChildOf, Has<HasPlane>)>();
        let mut can = |w: &World, e: Entity| {
            let (g, p, plane) = q.get(w, e).unwrap();
            movable(g, p, root, plane)
        };
        assert!(can(w, cart));
        // The ground plane: physics refuses it. A light: set_pose ignores it.
        // A link moves with its model.
        assert!(!can(w, ground) && !can(w, sun) && !can(w, link));
    }

    #[test]
    fn holds_last_while_dragging_and_a_moment_after() {
        let mut held = Held::default();
        let e = Entity::from_raw_u32(1).unwrap();
        assert!(!held.holds(e, 0.0));
        held.hold(e);
        assert!(held.holds(e, 100.0));
        held.release(e, 10.0);
        assert!(held.holds(e, 10.0 + HOLD_AFTER_EDIT_SECS / 2.0));
        assert!(!held.holds(e, 10.0 + HOLD_AFTER_EDIT_SECS));
        // Sent and unanswered: held past the moment, until the answer and
        // a moment after it, or until the server is given up on.
        held.release(e, 20.0);
        held.sent(e, 20.0);
        assert!(held.holds(e, 21.0));
        held.answered(22.0);
        assert!(held.holds(e, 22.0 + HOLD_AFTER_EDIT_SECS / 2.0));
        assert!(!held.holds(e, 22.0 + HOLD_AFTER_EDIT_SECS));
        held.release(e, 30.0);
        held.sent(e, 30.0);
        assert!(!held.holds(e, 30.0 + REPLY_TIMEOUT_SECS));
        // An answer with nothing sent is no one's.
        held.answered(40.0);
        held.answered(40.0);
    }

    #[test]
    fn a_set_pose_holds_until_its_answer() {
        let mut app = app(Some(true));
        let e = cart(&mut app);
        app.world_mut().resource_mut::<Held>().release(e, 0.0);
        app.world_mut().write_message(SetPose {
            entity: e,
            pose: Transform::IDENTITY,
        });
        app.update();
        let late = HOLD_AFTER_EDIT_SECS + 1.0;
        assert!(app.world().resource::<Held>().holds(e, late));
        // Another service's answer isn't it.
        for service in ["/world/w/control", "/world/w/set_pose"] {
            app.world_mut().write_message(GzServiceReply {
                service: service.into(),
                msg_type: "gz.msgs.Boolean".into(),
                payload: Vec::new(),
            });
            app.update();
        }
        let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
        let held = app.world().resource::<Held>();
        assert!(held.awaiting.is_empty());
        assert!(!held.holds(e, now + HOLD_AFTER_EDIT_SECS));
    }
}
