//! The inspector (spec §3.5), as a frame: the section header with pin and
//! collapse, a scrolling body holding the inspector for what is
//! [`Inspected`] ([`InspectorRegistry`]; with nothing selected, the World
//! inspector), and the splitter. The subject follows the selection unless
//! pinned; a subject that is despawned is followed to its respawn (the
//! same scene path), else shown as removed until dismissed.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::Selection;
use gz_core::scene::NodeKind;

use super::regions::side_panel;
use super::splitter::side_splitter;
use super::{ShellLayout, Side, UiCommand};
use crate::assets::icons::IconId;
use crate::inspectors::{InspectKind, Inspected, InspectorRegistry};
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::layout::column;
use crate::widgets::{icon_button, section_header, IconButtonSize, Tooltip};
use gz_bevy::view::PartKind;

/// The inspector header's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InspectorHeader {
    /// Keeps showing this entity when the selection changes (M5).
    Pin,
    Collapse,
}

/// The scrolling body the inspector's content goes in.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct InspectorBody;

/// The content in the body, for this subject (kind, entity, the part's
/// name; for several, the selection itself), built from this generation
/// of the world's SDFormat (a new reply rebuilds it).
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct InspectorContent(
    pub(crate) InspectKind,
    pub(crate) Option<Entity>,
    pub(crate) u32,
    pub(crate) Option<String>,
    pub(crate) Option<Selection>,
);

/// The inspector region; [`rebuild_body`] fills its body.
pub(crate) fn inspector(theme: &Theme, layout: &ShellLayout, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        side_panel(Side::Inspector, layout),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(section_header(&t, "Inspector", None))
                .with_children(|h| {
                    let small = IconButtonSize::Small;
                    h.spawn((
                        icon_button(
                            &t,
                            IconId::Pin,
                            small,
                            Tooltip::new("Pin")
                                .detail("Keep showing this when the selection changes"),
                        ),
                        InspectorHeader::Pin,
                    ));
                    h.spawn((
                        icon_button(
                            &t,
                            IconId::ChevronRight,
                            small,
                            Tooltip::new("Collapse inspector").binding(&map, "ui.inspector"),
                        ),
                        InspectorHeader::Collapse,
                    ));
                });
            p.spawn((
                InspectorBody,
                ScrollArea,
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    overflow: Overflow::scroll_y(),
                    ..column(0.0)
                },
            ));
            p.spawn(side_splitter(&t, Side::Inspector));
        })),
    )
}

/// The header's buttons: Pin toggles, the chevron collapses the
/// inspector.
pub(crate) fn on_header_button(
    ev: On<Activate>,
    buttons: Query<&InspectorHeader>,
    mut inspected: ResMut<Inspected>,
    mut out: MessageWriter<UiCommand>,
) {
    let Ok(which) = buttons.get(ev.entity) else {
        return;
    };
    match which {
        InspectorHeader::Collapse => {
            out.write(UiCommand::ToggleInspector);
        }
        InspectorHeader::Pin => {
            // `pin_state` shows it.
            inspected.pinned = !inspected.pinned;
        }
    }
}

/// The kind of inspector for a scene entity.
fn kind_of(kind: NodeKind) -> Option<InspectKind> {
    match kind {
        NodeKind::Model => Some(InspectKind::Model),
        NodeKind::Link => Some(InspectKind::Link),
        NodeKind::Light => Some(InspectKind::Light),
        NodeKind::Visual => None,
    }
}

