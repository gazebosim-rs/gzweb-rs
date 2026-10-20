//! The transport (spec §3.1): Reset, Play/Pause, Step and the steps per
//! click, then the sim clock and the RTF readout. Play/Pause follows the
//! stats; Step steps a paused sim. Disabled without a live world.

use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, MenuAction, MenuEvent, MenuPopup};
use gz_bevy::connection::{GzCommand, GzStatus, SimStats};
use gz_bevy::view::input::InputMap;

use crate::assets::fonts::text;
use crate::assets::icons::{Icon, IconId};
use crate::shell::connection::{CurrentPhase, Phase};
use crate::theme::visuals::Active;
use crate::theme::{ColorRole, Fill, Theme, TypeStyle, SPACE};
use crate::widgets::button::{text_button_parts, TextButtonProps};
use crate::widgets::chip::dot;
use crate::widgets::format;
use crate::widgets::layout::{row, separator, Orientation, BAR_SEPARATOR_H};
use crate::widgets::menu::{menu, menu_item, menu_separator, MenuSide};
use crate::widgets::text::set_text;
use crate::widgets::MenuItemProps;
use crate::widgets::{bar_button, button_group, Tooltip};
use crate::widgets::{number_field, AxisTag, NumberCommit, NumberField};

/// Iterations per Step click and ⌘→ (§3.1 "1 step ▾"); `gz-bevy`'s, since
/// its key handling steps by it too (DECISIONS.md D-54).
pub(crate) use gz_bevy::connection::StepsPerClick;

/// The transport's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Reset,
    PlayPause,
    Step,
}

impl Transport {
    /// Whether the control works now (DECISIONS.md D-34, D-56): Reset needs
    /// a live world (as the World inspector's Reset); Play/Pause also the
    /// sim's stats (they say which way to toggle); Step a paused sim.
    pub(crate) fn enabled(self, phase: Phase, sim: Option<&SimStats>) -> bool {
        let live = phase == Phase::Live;
        match self {
            Transport::Reset => live,
            Transport::PlayPause => live && sim.is_some(),
            Transport::Step => live && sim.is_some_and(|s| s.paused),
        }
    }
}

/// A "Steps per click" menu item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StepsOption(pub u32);

/// The steps trigger.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct StepsTrigger;

/// The steps menu's Custom field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CustomSteps;

/// The steps menu's choices (§3.1), before "Custom".
pub(crate) const STEPS_CHOICES: [u32; 3] = [1, 10, 100];
/// The most iterations a custom step count asks for.
pub(crate) const MAX_STEPS: u32 = 100_000;

/// The sim clock's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SimClock;

/// The RTF readout's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct RtfText;

/// The RTF status dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct RtfDot;

/// The RTF readout's hover target (its tooltip has the stats).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct RtfReadout;

/// The steps-per-click menu: "1 step ▾" with 1, 10 and 100, the current
/// one marked, and a Custom field (§3.1, §5.14; DECISIONS.md D-54).
fn steps_menu(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    menu(
        theme,
        MenuSide::Below,
        (
            text_button_parts(theme, TextButtonProps::new("1 step").mono().chevron()),
            Tooltip::new("Steps per click"),
            StepsTrigger,
        ),
        move |p: &mut ChildSpawner| {
            for n in STEPS_CHOICES {
                let mut item = p.spawn((
                    menu_item(&t, MenuItemProps::new(steps_label(n))),
                    StepsOption(n),
                ));
                if n == StepsPerClick::default().0 {
                    item.insert(Active);
                }
            }
            p.spawn(menu_separator());
            p.spawn(Node {
                padding: UiRect::axes(px(SPACE[5]), px(SPACE[2])),
                ..default()
            })
            .with_child((
                number_field(
                    &t,
                    AxisTag::Label("Custom".into()),
                    NumberField::new(1.0)
                        .precision(0, 1.0)
                        .range(1.0, f64::from(MAX_STEPS))
                        .named("Custom steps per click"),
                    true,
                ),
                CustomSteps,
            ));
        },
    )
}

