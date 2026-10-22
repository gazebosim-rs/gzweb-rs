//! Joint states for the full UI's Joint inspector (spec §3.5: position,
//! velocity and effort, live): while [`JointStateWanted`] names a model,
//! its `joint_state` topic (or the `<topic>` its JointStatePublisher sets)
//! is subscribed (Gazebo's JointStatePublisher system publishes it, when
//! the world has one) and decoded into [`JointStates`]. Nothing is
//! subscribed otherwise.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gz_core::msgs;
use gz_core::topic::world_topic;
use gz_core::transport::Status;
use prost::Message as _;

use crate::connection::{GzCommand, GzPublish, GzStatus};

/// How many joint states a second the inspector needs.
pub const JOINT_STATE_RATE: u32 = 10;

/// The model whose joint states are wanted, if any.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct JointStateWanted {
    /// Its scene path.
    pub model: Option<String>,
    /// The `<topic>` its JointStatePublisher sets, used as is; else the
    /// default ([`joint_state_topic`]).
    pub topic: Option<String>,
}

/// One joint's state, from its first axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointState {
    pub position: f64,
    pub velocity: f64,
    pub effort: f64,
}

/// The latest joint states of the wanted model, by joint name; empty until
/// the first message (or with no publisher).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct JointStates {
    /// The topic subscribed to.
    pub topic: Option<String>,
    pub joints: HashMap<String, JointState>,
}

/// The topic JointStatePublisher publishes a model's joint states on
/// (`/world/<world>/model/<model>/joint_state`; a nested model's path
/// `outer/inner` is `model/outer/model/inner`).
pub fn joint_state_topic(world: &str, model: &str) -> String {
    world_topic(
        world,
        &format!("model/{}/joint_state", model.replace('/', "/model/")),
    )
}

pub struct JointStatePlugin;

impl Plugin for JointStatePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GzCommand>()
            .add_message::<GzPublish>()
            .init_resource::<JointStateWanted>()
            .init_resource::<JointStates>()
            .add_systems(Update, (subscribe_joint_states, read_joint_states).chain());
    }
}

/// Subscribes to the wanted model's topic (throttled) and drops the last
/// one's. While the connection isn't ready it forgets the subscription, so
/// a reconnect subscribes again.
fn subscribe_joint_states(
    wanted: Res<JointStateWanted>,
    status: Option<Res<GzStatus>>,
    mut states: ResMut<JointStates>,
    mut out: MessageWriter<GzCommand>,
) {
    if !wanted.is_changed() && !status.as_ref().is_some_and(|s| s.is_changed()) {
        return;
    }
    let ready = status
        .as_deref()
        .filter(|s| s.status == Status::Ready)
        .and_then(|s| s.world.as_deref());
    let want = match (ready, &wanted.model) {
        (Some(world), Some(model)) => Some(
            wanted
                .topic
                .clone()
                .unwrap_or_else(|| joint_state_topic(world, model)),
        ),
        _ => None,
    };
    if want == states.topic {
        return;
    }
    if let (Some(old), Some(_)) = (&states.topic, ready) {
        out.write(GzCommand::Unsubscribe(old.clone()));
    }
    if let Some(topic) = &want {
        out.write(GzCommand::Subscribe(topic.clone()));
        out.write(GzCommand::Throttle(topic.clone(), JOINT_STATE_RATE));
    }
    states.topic = want;
    states.joints.clear();
}

/// Decodes the subscribed topic's `gz.msgs.Model` messages.
fn read_joint_states(mut publishes: MessageReader<GzPublish>, mut states: ResMut<JointStates>) {
    for p in publishes.read() {
        if states.topic.as_deref() != Some(p.topic.as_str()) {
            continue;
        }
        let Ok(model) = msgs::Model::decode(p.payload.as_slice()) else {
            warn!("joint states on {}: not a gz.msgs.Model", p.topic);
            continue;
        };
        for joint in &model.joint {
            if let Some(axis) = &joint.axis1 {
                states.joints.insert(
                    joint.name.clone(),
                    JointState {
                        position: axis.position,
                        velocity: axis.velocity,
                        effort: axis.force,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<GzCommand>()
            .add_message::<GzPublish>()
            .insert_resource(GzStatus {
                status: Status::Ready,
                world: Some("w".into()),
                ..GzStatus::new("ws://test")
            })
            .add_plugins(JointStatePlugin);
        app
    }

    fn sent(app: &mut App) -> Vec<GzCommand> {
        let mut m = app.world_mut().resource_mut::<Messages<GzCommand>>();
        m.drain().collect()
    }

    #[test]
    fn the_wanted_models_topic_is_subscribed_and_decoded() {
        assert_eq!(
            joint_state_topic("w", "outer/inner"),
            "/world/w/model/outer/model/inner/joint_state"
        );
        let mut app = app();
        app.update();
        assert!(sent(&mut app).is_empty(), "nothing wanted");
        app.world_mut().resource_mut::<JointStateWanted>().model = Some("arm".into());
        app.update();
        let topic = "/world/w/model/arm/joint_state".to_string();
        assert_eq!(
            sent(&mut app),
            [
                GzCommand::Subscribe(topic.clone()),
                GzCommand::Throttle(topic.clone(), JOINT_STATE_RATE)
            ]
        );
        let msg = msgs::Model {
            name: "arm".into(),
            joint: vec![msgs::Joint {
                name: "elbow".into(),
                axis1: Some(msgs::Axis {
                    position: 0.5,
                    velocity: -1.0,
                    force: 2.0,
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        for t in [topic.clone(), "/elsewhere".to_string()] {
            app.world_mut().write_message(GzPublish {
                topic: t,
                msg_type: "gz.msgs.Model".into(),
                payload: msg.encode_to_vec(),
            });
        }
        app.update();
        let states = app.world().resource::<JointStates>();
        assert_eq!(
            states.joints.get("elbow"),
            Some(&JointState {
                position: 0.5,
                velocity: -1.0,
                effort: 2.0
            })
        );
        // Unwanted: unsubscribed, forgotten.
        app.world_mut().resource_mut::<JointStateWanted>().model = None;
        app.update();
        assert_eq!(sent(&mut app), [GzCommand::Unsubscribe(topic)]);
        assert!(app.world().resource::<JointStates>().joints.is_empty());
    }

    #[test]
    fn a_publishers_own_topic_is_used_as_is() {
        let mut app = app();
        *app.world_mut().resource_mut::<JointStateWanted>() = JointStateWanted {
            model: Some("arm".into()),
            topic: Some("/arm/joints".into()),
        };
        app.update();
        assert_eq!(
            sent(&mut app)[0],
            GzCommand::Subscribe("/arm/joints".into())
        );
    }

    #[test]
    fn a_reconnect_or_a_new_world_subscribes_again() {
        let mut app = app();
        app.world_mut().resource_mut::<JointStateWanted>().model = Some("arm".into());
        app.update();
        sent(&mut app);
        // Down: the transport has dropped its subscriptions; nothing to
        // unsubscribe.
        app.world_mut().resource_mut::<GzStatus>().status = Status::Connecting;
        app.update();
        assert!(sent(&mut app).is_empty());
        assert_eq!(app.world().resource::<JointStates>().topic, None);
        // Back, in another world.
        let mut status = app.world_mut().resource_mut::<GzStatus>();
        status.status = Status::Ready;
        status.world = Some("v".into());
        app.update();
        assert_eq!(
            sent(&mut app)[0],
            GzCommand::Subscribe("/world/v/model/arm/joint_state".into())
        );
    }
}