/// The subject follows the selection: nothing is the world, one entity
/// or part its kind, more the common fields. Pinned, it stays. Despawned,
/// it moves to the entity now at its path (selected again if it was the
/// selection), else shows as removed: while pinned until Dismiss, else
/// until something else is selected.
pub(crate) fn choose_subject(
    mut selection: ResMut<Selection>,
    entities: Query<(Entity, &GzEntity)>,
    mut inspected: ResMut<Inspected>,
    mut was_selected: Local<bool>,
) {
    match inspected.entity {
        Some(e) if entities.get(e).is_err() => {
            let respawned = inspected.path.as_ref().and_then(|path| {
                entities
                    .iter()
                    .find(|(_, g)| &g.path == path)
                    .map(|(e, _)| e)
            });
            if let Some(e) = respawned {
                inspected.entity = Some(e);
                inspected.removed = false;
                if let Some(part) = inspected.part.as_mut() {
                    part.owner = e;
                }
                // It was the selection (pruning dropped it): it is again.
                if *was_selected && selection.is_empty() {
                    match inspected.part.clone() {
                        Some(part) => selection.parts.push(part),
                        None => selection.entities.push(e),
                    }
                }
                return;
            }
            if !inspected.removed {
                inspected.removed = true;
            }
        }
        Some(e) => {
            *was_selected = match &inspected.part {
                Some(part) => selection.parts.contains(part),
                None => selection.entities.contains(&e),
            };
        }
        None => *was_selected = false,
    }
    if inspected.removed && (inspected.pinned || selection.is_empty()) {
        return;
    }
    if inspected.pinned {
        return;
    }
    if !selection.is_changed() && !inspected.removed {
        return;
    }
    let part_subject = |p: &gz_bevy::view::Part| {
        entities.get(p.owner).ok().map(|(_, g)| Inspected {
            kind: match p.kind {
                PartKind::Joint => InspectKind::Joint,
                PartKind::Sensor => InspectKind::Sensor,
            },
            entity: Some(p.owner),
            path: Some(g.path.clone()),
            part: Some(p.clone()),
            ..default()
        })
    };
    let want = match (selection.entities.as_slice(), selection.parts.as_slice()) {
        ([], []) => Inspected::default(),
        ([], [p]) => part_subject(p).unwrap_or_default(),
        ([e], []) => match entities
            .get(*e)
            .ok()
            .and_then(|(_, g)| kind_of(g.kind).map(|k| (g, k)))
        {
            Some((g, kind)) => Inspected {
                kind,
                entity: Some(*e),
                path: Some(g.path.clone()),
                ..default()
            },
            None => Inspected::default(),
        },
        _ => Inspected {
            kind: InspectKind::Multi,
            ..default()
        },
    };
    inspected.set_if_neq(want);
}

/// The header's Pin shows whether the subject is pinned.
pub(crate) fn pin_state(
    inspected: Res<Inspected>,
    buttons: Query<(Entity, &InspectorHeader, Has<Active>)>,
    added: Query<(), Added<InspectorHeader>>,
    mut commands: Commands,
) {
    if !inspected.is_changed() && added.is_empty() {
        return;
    }
    for (e, which, active) in &buttons {
        if *which == InspectorHeader::Pin && active != inspected.pinned {
            if inspected.pinned {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
    }
}

/// A new subject gets a new content entity, with its inspector's marker,
/// so the inspector's observer builds it.
pub(crate) fn rebuild_body(
    inspected: Res<Inspected>,
    selection: Res<Selection>,
    registry: Res<InspectorRegistry>,
    sdf: Option<Res<SdfExtras>>,
    bodies: Query<(Entity, Option<&Children>), With<InspectorBody>>,
    contents: Query<&InspectorContent>,
    mut commands: Commands,
) {
    let generation = sdf.map_or(0, |s| s.generation);
    let want = InspectorContent(
        inspected.kind,
        inspected.entity,
        generation,
        inspected.part.as_ref().map(|p| p.name.clone()),
        (inspected.kind == InspectKind::Multi).then(|| selection.clone()),
    );
    for (body, children) in &bodies {
        let current = children
            .into_iter()
            .flatten()
            .find_map(|c| contents.get(*c).ok().map(|k| (*c, k.clone())));
        if current.as_ref().map(|c| &c.1) == Some(&want) {
            continue;
        }
        if let Some((old, _)) = current {
            commands.entity(old).despawn();
        }
        let mut content = commands.spawn((
            want.clone(),
            Node {
                flex_shrink: 0.0,
                ..column(0.0)
            },
            ChildOf(body),
        ));
        if let Some(mark) = registry.for_kind(want.0).and_then(|i| i.content) {
            mark(&mut content);
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<Inspected>()
        .add_systems(
            Update,
            (choose_subject, rebuild_body, pin_state)
                .chain()
                .in_set(crate::GzUiSystems::Behaviour),
        )
        .add_systems(
            Update,
            crate::inspectors::common::pose_values.in_set(super::ShellSystems::Readouts),
        )
        .add_observer(on_header_button)
        .add_observer(crate::inspectors::common::on_dismiss)
        .add_observer(crate::inspectors::common::on_entity_action)
        .add_observer(crate::inspectors::common::on_select_path)
        .add_systems(
            Update,
            (
                crate::inspectors::common::removed_state,
                crate::inspectors::common::hide_label,
            )
                .in_set(super::ShellSystems::Readouts),
        );
}
