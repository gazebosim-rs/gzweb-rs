//! The server's topics, shared subscriptions and message rates
//! (DECISIONS.md D-97): what the full UI's Topics panel lists and echoes,
//! and what any part of the app subscribes through.
//!
//! - [`GzTopics`]: every topic the server has, with its message type,
//!   from the handshake's `topics-types` and each
//!   [`GzCommand::RefreshTopics`] after.
//! - [`GzSubscriptions`]: who holds which topic. A topic is subscribed
//!   while anyone holds it and unsubscribed once nobody does, so the
//!   Topics panel's echo and a Plot series on the same topic don't undo
//!   each other. [`sync_subscriptions`] sends the difference, and again
//!   after a reconnect (the server forgets a connection's
//!   subscriptions). Holding a topic the client consumes itself (the
//!   world's poses, scene, stats) taps it: its messages come out as
//!   [`GzPublish`] too, and releasing it never unsubscribes the client.
//! - [`GzTraffic`]: how many messages each topic the client receives has
//!   brought and at what rate, measured each second.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gz_core::transport::Status;
use std::collections::BTreeMap;

use crate::connection::{GzCommand, GzPublish, GzStatus};

/// One of the server's topics.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TopicInfo {
    pub name: String,
    /// Its message type (`gz.msgs.Pose_V`).
    pub msg_type: String,
}

/// The server's topics, by name, as of its last `topics-types` reply.
/// Empty until the first; kept across a drop (the connection's phase says
/// whether it is current).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GzTopics {
    pub list: Vec<TopicInfo>,
    /// Replies so far: the list arrived at least once when non-zero.
    pub replies: u32,
}

impl GzTopics {
    /// Replaces the list with a reply's (topic, type) pairs, sorted by name.
    pub fn set(&mut self, pairs: Vec<(String, String)>) {
        let mut list: Vec<TopicInfo> = pairs
            .into_iter()
            .map(|(name, msg_type)| TopicInfo { name, msg_type })
            .collect();
        list.sort();
        list.dedup_by(|a, b| a.name == b.name);
        self.list = list;
        self.replies += 1;
    }

    pub fn get(&self, name: &str) -> Option<&TopicInfo> {
        self.list
            .binary_search_by(|t| t.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.list[i])
    }
}

/// Who holds a topic: a short, stable name for the part of the app
/// (`"topics.echo"`, `"joint_state"`).
pub type Holder = &'static str;

/// The topics the app wants and who wants them, each holder with the rate
/// it needs (messages a second; `None`: every message).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GzSubscriptions {
    holds: BTreeMap<String, BTreeMap<Holder, Option<u32>>>,
    /// What was sent on this connection: topic → rate.
    sent: BTreeMap<String, Option<u32>>,
}

impl GzSubscriptions {
    /// `holder` wants `topic` at `rate` (none: unthrottled).
    pub fn hold(&mut self, holder: Holder, topic: &str, rate: Option<u32>) {
        let holders = self.holds.entry(topic.to_string()).or_default();
        if holders.get(holder) != Some(&rate) {
            holders.insert(holder, rate);
        }
    }

    /// `holder` no longer wants `topic`.
    pub fn release(&mut self, holder: Holder, topic: &str) {
        if let Some(holders) = self.holds.get_mut(topic) {
            holders.remove(holder);
            if holders.is_empty() {
                self.holds.remove(topic);
            }
        }
    }

    /// The topics `holder` holds.
    pub fn held_by(&self, holder: Holder) -> impl Iterator<Item = &str> {
        self.holds
            .iter()
            .filter(move |(_, h)| h.contains_key(holder))
            .map(|(t, _)| t.as_str())
    }

    /// Whether anyone holds `topic`.
    pub fn is_held(&self, topic: &str) -> bool {
        self.holds.contains_key(topic)
    }

    /// The rate `topic` is subscribed at: unthrottled if any holder wants
    /// every message, else the highest any holder asked for.
    fn rate(holders: &BTreeMap<Holder, Option<u32>>) -> Option<u32> {
        holders.values().try_fold(0, |max, r| r.map(|r| r.max(max)))
    }
}

