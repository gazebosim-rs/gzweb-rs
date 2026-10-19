//! Simulation control requests (gzweb `SceneManager.play/pause/stop`), as
//! `(service, msg_type, payload)` for [`crate::transport::Transport::request`].
//!
//! Every request carries a header stamped with 1 s. gz-sim ignores it, but it
//! keeps the payload non-empty and free of `0x00` bytes, which the Gazebo
//! websocket server can't carry (see [`crate::transport`]):
//! `WorldControl { pause: false }` alone encodes to nothing. Step counts
//! whose varint contains a `0x00` or `,` byte are split into several
//! requests. gzweb sends `ignition.msgs.*` type names, which Harmonic and
//! Jetty reject; these use `gz.msgs.*`. The service name is built from the
//! sanitized world name, as gz-sim does ([`crate::topic::world_topic`]).

use prost::Message;

use gz_proto::msgs;

/// A service call: service name, request type, serialized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub service: String,
    pub msg_type: &'static str,
    pub payload: Vec<u8>,
}

fn header() -> Option<msgs::Header> {
    Some(msgs::Header {
        stamp: Some(msgs::Time { sec: 1, nsec: 0 }),
        ..Default::default()
    })
}

fn world_control(world: &str, control: msgs::WorldControl) -> Request {
    Request {
        service: crate::topic::world_topic(world, "control"),
        msg_type: "gz.msgs.WorldControl",
        payload: msgs::WorldControl {
            header: header(),
            ..control
        }
        .encode_to_vec(),
    }
}

/// Run the simulation.
pub fn play(world: &str) -> Request {
    world_control(world, msgs::WorldControl::default())
}

/// Pause the simulation.
pub fn pause(world: &str) -> Request {
    world_control(
        world,
        msgs::WorldControl {
            pause: true,
            ..Default::default()
        },
    )
}

/// Advance a paused simulation by `steps` iterations (at least 1). Usually
/// one request; more when a count would encode with a byte the Gazebo
/// websocket server can't carry (44, for instance, is `0x2c`), so the counts
/// add up to `steps`.
pub fn step(world: &str, steps: u32) -> Vec<Request> {
    let one = |n: u32| {
        world_control(
            world,
            msgs::WorldControl {
                pause: true,
                multi_step: n,
                ..Default::default()
            },
        )
    };
    let mut left = steps.max(1);
    let mut out = Vec::new();
    while left > 0 {
        // The largest sendable count; 1 always is.
        let req = (1..=left)
            .rev()
            .map(one)
            .find(|r| sendable(&r.payload))
            .unwrap_or_else(|| one(1));
        left -= msgs::WorldControl::decode(req.payload.as_slice()).map_or(1, |c| c.multi_step);
        out.push(req);
    }
    out
}

fn sendable(payload: &[u8]) -> bool {
    !payload.iter().any(|&b| b == 0 || b == b',')
}

/// Reset the world (time and models) and leave it paused.
pub fn reset(world: &str) -> Request {
    world_control(
        world,
        msgs::WorldControl {
            pause: true,
            reset: Some(msgs::WorldReset {
                all: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

/// Stop the simulation server (gzweb `stop`).
pub fn stop() -> Request {
    Request {
        service: "/server_control".into(),
        msg_type: "gz.msgs.ServerControl",
        payload: msgs::ServerControl {
            header: header(),
            stop: true,
            ..Default::default()
        }
        .encode_to_vec(),
    }
}

/// The `data` of a `gz.msgs.Boolean` service reply, if it is one.
pub fn reply_ok(msg_type: &str, payload: &[u8]) -> Option<bool> {
    if msg_type != "gz.msgs.Boolean" {
        return None;
    }
    msgs::Boolean::decode(payload).ok().map(|b| b.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Status, Transport};

    fn decode(r: &Request) -> msgs::WorldControl {
        msgs::WorldControl::decode(r.payload.as_slice()).unwrap()
    }

    #[test]
    fn world_control_requests() {
        let p = play("shapes");
        assert_eq!(p.service, "/world/shapes/control");
        // gz-sim serves the control service under the sanitized name.
        assert_eq!(pause("my world").service, "/world/my_world/control");
        assert_eq!(p.msg_type, "gz.msgs.WorldControl");
        // The bytes probed against a live Harmonic server.
        assert_eq!(p.payload, [0x0a, 0x04, 0x0a, 0x02, 0x08, 0x01]);
        assert!(!decode(&p).pause);
        assert!(decode(&pause("w")).pause);
        let s = step("w", 0);
        assert_eq!(s.len(), 1);
        assert!(decode(&s[0]).pause && decode(&s[0]).multi_step == 1);
        assert_eq!(decode(&step("w", 200)[0]).multi_step, 200);
        // 44 is a ',' byte: split.
        let counts: Vec<u32> = step("w", 44).iter().map(|r| decode(r).multi_step).collect();
        assert_eq!(counts, [43, 1]);
        let r = decode(&reset("w"));
        assert!(r.pause && r.reset.unwrap().all);
        let stop = stop();
        assert_eq!(stop.service, "/server_control");
        assert!(
            msgs::ServerControl::decode(stop.payload.as_slice())
                .unwrap()
                .stop
        );
    }

    #[test]
    fn every_request_is_sendable() {
        let mut t = Transport::new(None);
        t.on_open();
        t.on_message(b"protos");
        assert_eq!(t.status(), &Status::Connected);
        for r in [play("w"), pause("w"), reset("w"), stop()] {
            t.request(&r.service, r.msg_type, &r.payload).unwrap();
        }
        // Every step count, including multi-byte varints with a 0x2c or 0x00.
        for n in (0..=20_000).chain([5632, 1 << 20, u32::MAX]) {
            let reqs = step("w", n);
            let total: u64 = reqs.iter().map(|r| u64::from(decode(r).multi_step)).sum();
            assert_eq!(total, u64::from(n.max(1)), "{n}");
            for r in &reqs {
                t.request(&r.service, r.msg_type, &r.payload).unwrap();
            }
            while t.poll_outgoing().is_some() {}
        }
    }

    #[test]
    fn boolean_replies() {
        let yes = msgs::Boolean {
            data: true,
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(reply_ok("gz.msgs.Boolean", &yes), Some(true));
        assert_eq!(reply_ok("gz.msgs.Boolean", &[]), Some(false));
        assert_eq!(reply_ok("gz.msgs.Boolean", &[0xff]), None);
        assert_eq!(reply_ok("gz.msgs.StringMsg", &yes), None);
    }
}
