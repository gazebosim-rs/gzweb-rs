// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Explicit primitive lifecycle editor; never invokes generic Gazebo edits.
use super::{PanelDef, PanelId, PanelView, RegisterPanel, WORLD_GROUP};
use crate::assets::{fonts::text, IconId};
use crate::shell::ShellSystems;
use crate::theme::{ColorRole, Theme, TypeStyle};
use crate::widgets::button::{button, ButtonVariant};
use crate::widgets::layout::column;
use crate::widgets::{number_field, AxisTag, NumberCommit, NumberField};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::Selection;
use gz_bevy::{BackendKind, GzConfig};
use gz_core::primitive::lifecycle::{Catalog, Command as Edit, Reply, State};

pub(super) const LIFECYCLE: PanelId = PanelId::new("lifecycle");
pub(super) struct LifecyclePanelPlugin;
#[derive(Component, Default)]
struct Body;
#[derive(Component)]
struct Field(usize);
#[derive(Component)]
struct Note;
#[derive(Component, Clone)]
enum Operation {
    Template(u64),
    Model(u64),
    Spawn,
    Remove,
    Read,
    Apply,
}
#[derive(Resource, Default)]
struct Editor {
    context: Option<(u64, u64)>,
    catalog: Option<Catalog>,
    selected: Option<u64>,
    read_for: Option<u64>,
    template: Option<u64>,
    state: State,
    reply: Option<Reply>,
    revision: u64,
    built: u64,
}
impl Editor {
    fn command(&self, action: &Operation) -> Result<Edit, &'static str> {
        Ok(match action {
            Operation::Spawn => Edit::Spawn {
                template_id: self.template.ok_or("Select a declared template")?,
                translation: self.state.translation,
                rotation: self.state.rotation,
            },
            Operation::Remove => Edit::Remove {
                model_id: self.selected.ok_or("Select a model")?,
            },
            Operation::Read => Edit::GetState {
                model_id: self.selected.ok_or("Select a model")?,
            },
            Operation::Apply => {
                let id = self.selected.ok_or("Select a model")?;
                if self.read_for != Some(id) {
                    return Err("Read selected state before applying edits");
                }
                Edit::SetState {
                    model_id: id,
                    state: self.state.clone(),
                }
            }
            _ => return Err("Choose an operation"),
        })
    }
}
impl Plugin for LifecyclePanelPlugin {
    fn build(&self, app: &mut App) {
        if !app
            .world()
            .get_resource::<GzConfig>()
            .is_some_and(|c| c.backend == BackendKind::Primitive && c.lifecycle)
        {
            return;
        }
        app.register_panel(
            PanelDef::new(LIFECYCLE, "World lifecycle", IconId::ActivityScene)
                .place(WORLD_GROUP, 25)
                .view(PanelView::Ready)
                .content::<Body>(),
        )
        .init_resource::<Editor>()
        .add_systems(
            Update,
            (sync, view, enable_buttons)
                .chain()
                .in_set(ShellSystems::Panels),
        )
        .add_observer(build)
        .add_observer(clicked)
        .add_observer(number);
    }
}
fn sync(
    status: Res<GzStatus>,
    index: Res<GzIndex>,
    mut selection: ResMut<Selection>,
    mut editor: ResMut<Editor>,
) {
    let context = status.lifecycle.as_ref().map(|c| (c.world_id, c.epoch));
    if context != editor.context {
        editor.context = context;
        editor.selected = None;
        editor.read_for = None;
        editor.template = None;
        editor.state = State::default();
        editor.reply = None;
        selection.entities.clear();
        selection.parts.clear();
        editor.revision += 1;
    }
    let changed = match (&editor.catalog, &status.lifecycle) {
        (Some(a), Some(b)) => a.templates != b.templates || a.models != b.models,
        (None, None) => false,
        _ => true,
    };
    if changed {
        if editor.selected.is_some_and(|id| {
            status
                .lifecycle
                .as_ref()
                .is_none_or(|c| !c.models.iter().any(|m| m.model_id == id))
        }) {
            editor.selected = None;
            editor.read_for = None;
            editor.state = State::default();
        }
        if editor.template.is_some_and(|id| {
            status
                .lifecycle
                .as_ref()
                .is_none_or(|c| !c.templates.iter().any(|t| t.template_id == id))
        }) {
            editor.template = None;
        }
        editor.revision += 1;
    }
    editor.catalog = status.lifecycle.clone();
    if editor.reply != status.lifecycle_reply {
        if let Some(reply) = &status.lifecycle_reply {
            if reply.ok && reply.model_id == editor.selected {
                if let Some(state) = &reply.state {
                    editor.state = state.clone();
                    editor.read_for = reply.model_id;
                    editor.revision += 1;
                }
            }
        }
        editor.reply = status.lifecycle_reply.clone();
    }
    if let Some(model) = editor.selected.and_then(|id| {
        editor
            .catalog
            .as_ref()?
            .models
            .iter()
            .find(|m| m.model_id == id)
    }) {
        let entities: Vec<_> = model
            .visual_ids
            .iter()
            .filter_map(|id| {
                u32::try_from(*id)
                    .ok()
                    .and_then(|id| index.by_id.get(&id).copied())
            })
            .collect();
        if selection.entities != entities {
            selection.entities = entities;
            selection.parts.clear();
        }
    } else if editor.selected.is_none() && changed {
        selection.entities.clear();
        selection.parts.clear();
    }
}
fn build(ev: On<Add<Body>>, mut commands: Commands, mut editor: ResMut<Editor>) {
    commands.entity(ev.entity).insert(Node {
        overflow: Overflow::scroll_y(),
        ..column(8.)
    });
    editor.revision += 1;
}
fn view(
    mut commands: Commands,
    bodies: Query<Entity, With<Body>>,
    notes: Query<Entity, With<Note>>,
    status: Res<GzStatus>,
    theme: Res<Theme>,
    mut editor: ResMut<Editor>,
) {
    for note in &notes {
        let message = if status.control_pending {
            "Waiting for committed reply…"
        } else if status.lifecycle.is_none() {
            status
                .control_error
                .as_deref()
                .unwrap_or("Lifecycle unavailable; configure server and opt in")
        } else {
            status
                .control_error
                .as_deref()
                .unwrap_or("Read selected state before Apply. World Z-up: metres, xyzw, m/s, rad/s")
        };
        let sample = editor
            .reply
            .as_ref()
            .filter(|r| r.ok && r.state.is_some())
            .map(|r| {
                format!(
                    "\nState epoch {}, {}.{:09}s; sequence {}",
                    r.epoch, r.seconds, r.nanoseconds, r.scene_sequence
                )
            })
            .unwrap_or_default();
        commands
            .entity(note)
            .insert(Text::new(format!("{message}{sample}")));
    }
    if editor.built == editor.revision && !notes.is_empty() {
        return;
    }
    editor.built = editor.revision;
    for body in &bodies {
        commands.entity(body).despawn_children();
        commands.entity(body).with_children(|b| {
            b.spawn((
                Note,
                text(
                    TypeStyle::BODY,
                    ColorRole::Text,
                    "Lifecycle capability pending",
                ),
            ));
            let Some(catalog) = &editor.catalog else {
                return;
            };
            b.spawn(text(TypeStyle::BODY, ColorRole::Text, "Declared templates"));
            for t in &catalog.templates {
                b.spawn((
                    button(
                        &theme,
                        format!(
                            "{} {}",
                            if editor.template == Some(t.template_id) {
                                "✓"
                            } else {
                                ""
                            },
                            t.label
                        ),
                        ButtonVariant::Normal,
                    ),
                    Operation::Template(t.template_id),
                ));
            }
            b.spawn((
                button(&theme, "Spawn at edited pose", ButtonVariant::Normal),
                Operation::Spawn,
            ));
            b.spawn(text(
                TypeStyle::BODY,
                ColorRole::Text,
                "Server models (public IDs)",
            ));
            for m in &catalog.models {
                b.spawn((
                    button(
                        &theme,
                        format!(
                            "{} {} #{}",
                            if editor.selected == Some(m.model_id) {
                                "✓"
                            } else {
                                ""
                            },
                            m.name,
                            m.model_id
                        ),
                        ButtonVariant::Normal,
                    ),
                    Operation::Model(m.model_id),
                ));
            }
            for (label, action) in [
                ("Read selected state", Operation::Read),
                ("Apply edited state", Operation::Apply),
                ("Remove selected model", Operation::Remove),
            ] {
                b.spawn((button(&theme, label, ButtonVariant::Normal), action));
            }
            let values = editor
                .state
                .translation
                .into_iter()
                .chain(editor.state.rotation)
                .chain(editor.state.linear_velocity)
                .chain(editor.state.angular_velocity);
            for (i, (label, value)) in [
                "position x (m)",
                "position y (m)",
                "position z (m)",
                "quaternion x",
                "quaternion y",
                "quaternion z",
                "quaternion w",
                "linear x (m/s)",
                "linear y (m/s)",
                "linear z (m/s)",
                "angular x (rad/s)",
                "angular y (rad/s)",
                "angular z (rad/s)",
            ]
            .into_iter()
            .zip(values)
            .enumerate()
            {
                b.spawn((
                    number_field(
                        &theme,
                        AxisTag::Label(label.into()),
                        NumberField::new(value).precision(6, 0.01).named(label),
                        true,
                    ),
                    Field(i),
                ));
            }
        });
    }
}
fn clicked(
    ev: On<Activate>,
    buttons: Query<(&Operation, Has<bevy::ui::InteractionDisabled>)>,
    status: Res<GzStatus>,
    mut editor: ResMut<Editor>,
    mut requests: MessageWriter<GzCommand>,
) {
    let Ok((action, disabled)) = buttons.get(ev.entity) else {
        return;
    };
    if disabled || status.control_pending || status.lifecycle.is_none() {
        return;
    }
    match action {
        Operation::Template(id) => {
            editor.template = Some(*id);
            editor.revision += 1;
        }
        Operation::Model(id) => {
            editor.selected = Some(*id);
            editor.read_for = None;
            editor.state = State::default();
            editor.revision += 1;
        }
        _ => {
            if let Ok(command) = editor.command(action) {
                requests.write(GzCommand::Lifecycle(command));
            }
        }
    }
}
fn number(ev: On<NumberCommit>, fields: Query<&Field>, mut editor: ResMut<Editor>) {
    let Ok(field) = fields.get(ev.entity) else {
        return;
    };
    match field.0 {
        0..=2 => editor.state.translation[field.0] = ev.value,
        3..=6 => editor.state.rotation[field.0 - 3] = ev.value,
        7..=9 => editor.state.linear_velocity[field.0 - 7] = ev.value,
        10..=12 => editor.state.angular_velocity[field.0 - 10] = ev.value,
        _ => {}
    }
}
#[cfg(test)]
mod tests;

fn enable_buttons(
    mut commands: Commands,
    buttons: Query<(Entity, &Operation)>,
    status: Res<GzStatus>,
    editor: Res<Editor>,
) {
    for (entity, action) in &buttons {
        let enabled = !status.control_pending
            && status.lifecycle.is_some()
            && match action {
                Operation::Template(_) | Operation::Model(_) => true,
                _ => editor.command(action).is_ok_and(|c| {
                    editor
                        .catalog
                        .as_ref()
                        .is_some_and(|catalog| c.validate(catalog).is_ok())
                }),
            };
        if enabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        } else {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
    }
}
