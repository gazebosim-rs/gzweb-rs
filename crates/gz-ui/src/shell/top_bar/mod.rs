//! The top bar (spec §3.1): logo, World menu, tools, transport
//! with the sim clock and RTF, and the search trigger, with two flexible
//! spacers keeping the transport centred between the left group and the
//! search. The tool and transport groups have no container: their buttons
//! sit on the bar's `surface` with the activity buttons' look, Play/Pause
//! included (DECISIONS.md D-48).

pub(crate) mod save_dialog;
pub(crate) mod search;
pub(crate) mod tools;
pub(crate) mod transport;
pub(crate) mod world_button;
pub(crate) mod world_menu;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use super::regions::{pass_through, region, Region};
use super::ShellSystems;
use crate::assets::brand::logo;
use crate::theme::{border, ColorRole, Theme, SPACE};
use crate::widgets::layout::{flex_spacer, row, separator, Orientation, BAR_SEPARATOR_H};

/// A top bar menu to open at start, for a capture (`gzweb --menu
/// world|snap`): once the world is live; then the resource is removed.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct StartMenu(pub String);

/// Opens a [`StartMenu`]'s menu once the world is live (the context
/// menu once something is selected, in the first view).
#[allow(clippy::too_many_arguments)]
fn apply_start_menu(
    start: Option<Res<StartMenu>>,
    phase: Res<super::CurrentPhase>,
    selection: Option<Res<gz_bevy::view::Selection>>,
    cameras: Query<Entity, With<gz_bevy::camera::MainCamera>>,
    menus: Query<(Entity, &Children), With<crate::widgets::menu::Menu>>,
    worlds: Query<(), With<world_button::WorldButton>>,
    snaps: Query<(), With<tools::SnapTrigger>>,
    switchers: Query<(), With<crate::shell::status_bar::WorkspaceButton>>,
    mut contexts: MessageWriter<gz_bevy::view::EntityContext>,
    mut commands: Commands,
) {
    let Some(start) = start else {
        return;
    };
    if phase.0 != super::Phase::Live {
        return;
    }
    if start.0 == "context" {
        if let Some(entity) = selection.and_then(|s| s.entities.first().copied()) {
            contexts.write(gz_bevy::view::EntityContext {
                entity,
                view: cameras.iter().next(),
            });
            commands.remove_resource::<StartMenu>();
        }
        return;
    }
    let menu = menus.iter().find(|(_, children)| {
        children.iter().any(|c| match start.0.as_str() {
            "world" => worlds.contains(c),
            "snap" => snaps.contains(c),
            "workspace" => switchers.contains(c),
            _ => false,
        })
    });
    match menu {
        Some((menu, _)) => {
            commands.trigger(bevy::ui_widgets::MenuEvent {
                source: menu,
                action: bevy::ui_widgets::MenuAction::Open(
                    bevy::input_focus::tab_navigation::NavAction::First,
                ),
            });
        }
        None => warn!("--menu: no menu '{}'", start.0),
    }
    commands.remove_resource::<StartMenu>();
}

/// The top bar.
pub(crate) fn top_bar(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        region(
            Region::TopBar,
            Node {
                height: px(theme.s.top_bar_h),
                flex_shrink: 0.0,
                padding: UiRect {
                    left: px(SPACE[4]),
                    right: px(SPACE[5]),
                    ..default()
                },
                border: UiRect::bottom(px(border::HAIRLINE)),
                ..row(SPACE[4])
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((logo(t.s.logo), pass_through()));
            b.spawn(world_menu::world_menu(&t, &map));
            b.spawn(separator(Orientation::Vertical, BAR_SEPARATOR_H, SPACE[2]));
            tools::spawn_tools(b, &t, &map);
            b.spawn((flex_spacer(), pass_through()));
            b.spawn(transport::transport_group(&t, &map));
            b.spawn((flex_spacer(), pass_through()));
            b.spawn(search::search_trigger(&t, &map));
        })),
    )
}

pub(crate) fn build(app: &mut App) {
    app.add_message::<gz_bevy::view::EntityContext>();
    if !app.is_plugin_added::<gz_bevy::world_save::WorldSavePlugin>() {
        app.add_plugins(gz_bevy::world_save::WorldSavePlugin);
    }
    app.init_resource::<tools::Tool>()
        // The gizmo's; the Snap popover sets it.
        .init_resource::<gz_bevy::view::gizmo::Snap>()
        .init_resource::<world_menu::LastSaved>()
        .init_resource::<save_dialog::SaveDialog>()
        .init_resource::<transport::StepsPerClick>()
        .add_systems(
            Update,
            (
                tools::tool_states,
                tools::box_select_tool,
                tools::edit_tools,
                tools::transform_tool,
                transport::transport_enabled,
                transport::play_pause_look,
                transport::clock_text,
                transport::rtf_readout,
                transport::steps_label_text,
                world_button::world_texts,
                world_button::unsaved_dot,
                world_menu::forget_last_saved,
                world_menu::world_menu_states,
                world_menu::one_line_labels,
                tools::snap_states,
                world_menu::world_saved,
                save_dialog::sync_dialog,
                save_dialog::focus_name,
                search::fit_search_placeholder,
            )
                .in_set(ShellSystems::Readouts),
        )
        .add_systems(Update, apply_start_menu.in_set(ShellSystems::Readouts))
        .add_systems(
            Update,
            world_menu::run_world_commands
                .in_set(ShellSystems::Layout)
                .after(super::commands::apply_ui_commands),
        )
        .add_observer(tools::on_snap_field)
        .add_observer(world_menu::on_world_item)
        .add_observer(save_dialog::on_dialog_button)
        .add_observer(save_dialog::on_name_submit)
        .add_observer(save_dialog::on_dialog_key)
        .add_observer(transport::on_transport)
        .add_observer(transport::on_steps_option)
        .add_observer(transport::on_custom_steps);
}

#[cfg(test)]
mod tests;
