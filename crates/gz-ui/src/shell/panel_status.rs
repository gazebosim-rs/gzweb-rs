//! What each panel shows on the shell's chrome (spec §3.2, §3.3): its
//! activity button's badge and its section header's count. The shell owns
//! [`PanelStatus`] and draws it; each panel's own systems write its entry,
//! so the shell needs to know nothing about a panel (DECISIONS.md D-57).

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::panels::PanelId;
use crate::widgets::BadgeKind;

/// One panel's badge and count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelState {
    /// The activity button's badge: kind and number (none hides it).
    pub badge: Option<(BadgeKind, u32)>,
    /// The section header's count ("19 entities").
    pub count: Option<String>,
}

/// Every panel's [`PanelState`], by id; a panel without an entry shows
/// neither. Writers compare first ([`badge`](Self::badge),
/// [`count`](Self::count)) and set only a change, so the shell's readouts
/// run only then.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PanelStatus {
    panels: HashMap<PanelId, PanelState>,
}

impl PanelStatus {
    pub fn get(&self, id: PanelId) -> Option<&PanelState> {
        self.panels.get(&id)
    }

    pub fn badge(&self, id: PanelId) -> Option<(BadgeKind, u32)> {
        self.get(id).and_then(|s| s.badge)
    }

    pub fn count(&self, id: PanelId) -> Option<&str> {
        self.get(id).and_then(|s| s.count.as_deref())
    }

    pub fn set_badge(&mut self, id: PanelId, badge: Option<(BadgeKind, u32)>) {
        self.panels.entry(id).or_default().badge = badge;
    }

    pub fn set_count(&mut self, id: PanelId, count: Option<String>) {
        self.panels.entry(id).or_default().count = count;
    }
}
