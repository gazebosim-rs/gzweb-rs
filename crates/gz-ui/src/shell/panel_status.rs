//! What each panel shows on the shell's chrome (spec §3.2, §3.3): its
//! body's view, its activity button's badge, its section header's count
//! and which of its header actions are enabled. The shell owns [`PanelStatus`] and draws it; each panel's own
//! systems write its entry, so the shell needs to know nothing about a
//! panel (DECISIONS.md D-57, D-58).

use std::ops::DerefMut;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::panels::{ActionId, PanelId, PanelView};
use crate::widgets::BadgeKind;

/// One panel's view, badge, count and header action states.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelState {
    /// What its body shows (none: its [`PanelDef::view`](crate::panels::PanelDef)).
    pub view: Option<PanelView>,
    /// The activity button's badge: kind and number (none hides it).
    pub badge: Option<(BadgeKind, u32)>,
    /// The section header's count ("19 entities").
    pub count: Option<String>,
    /// Header actions enabled or disabled at run time, by id; an action
    /// without an entry keeps its [`HeaderAction::enabled`](crate::panels::HeaderAction).
    pub actions: HashMap<ActionId, bool>,
}

/// Every panel's [`PanelState`], by id; a panel without an entry shows its
/// default view and neither badge nor count. Panels write it through
/// [`SetPanelStatus`], which marks it changed only on a real change, so
/// the shell's readouts and slot rebuilds run only then.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PanelStatus {
    panels: HashMap<PanelId, PanelState>,
}

impl PanelStatus {
    pub fn get(&self, id: PanelId) -> Option<&PanelState> {
        self.panels.get(&id)
    }

    pub fn view(&self, id: PanelId) -> Option<&PanelView> {
        self.get(id).and_then(|s| s.view.as_ref())
    }

    pub fn badge(&self, id: PanelId) -> Option<(BadgeKind, u32)> {
        self.get(id).and_then(|s| s.badge)
    }

    pub fn count(&self, id: PanelId) -> Option<&str> {
        self.get(id).and_then(|s| s.count.as_deref())
    }

    /// Whether `panel`'s header action `action` is enabled, if its systems
    /// said (else its [`HeaderAction::enabled`](crate::panels::HeaderAction)
    /// holds).
    pub fn action_enabled(&self, panel: PanelId, action: ActionId) -> Option<bool> {
        self.get(panel)
            .and_then(|s| s.actions.get(&action).copied())
    }
}

/// Writing a panel's [`PanelStatus`] entry from its systems: each setter
/// compares first and marks the resource changed only on a difference.
/// For `ResMut<PanelStatus>` (and `Mut<PanelStatus>`).
pub trait SetPanelStatus {
    fn set_view(&mut self, id: PanelId, view: PanelView);
    fn set_badge(&mut self, id: PanelId, badge: Option<(BadgeKind, u32)>);
    fn set_count(&mut self, id: PanelId, count: Option<String>);
    /// Enables or disables one of `id`'s header actions; the header's
    /// button follows in place.
    fn set_action_enabled(&mut self, id: PanelId, action: ActionId, enabled: bool);
}

impl<T> SetPanelStatus for T
where
    T: DetectChangesMut<Inner = PanelStatus> + DerefMut<Target = PanelStatus>,
{
    fn set_view(&mut self, id: PanelId, view: PanelView) {
        if self.bypass_change_detection().view(id) != Some(&view) {
            self.panels.entry(id).or_default().view = Some(view);
        }
    }

    fn set_badge(&mut self, id: PanelId, badge: Option<(BadgeKind, u32)>) {
        if self.bypass_change_detection().badge(id) != badge {
            self.panels.entry(id).or_default().badge = badge;
        }
    }

    fn set_count(&mut self, id: PanelId, count: Option<String>) {
        if self.bypass_change_detection().count(id) != count.as_deref() {
            self.panels.entry(id).or_default().count = count;
        }
    }

    fn set_action_enabled(&mut self, id: PanelId, action: ActionId, enabled: bool) {
        if self.bypass_change_detection().action_enabled(id, action) != Some(enabled) {
            self.panels
                .entry(id)
                .or_default()
                .actions
                .insert(action, enabled);
        }
    }
}
