//! A panel for the shell's tests that does everything a panel can, driven
//! from a resource, so the shell's tests don't lean on a built-in panel.

use bevy::prelude::*;

use crate::assets::IconId;
use crate::panels::{
    ActionId, HeaderAction, PanelAction, PanelActionActivated, PanelDef, PanelId, PanelView,
    RegisterPanel,
};
use crate::shell::{PanelStatus, SetPanelStatus, ShellSystems};
use crate::widgets::BadgeKind;

pub(crate) const PROBE: PanelId = PanelId::new("probe");
/// Its enabled header action, on ⌘I (`file.import`, which nothing else
/// in the shell takes).
pub(crate) const POKE: ActionId = ActionId::new("poke");
/// Its disabled header action.
pub(crate) const LATER: ActionId = ActionId::new("later");
/// Its footer's action, enabled as [`Probe::foot`] says.
pub(crate) const FOOT: ActionId = ActionId::new("foot");
/// Its footer's unavailable action.
pub(crate) const UNREADY: ActionId = ActionId::new("unready");
/// Why [`UNREADY`] is unavailable.
pub(crate) const UNREADY_WHY: &str = "Needs a server";

/// What the probe panel shows, and the panel-owned state its content is
/// built from; tests write it.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct Probe {
    pub view: PanelView,
    pub badge: Option<(BadgeKind, u32)>,
    pub count: Option<String>,
    /// How many rows its content has (as M4's tree keeps its expansion).
    pub rows: u32,
    /// Whether [`FOOT`] is enabled, once set.
    pub foot: Option<bool>,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            view: PanelView::Ready,
            badge: None,
            count: None,
            rows: 2,
            foot: None,
        }
    }
}

/// The actions the probe saw, with their buttons.
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct ProbeActions(pub Vec<(ActionId, Entity)>);

/// The probe's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ProbeContent;

/// A row of the probe's content.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeRow(pub u32);

/// Registers the probe panel (in its own group, last in the activity bar).
pub(crate) struct ProbePanelPlugin;

impl Plugin for ProbePanelPlugin {
    fn build(&self, app: &mut App) {
        app.register_panel(
            PanelDef::new(PROBE, "Probe", IconId::ActivityPlot)
                .place(9, 0)
                .header_action(HeaderAction::new(POKE, IconId::Plus, "Poke").binding("file.import"))
                .header_action(HeaderAction::new(LATER, IconId::Copy, "Later").disabled())
                .content::<ProbeContent>()
                .footer_action(PanelAction::new(FOOT, "Foot"))
                .footer_action(PanelAction::new(UNREADY, "Unready").unavailable(UNREADY_WHY)),
        )
        .init_resource::<Probe>()
        .init_resource::<ProbeActions>()
        .add_systems(Update, write_status.in_set(ShellSystems::Panels))
        .add_observer(build_content)
        .add_observer(on_action);
    }
}

fn write_status(probe: Res<Probe>, mut status: ResMut<PanelStatus>) {
    if probe.is_changed() {
        status.set_view(PROBE, probe.view.clone());
        status.set_badge(PROBE, probe.badge);
        status.set_count(PROBE, probe.count.clone());
        if let Some(on) = probe.foot {
            status.set_action_enabled(PROBE, FOOT, on);
        }
    }
}

/// Builds the content from the probe's state, with whatever system
/// parameters it needs.
fn build_content(ev: On<Add<ProbeContent>>, probe: Res<Probe>, mut commands: Commands) {
    commands.entity(ev.entity).with_children(|b| {
        for i in 0..probe.rows {
            b.spawn((ProbeRow(i), Node::default()));
        }
    });
}

fn on_action(ev: On<PanelActionActivated>, mut seen: ResMut<ProbeActions>) {
    if ev.panel == PROBE {
        seen.0.push((ev.action, ev.entity));
    }
}
