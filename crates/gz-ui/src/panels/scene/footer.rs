//! The Scene panel's footer (spec §3.3): Hide others and Isolate act on
//! the selection's models; New group waits for groups. The shell draws the
//! footer under every state of the body (DECISIONS.md D-98); the panel
//! builds its buttons into it, since Isolate has an on state and each
//! button's tooltip follows the selection.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::spawn::{GzEntity, GzIndex};
use gz_bevy::undo::{Change, Isolated, Record};
use gz_bevy::view::{Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use super::SceneFooter;
use crate::shell::sidebar::footer::footer_button;
use crate::theme::visuals::Active;
use crate::theme::Theme;
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

/// Builds the buttons into the shell's footer row, disabled until
/// [`footer_states`] runs.
pub(crate) fn build_footer(ev: On<Add<SceneFooter>>, theme: Res<Theme>, mut commands: Commands) {
    commands.entity(ev.entity).with_children(|f| {
        for which in FooterButton::ALL {
            footer_button(f, &theme, which.label()).insert((
                which,
                InteractionDisabled,
                Tooltip::new(which.label()),
            ));
        }
    });
}

/// The world's top-level models.
fn models(index: &GzIndex, nodes: &Query<(Entity, &GzEntity, &ChildOf)>) -> Vec<Entity> {
    nodes
        .iter()
        .filter(|(_, g, c)| is_model(index, g, c))
        .map(|(e, ..)| e)
        .collect()
}

fn is_model(index: &GzIndex, g: &GzEntity, parent: &ChildOf) -> bool {
    g.kind == NodeKind::Model && Some(parent.parent()) == index.root
}

/// The top-level models the selection is part of: each selected entity's
/// ancestors up to its model, so the cost follows the selection, not the
/// world.
fn picked(
    index: &GzIndex,
    selection: &Selection,
    nodes: &Query<(Entity, &GzEntity, &ChildOf)>,
    parents: &Query<&ChildOf>,
) -> HashSet<Entity> {
    selection
        .entities
        .iter()
        .chain(selection.parts.iter().map(|p| &p.owner))
        .filter_map(|e| {
            let mut e = *e;
            loop {
                if nodes.get(e).is_ok_and(|(_, g, c)| is_model(index, g, c)) {
                    return Some(e);
                }
                e = parents.get(e).ok()?.parent();
            }
        })
        .collect()
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
    mut gone: RemovedComponents<GzEntity>,
    mut commands: Commands,
) {
    // Looked at as entities go or the scene changes (while the panel was
    // closed too) and as Isolate starts, not every frame.
    let went = gone.read().count() > 0;
    let stale = (went || index.is_changed() || isolated.is_changed())
        && isolated.0.as_ref().is_some_and(|saved| {
            let paths: HashSet<&str> = saved.iter().map(|(p, _)| p.as_str()).collect();
            !nodes
                .iter()
                .any(|(_, g, _)| paths.contains(g.path.as_str()))
        });
    if stale {
        isolated.0 = None;
    }
    if !selection.is_changed() && !isolated.is_changed() && added.is_empty() {
        return;
    }
    let picked = picked(&index, &selection, &nodes, &parents);
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
/// the selection; pressed again, it puts the visibility back. Each is one
/// edit to undo.
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
    mut records: MessageWriter<Record>,
) {
    let Ok(which) = buttons.get(ev.entity) else {
        return;
    };
    let picked = picked(&index, &selection, &nodes, &parents);
    if picked.is_empty() && isolated.0.is_none() {
        return;
    }
    let all = models(&index, &nodes);
    // What Isolate saves: every model's visibility, by path.
    let saved: Vec<(String, Visibility)> = all
        .iter()
        .filter_map(|e| {
            let path = nodes.get(*e).ok()?.1.path.clone();
            visibility.get(*e).ok().map(|v| (path, *v))
        })
        .collect();
    let mut changes = Vec::new();
    let mut set = |e: Entity, after: Visibility| {
        if let Ok(mut current) = visibility.get_mut(e) {
            let before = *current;
            if before != after {
                *current = after;
                changes.push(Change::Visibility {
                    entity: e,
                    before,
                    after,
                });
            }
        }
    };
    let verb = match which {
        FooterButton::NewGroup => return,
        FooterButton::HideOthers => {
            for e in all.iter().filter(|e| !picked.contains(*e)) {
                set(*e, Visibility::Hidden);
            }
            "hide others"
        }
        FooterButton::Isolate => {
            if let Some(saved) = isolated.0.take() {
                let by_path: HashMap<&str, Entity> =
                    nodes.iter().map(|(e, g, _)| (g.path.as_str(), e)).collect();
                for (path, v) in &saved {
                    if let Some(e) = by_path.get(path.as_str()) {
                        set(*e, *v);
                    }
                }
                changes.push(Change::Isolated {
                    before: Some(saved),
                    after: None,
                });
                "end isolate"
            } else {
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
                changes.push(Change::Isolated {
                    before: None,
                    after: Some(saved.clone()),
                });
                isolated.0 = Some(saved);
                for view in &cameras {
                    views.write(ViewCommand {
                        view,
                        op: ViewOp::Focus { at: None },
                    });
                }
                "isolate"
            }
        }
    };
    if !changes.is_empty() {
        records.write(Record {
            verb,
            named: false,
            changes,
            scrub: None,
            open: false,
        });
    }
}
