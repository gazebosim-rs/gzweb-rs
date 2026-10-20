//! The top bar (spec §3.1): logo, world menu button, tools, transport
//! with the sim clock and RTF, and the search trigger, with two flexible
//! spacers keeping the transport centred between the left group and the
//! search. The tool and transport groups have no container: their buttons
//! sit on the bar's `surface` with the activity buttons' look, Play/Pause
//! included (DECISIONS.md D-48).

pub(crate) mod search;
pub(crate) mod tools;
pub(crate) mod transport;
pub(crate) mod world_button;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use super::regions::{pass_through, region, Region};
use super::ShellSystems;
use crate::assets::brand::logo;
use crate::theme::{border, ColorRole, Theme, SPACE};
use crate::widgets::layout::{flex_spacer, row, separator, Orientation};

/// The length of the top bar's separators between its groups.
const BAR_SEPARATOR_H: f32 = 18.0;

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
            b.spawn(world_button::world_button(&t));
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
    app.init_resource::<tools::Tool>()
        .init_resource::<transport::StepsPerClick>()
        .add_systems(
            Update,
            (
                tools::tool_states,
                transport::transport_enabled,
                transport::play_pause_look,
                transport::clock_text,
                transport::rtf_readout,
                transport::steps_label_text,
                world_button::world_texts,
            )
                .in_set(ShellSystems::Readouts),
        )
        .add_observer(transport::on_transport)
        .add_observer(transport::on_steps_option);
}

#[cfg(test)]
mod tests;
