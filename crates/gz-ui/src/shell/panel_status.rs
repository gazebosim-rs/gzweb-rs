//! What each panel shows on the shell's chrome (spec §3.2, §3.3): its
//! body's view, its activity button's badge, its section header's count
//! and which of its header and footer actions are enabled. The shell owns
//! [`PanelStatus`] and draws it; each panel's own
//! systems write its entry, so the shell needs to know nothing about a
//! panel (DECISIONS.md D-57, D-58).

use std::ops::DerefMut;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::panels::{ActionId, HeaderAction, PanelId, PanelView};
use crate::widgets::BadgeKind;

/// One panel's view, badge, count and actions' enabled state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelState {
    /// What its body shows (none: its [`PanelDef::view`](crate::panels::PanelDef)).
    pub view: Option<PanelView>,
    /// The activity button's badge: kind and number (none hides it).
    pub badge: Option<(BadgeKind, u32)>,
    /// The section header's count ("19 entities").
    pub count: Option<String>,
    /// Its header and footer actions' enabled state, where its systems
    /// set one: an action without an entry keeps its registered state
    /// ([`HeaderAction::enabled`](crate::panels::HeaderAction),
    /// [`PanelAction::enabled`](crate::panels::PanelAction)): Expand all
    /// while there is a tree, Copy while there is a message.
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

    /// Whether `id`'s header or footer action `action` is enabled, if its
    /// systems said so (none: as registered).
    pub fn action_enabled(&self, id: PanelId, action: ActionId) -> Option<bool> {
        self.get(id).and_then(|s| s.actions.get(&action).copied())
    }

    /// Whether `id`'s header action `a` is enabled now: as its systems
    /// set it, else as registered.
    pub(crate) fn header_action_enabled(&self, id: PanelId, a: &HeaderAction) -> bool {
        self.action_enabled(id, a.id).unwrap_or(a.enabled)
    }

    /// Whether `id`'s action `action` is enabled now: as its systems set
    /// it, else `registered`.
    pub(crate) fn action_enabled_or(
        &self,
        id: PanelId,
        action: ActionId,
        registered: bool,
    ) -> bool {
        self.action_enabled(id, action).unwrap_or(registered)
    }
}

/// Writing a panel's [`PanelStatus`] entry from its systems: each setter
/// compares first and marks the resource changed only on a difference.
/// For `ResMut<PanelStatus>` (and `Mut<PanelStatus>`).
pub trait SetPanelStatus {
    fn set_view(&mut self, id: PanelId, view: PanelView);
    fn set_badge(&mut self, id: PanelId, badge: Option<(BadgeKind, u32)>);
    fn set_count(&mut self, id: PanelId, count: Option<String>);
    /// Enables or disables `id`'s header or footer action `action`; the
    /// shell updates its button in place (look, tooltip, whether it
    /// reacts).
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
