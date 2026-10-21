//! The Scene panel's footer (spec §3.3): Hide others and Isolate act on
//! the selection's models; New group waits for groups.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::spawn::{GzEntity, GzIndex};
use gz_bevy::view::{Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use crate::theme::visuals::Active;
use crate::widgets::Tooltip;

/// The footer's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FooterButton {
    NewGroup,
    HideOthers,
    Isolate,
}

impl FooterButton {
    pub(crate) const ALL: [FooterButton; 3] = [
        FooterButton::NewGroup,
        FooterButton::HideOthers,
        FooterButton::Isolate,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            FooterButton::NewGroup => "New group",
            FooterButton::HideOthers => "Hide others",
            FooterButton::Isolate => "Isolate",
        }
    }

    /// The tooltip, enabled or not.
    fn tooltip(self, enabled: bool, isolating: bool) -> Tooltip {
        let detail = match self {
            FooterButton::NewGroup => "Not available yet",
            _ if !enabled => "Select a model, or a part of one, first",
            FooterButton::HideOthers => "Hide every model but the selected ones",
            FooterButton::Isolate if isolating => "Show the models Isolate hid",
            FooterButton::Isolate => "Show only the selected models, and frame them",
        };
        Tooltip::new(self.label()).detail(detail)
    }
}

/// While Isolate is on: each top-level model's visibility before it, to
/// put back when it goes off.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Isolated(pub(crate) Option<Vec<(Entity, Visibility)>>);

/// The world's top-level models, and those the selection is part of.
fn models(
    index: &GzIndex,
    selection: &Selection,
    nodes: &Query<(Entity, &GzEntity, &ChildOf)>,
    parents: &Query<&ChildOf>,
) -> (Vec<Entity>, Vec<Entity>) {
    let all: Vec<Entity> = nodes
        .iter()
        .filter(|(_, g, c)| g.kind == NodeKind::Model && Some(c.parent()) == index.root)
        .map(|(e, ..)| e)
        .collect();
    let picked = selection
        .entities
        .iter()
        .filter_map(|e| {
            let mut e = *e;
            loop {
                if all.contains(&e) {
                    return Some(e);
                }
                e = parents.get(e).ok()?.parent();
            }
        })
        .collect();
    (all, picked)
}

/// Enabled while the selection is part of a model (Isolate also while
/// on, to turn it off); Isolate shows `Active` while on. Isolate forgets a
/// world it no longer applies to (none of the models it saved exist, as
/// after a reload).
#[allow(clippy::too_many_arguments)]
pub(crate) fn footer_states(
    selection: Res<Selection>,
    index: Res<GzIndex>,
    nodes: Query<(Entity, &GzEntity, &ChildOf)>,
    parents: Query<&ChildOf>,
    mut isolated: ResMut<Isolated>,
    buttons: Query<(Entity, &FooterButton, Has<InteractionDisabled>, Has<Active>)>,
    added: Query<(), Added<FooterButton>>,
    mut commands: Commands,
) {
    let stale = isolated
        .0
        .as_ref()
        .is_some_and(|saved| !saved.iter().any(|(e, _)| nodes.contains(*e)));
    if stale {
        isolated.0 = None;
    }
    if !selection.is_changed() && !isolated.is_changed() && added.is_empty() {
        return;
    }
    let (_, picked) = models(&index, &selection, &nodes, &parents);
    let on = isolated.0.is_some();
    for (e, which, disabled, active) in &buttons {
        let enabled = match which {
            FooterButton::NewGroup => false,
            FooterButton::HideOthers => !picked.is_empty(),
            FooterButton::Isolate => on || !picked.is_empty(),
        };
        let mut entity = commands.entity(e);
        if enabled == disabled {
            if enabled {
                entity.remove::<InteractionDisabled>();
            } else {
                entity.insert(InteractionDisabled);
            }
        }
        let want_active = *which == FooterButton::Isolate && on;
        if want_active != active {
            if want_active {
                entity.insert(Active);
            } else {
                entity.remove::<Active>();
            }
        }
        entity.insert(which.tooltip(enabled, on));
    }
}

/// Hide others hides the models outside the selection. Isolate saves every
/// model's visibility, shows the selected ones, hides the rest and frames
/// the selection; pressed again, it puts the visibility back.
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_footer(
    ev: On<Activate>,
    buttons: Query<&FooterButton, Without<InteractionDisabled>>,
    index: Res<GzIndex>,
    selection: Res<Selection>,
    nodes: Query<(Entity, &GzEntity, &ChildOf)>,
    parents: Query<&ChildOf>,
    cameras: Query<Entity, With<MainCamera>>,
    mut visibility: Query<&mut Visibility>,
    mut isolated: ResMut<Isolated>,
    mut views: MessageWriter<ViewCommand>,
) {
    let Ok(which) = buttons.get(ev.entity) else {
        return;
    };
    let (all, picked) = models(&index, &selection, &nodes, &parents);
    if picked.is_empty() && isolated.0.is_none() {
        return;
    }
    let mut set = |e: Entity, v: Visibility| {
        if let Ok(mut current) = visibility.get_mut(e) {
            current.set_if_neq(v);
        }
    };
    match which {
        FooterButton::NewGroup => {}
        FooterButton::HideOthers => {
            for e in all.iter().filter(|e| !picked.contains(e)) {
                set(*e, Visibility::Hidden);
            }
        }
        FooterButton::Isolate => {
            if let Some(saved) = isolated.0.take() {
                for (e, v) in saved {
                    set(e, v);
                }
                return;
            }
            let saved = all
                .iter()
                .filter_map(|e| visibility.get(*e).ok().map(|v| (*e, *v)))
                .collect();
            let mut set = |e: Entity, v: Visibility| {
                if let Ok(mut current) = visibility.get_mut(e) {
                    current.set_if_neq(v);
                }
            };
            for e in &all {
                let shown = picked.contains(e);
                set(
                    *e,
                    if shown {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                );
            }
            isolated.0 = Some(saved);
            for view in &cameras {
                views.write(ViewCommand {
                    view,
                    op: ViewOp::Focus { at: None },
                });
            }
        }
    }
}
