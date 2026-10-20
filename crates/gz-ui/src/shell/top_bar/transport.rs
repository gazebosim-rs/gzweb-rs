//! The transport (spec §3.1): Reset, Play/Pause, Step and the steps per
//! click, then the sim clock and the RTF readout. Play/Pause follows the
//! stats; Step steps a paused sim. Disabled without a live world.

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::view::input::InputMap;

use super::BAR_SEPARATOR_H;
use crate::assets::fonts::text;
use crate::assets::icons::{Icon, IconId};
use crate::shell::connection::{CurrentPhase, Phase};
use crate::shell::format;
use crate::theme::{ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::button::{text_button_parts, TextButton};
use crate::widgets::chip::dot;
use crate::widgets::layout::{row, separator, Orientation};
use crate::widgets::menu::{menu, menu_item, MenuSide};
use crate::widgets::text::set_text;
use crate::widgets::{bar_button, button_group, Tooltip};

/// Iterations per Step click (§3.1 "1 step ▾").
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepsPerClick(pub u32);

impl Default for StepsPerClick {
    fn default() -> Self {
        StepsPerClick(1)
    }
}

/// The transport's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Reset,
    PlayPause,
    Step,
}

/// A "Steps per click" menu item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepsOption(pub u32);

/// The steps trigger.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StepsTrigger;

/// The sim clock's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SimClock;

/// The RTF readout's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtfText;

/// The RTF status dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtfDot;

/// The RTF readout's hover target (its tooltip has the stats).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtfReadout;

/// The steps-per-click menu: "1 step ▾" with 1, 10 and 100.
fn steps_menu(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    menu(
        theme,
        MenuSide::Below,
        (
            text_button_parts(theme, TextButton::new("1 step").mono().chevron()),
            Tooltip::new("Steps per click"),
            StepsTrigger,
        ),
        move |p: &mut ChildSpawner| {
            for n in [1, 10, 100] {
                p.spawn((menu_item(&t, steps_label(n), None, None), StepsOption(n)));
            }
        },
    )
}

/// "1 step", "10 steps".
pub fn steps_label(n: u32) -> String {
    if n == 1 {
        "1 step".into()
    } else {
        format!("{n} steps")
    }
}

pub(super) fn transport_group(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        button_group("Simulation", false),
        Children::spawn(SpawnWith(move |g: &mut ChildSpawner| {
            g.spawn((
                bar_button(
                    &t,
                    IconId::Reset,
                    Tooltip::new("Reset world").binding(&map, "sim.reset"),
                ),
                Transport::Reset,
            ));
            g.spawn((
                bar_button(
                    &t,
                    IconId::Play,
                    Tooltip::new("Play").binding(&map, "sim.play_pause"),
                ),
                Transport::PlayPause,
            ));
            g.spawn((
                bar_button(
                    &t,
                    IconId::Step,
                    Tooltip::new("Step").binding(&map, "sim.step"),
                ),
                Transport::Step,
            ));
            g.spawn(steps_menu(&t));
            g.spawn(separator(Orientation::Vertical, BAR_SEPARATOR_H, SPACE[2]));
            g.spawn(Node {
                padding: UiRect {
                    left: px(SPACE[3]),
                    right: px(SPACE[5]),
                    ..default()
                },
                flex_shrink: 0.0,
                ..row(SPACE[6])
            })
            .with_children(|r| {
                r.spawn((
                    text(TypeStyle::CLOCK, ColorRole::Text, format::clock(0.0)),
                    SimClock,
                ));
                r.spawn((
                    RtfReadout,
                    Hovered::default(),
                    Tooltip::new("Real-time factor").detail(format::rtf_detail(None)),
                    row(SPACE[2] + 1.0),
                ))
                .with_children(|r| {
                    r.spawn((dot(t.s.status_dot, ColorRole::Disabled), RtfDot));
                    r.spawn((
                        text(TypeStyle::MONO_SMALL, ColorRole::Text2, "RTF —"),
                        RtfText,
                    ));
                });
            });
        })),
    )
}