/// "1 step", "10 steps".
pub(crate) fn steps_label(n: u32) -> String {
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
pub(crate) fn on_transport(
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
pub(crate) fn on_steps_option(
    ev: On<Activate>,
    options: Query<&StepsOption>,
    mut steps: ResMut<StepsPerClick>,
) {
    if let Ok(o) = options.get(ev.entity) {
        steps.set_if_neq(StepsPerClick(o.0));
    }
}

/// Enter in the Custom field sets the count and closes the menu. A commit
/// from leaving the field (the menu closing on a click elsewhere, or on
/// another choice) is dropped: dismissing a menu doesn't choose.
pub(crate) fn on_custom_steps(
    ev: On<NumberCommit>,
    fields: Query<(), With<CustomSteps>>,
    parents: Query<&ChildOf>,
    popups: Query<&Visibility, With<MenuPopup>>,
    mut steps: ResMut<StepsPerClick>,
    mut commands: Commands,
) {
    if !ev.is_final || !fields.contains(ev.entity) {
        return;
    }
    let open = parents
        .iter_ancestors(ev.entity)
        .any(|a| popups.get(a).is_ok_and(|v| *v == Visibility::Visible));
    if !open {
        return;
    }
    let n = ev.value.round().clamp(1.0, f64::from(MAX_STEPS)) as u32;
    steps.set_if_neq(StepsPerClick(n));
    for action in [MenuAction::FocusRoot, MenuAction::CloseAll] {
        commands.trigger(MenuEvent {
            source: ev.entity,
            action,
        });
    }
}

/// Which transport controls work ([`Transport::enabled`]), when the
/// connection or the stats change.
pub(crate) fn transport_enabled(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    buttons: Query<(Entity, &Transport, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if !phase.is_changed() && !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let sim = status.and_then(|s| s.sim);
    for (e, which, disabled) in &buttons {
        let enabled = which.enabled(phase.0, sim.as_ref());
        if enabled == disabled {
            if enabled {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}

/// Play/Pause shows `pause` (and says Pause, in its tooltip and to
/// AccessKit) while running, `play` while paused.
pub(crate) fn play_pause_look(
    status: Option<Res<GzStatus>>,
    mut buttons: Query<(&Transport, &mut Tooltip, &mut AccessibilityNode, &Children)>,
    mut icons: Query<&mut Icon>,
) {
    if !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let running = status.and_then(|s| s.sim).is_some_and(|s| !s.paused);
    let (id, title) = if running {
        (IconId::Pause, "Pause")
    } else {
        (IconId::Play, "Play")
    };
    for (which, mut tip, mut access, children) in &mut buttons {
        if *which != Transport::PlayPause {
            continue;
        }
        if tip.title != title {
            tip.title = title.into();
        }
        if access.label() != Some(title) {
            access.set_label(title);
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
pub(crate) fn clock_text(
    status: Option<Res<GzStatus>>,
    mut clocks: Query<&mut Text, With<SimClock>>,
) {
    if !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let sim = status.and_then(|s| s.sim);
    let want = format::clock(sim.map_or(0.0, |s| s.sim_time));
    for mut t in &mut clocks {
        set_text(&mut t, &want);
    }
}

/// The RTF readout (§3.1): its text, its dot's colour and its tooltip's
/// stats (live while the tooltip is open).
pub(crate) fn rtf_readout(
    status: Option<Res<GzStatus>>,
    mut texts: Query<&mut Text, With<RtfText>>,
    mut dots: Query<&mut Fill, With<RtfDot>>,
    mut readouts: Query<&mut Tooltip, With<RtfReadout>>,
) {
    if !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
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

/// The steps trigger says the steps per click ("10 steps"), as text and
/// to AccessKit; the menu marks the choice and its Custom field shows it.
pub(crate) fn steps_label_text(
    steps: Res<StepsPerClick>,
    mut triggers: Query<(&Children, &mut AccessibilityNode), With<StepsTrigger>>,
    mut labels: Query<&mut Text>,
    options: Query<(Entity, &StepsOption, Has<Active>)>,
    mut custom: Query<&mut NumberField, With<CustomSteps>>,
    mut commands: Commands,
) {
    if !steps.is_changed() {
        return;
    }
    let label = steps_label(steps.0);
    for (children, mut access) in &mut triggers {
        for &c in children {
            if let Ok(mut t) = labels.get_mut(c) {
                set_text(&mut t, &label);
            }
        }
        if access.label() != Some(label.as_str()) {
            access.set_label(label.as_str());
        }
    }
    for (e, option, active) in &options {
        if (option.0 == steps.0) != active {
            if active {
                commands.entity(e).remove::<Active>();
            } else {
                commands.entity(e).insert(Active);
            }
        }
    }
    for mut field in &mut custom {
        let v = f64::from(steps.0);
        if field.value != v {
            field.value = v;
        }
    }
}
