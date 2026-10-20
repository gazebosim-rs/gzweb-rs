//! The top bar (spec §3.1): logo, world menu button, tools, transport
//! with the sim clock and RTF, and the search trigger, with two flexible
//! spacers keeping the transport centred between the left group and the
//! search. The tool and transport groups have no container: their buttons
//! sit on the bar's `surface` with the activity buttons' look, Play/Pause
//! included (DECISIONS.md D-48).

pub mod search;
pub mod tools;
pub mod transport;
pub mod world_button;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use gz_bevy::connection::GzStatus;
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::input::InputMap;

use super::format;
use super::regions::{pass_through, region, Region};
use crate::assets::brand::logo;
use crate::theme::{border, ColorRole, Fill, Theme, SPACE};
use crate::widgets::menu::shortcut_label;
use crate::widgets::{spacer, Tooltip};
use transport::{steps_label, RtfDot, RtfReadout, StepsPerClick, StepsTrigger};
use world_button::world_name;

/// Text the top bar keeps up to date.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopText {
    WorldName,
    WorldExt,
    Clock,
    Rtf,
}

fn tip(map: &InputMap, id: &str, name: &str) -> Tooltip {
    Tooltip::new(name).shortcut_opt(shortcut_label(map, id))
}

fn separator() -> impl Bundle {
    (
        Node {
            width: px(1),
            height: px(18),
            flex_shrink: 0.0,
            margin: UiRect::horizontal(px(SPACE[2])),
            ..default()
        },
        Fill(ColorRole::Line),
    )
}

/// The top bar.
pub fn top_bar(theme: &Theme, map: &InputMap) -> impl Bundle {
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
                column_gap: px(SPACE[4]),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(border::HAIRLINE)),
                ..default()
            },
            ColorRole::Surface,
        ),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((logo(t.s.logo), pass_through()));
            b.spawn(world_button::world_button(&t));
            b.spawn(separator());
            tools::spawn_tools(b, &t, &map);
            b.spawn((spacer(), pass_through()));
            b.spawn(transport::transport_group(&t, &map));
            b.spawn((spacer(), pass_through()));
            b.spawn(search::search_trigger(&t, &map));
        })),
    )
}

fn set(text: &mut Text, want: &str) {
    if text.0 != want {
        text.0 = want.to_string();
    }
}

fn set_span(text: &mut TextSpan, want: &str) {
    if text.0 != want {
        text.0 = want.to_string();
    }
}

/// The world name, clock, RTF and steps label.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn top_bar_texts(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    steps: Res<StepsPerClick>,
    mut texts: Query<(&TopText, Option<&mut Text>, Option<&mut TextSpan>)>,
    mut dots: Query<&mut Fill, With<RtfDot>>,
    mut readouts: Query<&mut Tooltip, With<RtfReadout>>,
    triggers: Query<&Children, With<StepsTrigger>>,
    mut labels: Query<&mut Text, Without<TopText>>,
) {
    let status = status.as_deref();
    let world = world_name(status, index.as_deref());
    let sim = status.and_then(|s| s.sim);
    let (rtf, role) = format::rtf(sim.as_ref());
    for (which, text, span) in &mut texts {
        let want = match which {
            TopText::WorldName => world.clone().unwrap_or_else(|| "No world".into()),
            TopText::WorldExt => world.as_ref().map_or_else(String::new, |_| ".sdf".into()),
            TopText::Clock => format::clock(sim.map_or(0.0, |s| s.sim_time)),
            TopText::Rtf => rtf.clone(),
        };
        if let Some(mut t) = text {
            set(&mut t, &want);
        }
        if let Some(mut s) = span {
            set_span(&mut s, &want);
        }
    }
    for mut fill in &mut dots {
        fill.set_if_neq(Fill(role));
    }
    let detail = format::rtf_detail(sim.as_ref());
    for mut tip in &mut readouts {
        if tip.detail.as_deref() != Some(detail.as_str()) {
            tip.detail = Some(detail.clone());
        }
    }
    if steps.is_changed() {
        let label = steps_label(steps.0);
        for children in &triggers {
            for &c in children {
                if let Ok(mut t) = labels.get_mut(c) {
                    set(&mut t, &label);
                }
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(transport::on_transport)
        .add_observer(transport::on_steps_option);
}
