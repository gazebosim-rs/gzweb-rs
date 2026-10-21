//! The simulation backend (spec §9.4; `.agent/UI_PLAN.md` §3 item 1;
//! DECISIONS.md D-65): what the app talks to the simulation through.
//!
//! Everything above `gz-bevy` (the full UI, the minimal mode's keys, the
//! overlays) asks for things with messages: [`GzCommand`](crate::connection::GzCommand) (sim control,
//! subscriptions, publications, service calls) and
//! [`GzReconnect`](crate::connection::GzReconnect). It
//! reads what came back from resources and messages: [`GzStatus`] (the
//! connection, the world, the stats), [`GzInbox`] (the scene, its updates
//! and deletions, poses), [`GzPublish`] and [`GzServiceReply`].
//! [`pump`](crate::connection::pump) turns the messages into calls on the
//! app's [`Backend`], a `dyn SimBackend`, and the backend writes what it
//! receives into those resources. So nothing above this module knows how
//! the simulation is reached.
//!
//! The one implementation is the Gazebo websocket server
//! ([`WebsocketBackend`](crate::connection::WebsocketBackend)), on both
//! targets; a native gz-transport client (spec Q7) is a later track and
//! would implement the same trait.

use bevy::prelude::*;
use gz_core::transport::SendError;

use crate::connection::{GzInbox, GzPublish, GzServiceReply, GzStatus};
use crate::GzConfig;

/// World control (spec §9.4 `world_control`): the transport's commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldControl {
    Play,
    Pause,
    /// Step a paused simulation by this many iterations.
    Step(u32),
    /// Reset the world and leave it paused.
    Reset,
    /// Stop the simulation server.
    Stop,
}

/// Where a backend writes what it receives, and what it reads its options
/// from, for one frame.
pub struct BackendIo<'a> {
    /// Real time, in seconds.
    pub now: f64,
    pub config: &'a GzConfig,
    /// Written through `Mut`, so a frame with nothing new leaves it
    /// unchanged for the readouts' change detection.
    pub status: Mut<'a, GzStatus>,
    pub inbox: Mut<'a, GzInbox>,
    /// Topic messages for the app, written out after the frame's flush.
    pub publishes: Vec<GzPublish>,
    /// Service replies for the app, written out after the frame's flush.
    pub replies: Vec<GzServiceReply>,
}

/// A way to reach a simulation (spec §9.4: `connect`, `subscribe`,
/// `publish`, `request`, `world_control`). [`pump`](crate::connection::pump)
/// calls it each frame in this order: [`connect`](Self::connect) for each
/// reconnect asked for, [`poll`](Self::poll), the commands, then
/// [`flush`](Self::flush). Not `Send`: the browser's socket is a JS
/// object.
pub trait SimBackend {
    /// Connects again now: to `url`, or to the current server when `None`.
    /// Cancels a pending retry and forgets a rejection.
    fn connect(&mut self, url: Option<&str>, io: &mut BackendIo);
    /// Takes in what arrived (the connection opening or closing, frames)
    /// and retries a dropped connection when it is due.
    fn poll(&mut self, io: &mut BackendIo);
    fn subscribe(&mut self, topic: &str) -> Result<(), SendError>;
    fn unsubscribe(&mut self, topic: &str) -> Result<(), SendError>;
    /// Limits a subscribed topic to `rate` messages per second.
    fn throttle(&mut self, topic: &str, rate: u32) -> Result<(), SendError>;
    /// Publishes a serialized `msg_type` on `topic`.
    fn publish(&mut self, topic: &str, msg_type: &str, payload: &[u8]) -> Result<(), SendError>;
    /// Calls `service` with a serialized `msg_type` request; the reply
    /// comes back as a [`GzServiceReply`].
    fn request(&mut self, service: &str, msg_type: &str, payload: &[u8]) -> Result<(), SendError>;
    fn world_control(&mut self, cmd: WorldControl) -> Result<(), SendError>;
    /// Delivers what was received into `io` and sends what was queued.
    fn flush(&mut self, io: &mut BackendIo);
}

