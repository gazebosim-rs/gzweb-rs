//! The Light inspector (spec §3.5): the LIGHT tag and the light's name,
//! its path, Focus and Hide (Duplicate waits for M5c), the live pose and
//! the light's direction, and its parameters from the scene.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;
use gz_bevy::spawn::{GzEntity, GzIndex, GzLight, GzWorldRoot};

use super::common::{
    breadcrumb, crumb, definition_group, links, pose_editable, pose_group, quick_action,
    quick_action_node, quick_actions, title_row, EntityAction, PoseFrame,
};
use super::{InspectKind, Inspected, InspectorDef, InspectorId, RegisterInspector};
use crate::format;
use crate::shell::world_name;
use crate::theme::Theme;

/// The Light inspector's id.
pub const LIGHT: InspectorId = InspectorId::new("light");

/// The Light inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct LightBody;

/// Registers the Light inspector.
pub struct LightInspectorPlugin;

impl Plugin for LightInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.register_inspector(InspectorDef::new(LIGHT, InspectKind::Light).content::<LightBody>())
            .add_observer(build_light);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_light(
    ev: On<Add<LightBody>>,
    inspected: Res<Inspected>,
    theme: Res<Theme>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    lights: Query<(&GzEntity, Option<&GzLight>)>,
    nodes: Query<(&GzEntity, &ChildOf)>,
    roots: Query<Entity, With<GzWorldRoot>>,
    mut commands: Commands,
) {
    let Some(light) = inspected.entity else {
        return;
    };
    let Ok((g, desc)) = lights.get(light) else {
        return;
    };
    let (name, path) = (g.name.clone(), g.path.clone());
    let world = world_name(status.as_deref(), index.as_deref());
    let crumb = crumb(world.as_deref(), &path);
    let desc = desc.map(|d| d.0.clone());
    let editable = pose_editable(status.as_deref(), light, &nodes, &roots);
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn(title_row(&t, InspectKind::Light, &name));
            b.spawn(breadcrumb(crumb));
            b.spawn(quick_actions()).with_children(|r| {
                r.spawn(quick_action(&t, "Focus", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::Focus);
                r.spawn(quick_action(&t, "Duplicate", Some("Not available yet")))
                    .insert(quick_action_node(&t));
                r.spawn(quick_action(&t, "Hide", None))
                    .insert(quick_action_node(&t))
                    .insert(EntityAction::Hide);
            });
            pose_group(b, &t, "Pose", PoseFrame::Parent, editable);
            let Some(d) = desc else {
                return;
            };
            let n = |v: f64| format::number(v, 3);
            let angle = |a: Option<f64>| a.map_or("—".into(), |a| format::degrees(a as f32));
            let mut rows = vec![
                ("Type".into(), d.ty.as_str().to_string()),
                ("Diffuse".into(), format::hex(d.color)),
                ("Specular".into(), format::hex(d.specular)),
                ("Intensity".into(), n(d.msg_intensity)),
                (
                    "Range".into(),
                    if d.range > 0.0 {
                        format!("{} m", n(d.range))
                    } else {
                        "—".into()
                    },
                ),
            ];
            if let Some([x, y, z]) = d.direction {
                rows.push(("Direction".into(), format!("{}  {}  {}", n(x), n(y), n(z))));
            }
            rows.push(("Inner angle".into(), angle(d.inner_angle)));
            rows.push(("Outer angle".into(), angle(d.angle)));
            rows.push(("Cast shadows".into(), format::yes_no(d.cast_shadows).into()));
            definition_group(b, "Light", rows);
            links(b, &["Attenuation", "All components"]);
        })));
}

#[cfg(test)]
mod tests;
