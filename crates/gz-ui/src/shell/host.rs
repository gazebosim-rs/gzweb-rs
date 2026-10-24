//! Where panels show (DECISIONS.md D-98): a [`PanelHost`] is a node that
//! holds one panel's section header, body and footer. Today the hosts are
//! the sidebar's two slots; a pop-out window (M7e) will be one more. The
//! sidebar rebuilds each host from the panel it holds
//! ([`rebuild_hosts`](super::sidebar::rebuild_hosts)), and
//! [`ShownPanels`] says which panels the hosts hold and which of those
//! are on screen: the activity bar's active state, the header actions'
//! keys and [`panel_shown`](super::panel_shown) read it, so none of them
//! knows what a slot is.

use bevy::prelude::*;

use crate::panels::PanelId;

/// A node that holds a panel: the panel it holds, and whether it is on
/// screen (a slot of a collapsed sidebar holds its panel but doesn't show
/// it). Its owner writes both.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PanelHost {
    pub panel: Option<PanelId>,
    pub shown: bool,
}

/// The panels the hosts hold, and which of them are on screen.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct ShownPanels {
    hosted: Vec<PanelId>,
    shown: Vec<PanelId>,
}

impl ShownPanels {
    /// A host holds `panel`, shown or not (its activity button's active
    /// state, §3.2).
    pub fn is_hosted(&self, panel: PanelId) -> bool {
        self.hosted.contains(&panel)
    }

    /// `panel` is on screen.
    pub fn is_shown(&self, panel: PanelId) -> bool {
        self.shown.contains(&panel)
    }
}

/// Keeps [`ShownPanels`] in step with the hosts.
pub(crate) fn track_hosts(
    hosts: Query<&PanelHost>,
    changed: Query<(), Changed<PanelHost>>,
    mut removed: RemovedComponents<PanelHost>,
    mut panels: ResMut<ShownPanels>,
) {
    if changed.is_empty() && removed.read().count() == 0 {
        return;
    }
    let mut next = ShownPanels::default();
    for host in &hosts {
        if let Some(p) = host.panel {
            next.hosted.push(p);
            if host.shown {
                next.shown.push(p);
            }
        }
    }
    panels.set_if_neq(next);
}