/// Transport clicks → sim commands. Play/Pause follows the stats; Step
/// steps a paused sim by the steps per click.
pub fn on_transport(
    ev: On<Activate>,
    buttons: Query<(&Transport, Has<InteractionDisabled>)>,
    status: Option<Res<GzStatus>>,
    steps: Res<StepsPerClick>,
    mut out: MessageWriter<GzCommand>,
) {
    let Ok((button, disabled)) = buttons.get(ev.entity) else {
        return;
    };
    if disabled {
        return;
    }
    let sim = status.and_then(|s| s.sim);
    match button {
        Transport::Reset => {
            out.write(GzCommand::Reset);
        }
        Transport::PlayPause => {
            if let Some(sim) = sim {
                out.write(if sim.paused {
                    GzCommand::Play
                } else {
                    GzCommand::Pause
                });
            }
        }
        Transport::Step => {
            if sim.is_some_and(|s| s.paused) {
                out.write(GzCommand::Step(steps.0));
            }
        }
    }
}

/// A steps-per-click item sets the count.
pub fn on_steps_option(
    ev: On<Activate>,
    options: Query<&StepsOption>,
    mut steps: ResMut<StepsPerClick>,
) {
    if let Ok(o) = options.get(ev.entity) {
        steps.set_if_neq(StepsPerClick(o.0));
    }
}

/// The transport is disabled without a live world, and Step while the
/// sim runs.
pub fn transport_enabled(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    buttons: Query<(Entity, &Transport, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let sim = status.and_then(|s| s.sim);
    let live = phase.0 == Phase::Live && sim.is_some();
    let running = sim.is_some_and(|s| !s.paused);
    for (e, which, disabled) in &buttons {
        let enabled = match which {
            Transport::Step => live && !running,
            _ => live,
        };
        if enabled == disabled {
            if enabled {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}

/// Play/Pause shows `pause` (and says Pause) while running, `play` while
/// paused.
pub fn play_pause_look(
    status: Option<Res<GzStatus>>,
    mut buttons: Query<(&Transport, &mut Tooltip, &Children)>,
    mut icons: Query<&mut Icon>,
) {
    let running = status.and_then(|s| s.sim).is_some_and(|s| !s.paused);
    let (id, title) = if running {
        (IconId::Pause, "Pause")
    } else {
        (IconId::Play, "Play")
    };
    for (which, mut tip, children) in &mut buttons {
        if *which != Transport::PlayPause {
            continue;
        }
        if tip.title != title {
            tip.title = title.into();
        }
        for &c in children {
            if let Ok(mut icon) = icons.get_mut(c) {
                if icon.id != id {
                    icon.id = id;
                }
            }
        }
    }
}

/// The sim clock (§3.1).
pub fn clock_text(status: Option<Res<GzStatus>>, mut clocks: Query<&mut Text, With<SimClock>>) {
    let sim = status.and_then(|s| s.sim);
    let want = format::clock(sim.map_or(0.0, |s| s.sim_time));
    for mut t in &mut clocks {
        set_text(&mut t, &want);
    }
}

/// The RTF readout (§3.1): its text, its dot's colour and its tooltip's
/// stats.
pub fn rtf_readout(
    status: Option<Res<GzStatus>>,
    mut texts: Query<&mut Text, With<RtfText>>,
    mut dots: Query<&mut Fill, With<RtfDot>>,
    mut readouts: Query<&mut Tooltip, With<RtfReadout>>,
) {
    let sim = status.and_then(|s| s.sim);
    let (rtf, role) = format::rtf(sim.as_ref());
    for mut t in &mut texts {
        set_text(&mut t, &rtf);
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
}

/// The steps trigger says the steps per click ("10 steps").
pub fn steps_label_text(
    steps: Res<StepsPerClick>,
    triggers: Query<&Children, With<StepsTrigger>>,
    mut labels: Query<&mut Text>,
) {
    if !steps.is_changed() {
        return;
    }
    let label = steps_label(steps.0);
    for children in &triggers {
        for &c in children {
            if let Ok(mut t) = labels.get_mut(c) {
                set_text(&mut t, &label);
            }
        }
    }
}