/// Sends what changed in [`GzSubscriptions`]: subscribes the topics newly
/// held (throttled where every holder throttles), re-throttles those whose
/// rate changed, unsubscribes those released. While the connection isn't
/// ready it forgets what it sent, so it subscribes everything again once
/// it is.
pub fn sync_subscriptions(
    status: Option<Res<GzStatus>>,
    mut subs: ResMut<GzSubscriptions>,
    mut out: MessageWriter<GzCommand>,
) {
    let ready = status.as_ref().is_some_and(|s| s.status == Status::Ready);
    let status_changed = status.as_ref().is_some_and(|s| s.is_changed());
    if !subs.is_changed() && !status_changed {
        return;
    }
    let subs = subs.bypass_change_detection();
    if !ready {
        subs.sent.clear();
        return;
    }
    let want: BTreeMap<String, Option<u32>> = subs
        .holds
        .iter()
        .map(|(t, h)| (t.clone(), GzSubscriptions::rate(h)))
        .collect();
    for topic in subs.sent.keys().filter(|t| !want.contains_key(*t)) {
        out.write(GzCommand::Unsubscribe(topic.clone()));
    }
    for (topic, rate) in &want {
        let sent = subs.sent.get(topic);
        if sent == Some(rate) {
            continue;
        }
        match sent {
            None => {
                out.write(GzCommand::Subscribe(topic.clone()));
            }
            // From throttled to every message: the server has no unthrottle.
            Some(Some(_)) if rate.is_none() => {
                out.write(GzCommand::Unsubscribe(topic.clone()));
                out.write(GzCommand::Subscribe(topic.clone()));
            }
            Some(_) => {}
        }
        if let Some(r) = rate {
            out.write(GzCommand::Throttle(topic.clone(), *r));
        }
    }
    subs.sent = want;
}

/// One topic's traffic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Traffic {
    /// Messages since start.
    pub total: u64,
    /// When the last arrived (`Time<Real>` seconds).
    pub last_at: f64,
    /// Messages a second over the last full second; `None` before one.
    pub rate: Option<f32>,
    /// The window being measured: its start and the total then.
    window: (f64, u64),
}

/// The messages each topic the client receives has brought, its own
/// world topics included.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GzTraffic {
    pub topics: HashMap<String, Traffic>,
}

impl GzTraffic {
    /// A message on `topic` arrived at `now`.
    pub fn arrived(&mut self, topic: &str, now: f64) {
        let t = match self.topics.get_mut(topic) {
            Some(t) => t,
            None => self.topics.entry(topic.to_string()).or_insert(Traffic {
                window: (now, 0),
                ..default()
            }),
        };
        t.total += 1;
        t.last_at = now;
    }

    pub fn rate(&self, topic: &str) -> Option<f32> {
        self.topics.get(topic).and_then(|t| t.rate)
    }

    /// Updates the rate of each topic whose window has run
    /// [`RATE_WINDOW`] by `now`, and starts its next.
    pub fn measure(&mut self, now: f64) {
        for t in self.topics.values_mut() {
            let (start, total) = t.window;
            let dt = now - start;
            if dt >= RATE_WINDOW {
                t.rate = Some(((t.total - total) as f64 / dt) as f32);
                t.window = (now, t.total);
            }
        }
    }
}

/// How long [`measure_rates`] counts before it updates a rate.
pub const RATE_WINDOW: f64 = 1.0;

/// Each second, each topic's rate over that second.
pub fn measure_rates(time: Res<Time<Real>>, mut traffic: ResMut<GzTraffic>) {
    let now = time.elapsed_secs_f64();
    let due = traffic
        .topics
        .values()
        .any(|t| now - t.window.0 >= RATE_WINDOW);
    if due {
        traffic.measure(now);
    }
}

/// The topic list, shared subscriptions and rates. Added by
/// [`GzWebPlugin`](crate::GzWebPlugin) and by every plugin that holds
/// topics, whichever comes first.
pub struct TopicsPlugin;

impl Plugin for TopicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GzCommand>()
            .add_message::<GzPublish>()
            .init_resource::<GzTopics>()
            .init_resource::<GzSubscriptions>()
            .init_resource::<GzTraffic>()
            .add_systems(
                Update,
                (
                    sync_subscriptions.before(crate::connection::pump),
                    measure_rates,
                ),
            );
    }
}