/// The app's backend, a non-send resource.
pub struct Backend(pub Box<dyn SimBackend>);

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use bevy::ecs::system::RunSystemOnce;

    use super::*;
    use crate::connection::{pump, GzCommand, GzReconnect};

    /// Records the calls it gets; its flush delivers one topic message and
    /// one reply.
    struct Recorder(Rc<RefCell<Vec<String>>>);

    impl Recorder {
        fn log(&self, s: String) -> Result<(), SendError> {
            self.0.borrow_mut().push(s);
            Ok(())
        }
    }

    impl SimBackend for Recorder {
        fn connect(&mut self, url: Option<&str>, _io: &mut BackendIo) {
            let _ = self.log(format!("connect {url:?}"));
        }
        fn poll(&mut self, _io: &mut BackendIo) {
            let _ = self.log("poll".into());
        }
        fn subscribe(&mut self, topic: &str) -> Result<(), SendError> {
            self.log(format!("sub {topic}"))
        }
        fn unsubscribe(&mut self, topic: &str) -> Result<(), SendError> {
            self.log(format!("unsub {topic}"))
        }
        fn throttle(&mut self, topic: &str, rate: u32) -> Result<(), SendError> {
            self.log(format!("throttle {topic} {rate}"))
        }
        fn publish(&mut self, topic: &str, msg_type: &str, _: &[u8]) -> Result<(), SendError> {
            self.log(format!("pub {topic} {msg_type}"))?;
            Err(SendError::Unsendable("no"))
        }
        fn request(&mut self, service: &str, _: &str, _: &[u8]) -> Result<(), SendError> {
            self.log(format!("req {service}"))
        }
        fn world_control(&mut self, cmd: WorldControl) -> Result<(), SendError> {
            self.log(format!("{cmd:?}"))
        }
        fn flush(&mut self, io: &mut BackendIo) {
            let _ = self.log("flush".into());
            if io.now >= 0.0 && self.0.borrow().len() > 3 {
                io.publishes.push(GzPublish {
                    topic: "/t".into(),
                    msg_type: "gz.msgs.Empty".into(),
                    payload: vec![],
                });
                io.replies.push(GzServiceReply {
                    service: "/s".into(),
                    msg_type: "gz.msgs.Boolean".into(),
                    payload: vec![],
                });
            }
        }
    }

    /// `pump` drives any backend: reconnects, poll, the commands mapped to
    /// the trait's calls, flush; what the backend delivers comes out as
    /// messages, and a frame with nothing new leaves the status unchanged.
    #[test]
    fn pump_drives_the_backend_through_the_trait() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut world = World::new();
        world.insert_non_send(Backend(Box::new(Recorder(calls.clone()))));
        world.insert_resource(GzConfig::default());
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(GzStatus::new("ws://test"));
        world.init_resource::<GzInbox>();
        world.init_resource::<Messages<GzCommand>>();
        world.init_resource::<Messages<GzReconnect>>();
        world.init_resource::<Messages<GzPublish>>();
        world.init_resource::<Messages<GzServiceReply>>();
        world.write_message(GzReconnect {
            url: Some("ws://other".into()),
        });
        for cmd in [
            GzCommand::Play,
            GzCommand::Pause,
            GzCommand::Step(10),
            GzCommand::Reset,
            GzCommand::Stop,
            GzCommand::Subscribe("/a".into()),
            GzCommand::Unsubscribe("/a".into()),
            GzCommand::Throttle("/a".into(), 5),
            GzCommand::Publish {
                topic: "/cmd_vel".into(),
                msg_type: "gz.msgs.Twist".into(),
                payload: vec![],
            },
            GzCommand::Request {
                service: "/world/w/create".into(),
                msg_type: "gz.msgs.EntityFactory".into(),
                payload: vec![],
            },
        ] {
            world.write_message(cmd);
        }
        world.run_system_once(pump).unwrap();
        assert_eq!(
            *calls.borrow(),
            [
                "connect Some(\"ws://other\")",
                "poll",
                "Play",
                "Pause",
                "Step(10)",
                "Reset",
                "Stop",
                "sub /a",
                "unsub /a",
                "throttle /a 5",
                "pub /cmd_vel gz.msgs.Twist",
                "req /world/w/create",
                "flush",
            ]
        );
        let published: Vec<_> = world
            .resource_mut::<Messages<GzPublish>>()
            .drain()
            .map(|p| p.topic)
            .collect();
        assert_eq!(published, ["/t"]);
        assert_eq!(world.resource::<Messages<GzServiceReply>>().len(), 1);
        // Nothing new: the status isn't marked changed.
        world.clear_trackers();
        world.run_system_once(pump).unwrap();
        assert!(!world.resource_ref::<GzStatus>().is_changed());
    }
}
