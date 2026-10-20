//! What the shell makes of the connection (spec §3.3.1, §3.6, §7.10):
//! [`Phase`] from `gz-bevy`'s `GzStatus` and scene, the banner's state
//! after the connection drops, and the world's name.

use bevy::prelude::*;
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_bevy::spawn::GzIndex;
use gz_core::transport::Status;

use crate::theme::ColorRole;

/// The connection, as the UI shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// No world has loaded and nothing is being tried (a rejected key or
    /// URL, or reconnecting is off).
    Idle,
    /// No world yet; a connection is being tried or will be.
    Connecting,
    /// Connected, waiting for the first scene.
    Loading,
    /// A world is loaded.
    Live,
    /// A world was loaded and the connection dropped, at `since` (real
    /// seconds).
    Lost { since: f64 },
}

impl Phase {
    /// No world has loaded since the last connect: the connect card shows.
    pub fn no_world(self) -> bool {
        matches!(self, Phase::Idle | Phase::Connecting | Phase::Loading)
    }
}

/// After this long lost, the banner turns `danger_soft` and offers
/// another server (§7.10).
pub(crate) const LOST_LONG_SECS: f64 = 60.0;

/// The connection memory behind [`Phase`].
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConnectionView {
    /// A world loaded since the last connect to a new address.
    pub had_world: bool,
    /// When the loaded world's connection dropped.
    pub lost_at: Option<f64>,
    /// The user asked for the connect card while lost ("Connect to another
    /// server…").
    pub card_requested: bool,
}

impl ConnectionView {
    /// Updates the memory from the status at real time `now`, and returns
    /// the phase.
    pub(crate) fn update(&mut self, status: Option<&GzStatus>, loaded: bool, now: f64) -> Phase {
        let Some(status) = status else {
            return Phase::Idle;
        };
        let ready = status.status == Status::Ready;
        if ready && loaded {
            self.had_world = true;
            self.lost_at = None;
            self.card_requested = false;
            return Phase::Live;
        }
        if self.had_world {
            let since = *self.lost_at.get_or_insert(now);
            return Phase::Lost { since };
        }
        match &status.status {
            Status::Ready | Status::Connected => Phase::Loading,
            _ if status.attempting || status.reconnect_at.is_some() => Phase::Connecting,
            _ => Phase::Idle,
        }
    }

    /// A connect to a new address starts over: the card, not the banner.
    pub(crate) fn connect_elsewhere(&mut self) {
        *self = Self::default();
    }
}

/// The connection's [`Phase`] this frame (kept by `track_connection`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CurrentPhase(pub Phase);

impl Default for CurrentPhase {
    fn default() -> Self {
        CurrentPhase(Phase::Idle)
    }
}

/// Keeps [`ConnectionView`] and [`CurrentPhase`] up to date.
pub(crate) fn track_connection(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    time: Res<Time<Real>>,
    mut view: ResMut<ConnectionView>,
    mut phase: ResMut<CurrentPhase>,
) {
    let loaded = index.is_some_and(|i| i.scene.is_some());
    let now = time.elapsed_secs_f64();
    let p = view.update(status.as_deref(), loaded, now);
    #[cfg(target_arch = "wasm32")]
    if p == Phase::Live && phase.0 != Phase::Live {
        if let Some(s) = &status {
            gz_bevy::view::web::remember_url(&s.url);
        }
    }
    phase.set_if_neq(CurrentPhase(p));
}

/// The status bar's connection dot (§3.6 item 1): `ok` connected, `warn`
/// reconnecting or connecting, `danger` lost with nothing being tried.
pub(crate) fn dot_role(phase: Phase, status: Option<&GzStatus>) -> ColorRole {
    let trying = status.is_some_and(|s| s.attempting || s.reconnect_at.is_some());
    match phase {
        Phase::Live | Phase::Loading => ColorRole::Ok,
        _ if trying => ColorRole::Warn,
        _ => ColorRole::Danger,
    }
}

/// The disconnected banner (§7.10).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Banner {
    pub text: String,
    /// Lost for a minute: `danger_soft`, and "Connect to another server…".
    pub long: bool,
}

impl Banner {
    /// Fill and text roles.
    pub(crate) fn roles(&self) -> (ColorRole, ColorRole) {
        if self.long {
            (ColorRole::DangerSoft, ColorRole::Danger)
        } else {
            (ColorRole::WarnSoft, ColorRole::WarnText)
        }
    }
}

/// The banner for `phase` at real time `now`, if one shows.
pub(crate) fn banner(phase: Phase, status: Option<&GzStatus>, now: f64) -> Option<Banner> {
    let Phase::Lost { since } = phase else {
        return None;
    };
    let status = status?;
    let addr = crate::widgets::format::address(&status.url);
    let trying = status.attempting || status.reconnect_at.is_some();
    let text = if trying {
        format!(
            "Lost connection to {addr} · Reconnecting… ({})",
            status.attempts.max(1)
        )
    } else {
        format!("Lost connection to {addr}")
    };
    Some(Banner {
        text,
        long: now - since >= LOST_LONG_SECS,
    })
}

/// "Retry now" and Reconnect: try the current server again at once.
pub(crate) fn retry(out: &mut MessageWriter<GzReconnect>) {
    out.write(GzReconnect::default());
}

/// The world's name: the server's world, else the scene's.
pub fn world_name(status: Option<&GzStatus>, index: Option<&GzIndex>) -> Option<String> {
    status
        .and_then(|s| s.world.clone())
        .or_else(|| index.and_then(|i| i.scene.as_ref().map(|s| s.name.clone())))
        .filter(|n| !n.is_empty())
}