impl TopicsPlugin {
    /// Adds the plugin unless it is already there.
    pub fn ensure(app: &mut App) {
        if !app.is_plugin_added::<Self>() {
            app.add_plugins(Self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(TopicsPlugin)
            .insert_resource(GzStatus {
                status: Status::Ready,
                world: Some("w".into()),
                ..GzStatus::new("ws://test")
            });
        app
    }

    fn sent(app: &mut App) -> Vec<GzCommand> {
        let mut m = app.world_mut().resource_mut::<Messages<GzCommand>>();
        m.drain().collect()
    }

    fn subs(app: &mut App) -> Mut<'_, GzSubscriptions> {
        app.world_mut().resource_mut::<GzSubscriptions>()
    }

    use GzCommand::{Subscribe as Sub, Throttle, Unsubscribe as Unsub};

    /// A topic is subscribed while anyone holds it, at the highest rate a
    /// holder asks for (unthrottled if one wants every message), and
    /// unsubscribed once the last holder lets go.
    #[test]
    fn a_topic_is_subscribed_while_anyone_holds_it() {
        let mut app = app();
        let t = "/imu".to_string();
        subs(&mut app).hold("plot", &t, Some(10));
        app.update();
        assert_eq!(sent(&mut app), [Sub(t.clone()), Throttle(t.clone(), 10)]);
        subs(&mut app).hold("joints", &t, Some(20));
        app.update();
        assert_eq!(sent(&mut app), [Throttle(t.clone(), 20)]);
        subs(&mut app).hold("echo", &t, None);
        app.update();
        assert_eq!(sent(&mut app), [Unsub(t.clone()), Sub(t.clone())]);
        subs(&mut app).release("echo", &t);
        subs(&mut app).release("joints", &t);
        app.update();
        assert_eq!(sent(&mut app), [Throttle(t.clone(), 10)]);
        assert_eq!(subs(&mut app).held_by("plot").collect::<Vec<_>>(), ["/imu"]);
        subs(&mut app).release("plot", &t);
        app.update();
        assert_eq!(sent(&mut app), [Unsub(t.clone())]);
        assert!(!subs(&mut app).is_held(&t));
        app.update();
        assert!(sent(&mut app).is_empty());
    }

    /// A drop forgets what was sent: nothing to unsubscribe, and every
    /// held topic is subscribed again once the connection is ready.
    #[test]
    fn a_reconnect_subscribes_the_held_topics_again() {
        let mut app = app();
        subs(&mut app).hold("echo", "/a", None);
        app.update();
        assert_eq!(sent(&mut app), [Sub("/a".into())]);
        app.world_mut().resource_mut::<GzStatus>().status = Status::Connecting;
        app.update();
        assert!(sent(&mut app).is_empty());
        app.world_mut().resource_mut::<GzStatus>().status = Status::Ready;
        app.update();
        assert_eq!(sent(&mut app), [Sub("/a".into())]);
    }

    /// The list is sorted and replaced whole; a rate is the messages over
    /// the last full second.
    #[test]
    fn the_list_and_the_rates() {
        let mut topics = GzTopics::default();
        topics.set(vec![
            ("/b".into(), "gz.msgs.Clock".into()),
            ("/a".into(), "gz.msgs.Pose".into()),
        ]);
        assert_eq!(topics.list[0].name, "/a");
        assert_eq!(topics.get("/b").unwrap().msg_type, "gz.msgs.Clock");
        assert_eq!(topics.get("/c"), None);
        topics.set(vec![]);
        assert_eq!((topics.list.len(), topics.replies), (0, 2));

        let mut traffic = GzTraffic::default();
        for i in 0..5 {
            traffic.arrived("/a", 0.1 * i as f64);
        }
        assert_eq!(traffic.rate("/a"), None);
        traffic.measure(0.5);
        assert_eq!(traffic.rate("/a"), None, "not a full second yet");
        traffic.measure(1.0);
        assert_eq!(traffic.rate("/a"), Some(5.0));
        traffic.measure(2.0);
        assert_eq!(traffic.rate("/a"), Some(0.0), "quiet since");
        assert_eq!(traffic.topics["/a"].total, 5);
    }
}
