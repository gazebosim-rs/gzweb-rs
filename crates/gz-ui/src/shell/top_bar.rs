//! The top bar (spec §3.1): logo, world menu button, tools, transport
//! with the sim clock and RTF, and the search trigger, with two flexible
//! spacers keeping the transport centred between the left group and the
//! search. The tool and transport groups have no container: their buttons
//! sit on the bar's `surface` with the activity buttons' look, Play/Pause
//! included (DECISIONS.md D-48).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, ComputedNode, InteractionDisabled};
use bevy::ui_widgets::{
    Activate, Button as Headless, MenuAction, MenuEvent, MenuPopup, ValueChange,
};
use gz_bevy::connection::{GzCommand, GzStatus, SimStats};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::input::InputMap;

use super::connection::{CurrentPhase, Phase};
use super::{format, pass_through, region, Region, StepsPerClick, Tool, UiCommand};
use crate::brand::logo;
use crate::fonts::text;
use crate::icons::{content_icon, Icon, IconId};
use crate::theme::{border, radius, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::visuals::{Active, Visuals};
use crate::widgets::button::{text_button_parts, TextButton, ICON_BUTTON_RADIUS};
use crate::widgets::chip::dot;
use crate::widgets::menu::{menu, menu_item, menu_separator, shortcut_label, MenuSide};
use crate::widgets::{
    bar_button, bar_tool_button, button_group, clickable, rounded, spacer, Tooltip,
};
use crate::widgets::{number_field, AxisTag, NumberCommit, NumberField};

/// A tool button's tool.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolChoice(pub Tool);

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

/// Text the top bar keeps up to date.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopText {
    WorldName,
    WorldExt,
    Clock,
    Rtf,
}

/// The RTF status dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtfDot;

/// The RTF readout's hover target (its tooltip has the stats).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtfReadout;

/// The search / command bar trigger (opens ⌘K in M8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SearchTrigger;

/// The world menu button (opens the World menu in M8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WorldButton;

/// Search placeholder and key cap text: mono 11 for the cap.
pub const KEYCAP_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// Width of the search trigger (§3.1).
pub const SEARCH_W: f32 = 220.0;

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

/// The world menu button: name in `text` 500, `.sdf` in `text_3` 400, the
/// unsaved dot when dirty (never yet), chevron.
fn world_button(theme: &Theme) -> impl Bundle {
    let name = TypeStyle::new(FontFamily::Ui, 15.0, 500);
    let ext = TypeStyle::new(FontFamily::Ui, 15.0, 400);
    let icon_px = theme.s.icon_sm;
    (
        WorldButton,
        Node {
            height: px(theme.s.text_btn_h),
            padding: UiRect::horizontal(px(9)),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            border_radius: rounded(ICON_BUTTON_RADIUS),
            ..default()
        },
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text),
            Role::Button,
            "World menu",
        ),
        Tooltip::new("World").detail("World menu"),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(Node {
                column_gap: px(SPACE[2] + 1.0),
                align_items: AlignItems::Baseline,
                ..default()
            })
            .with_children(|n| {
                n.spawn((text(name, ColorRole::Text, "No world"), TopText::WorldName));
                n.spawn((text(ext, ColorRole::Text3, ""), TopText::WorldExt));
            });
            p.spawn(content_icon(IconId::ChevronDown, icon_px, ColorRole::Text));
        })),
    )
}

/// The steps menu's choices (§3.1), before "Custom".
pub const STEPS_CHOICES: [u32; 3] = [1, 10, 100];
/// The most iterations a custom step count asks for.
pub const MAX_STEPS: u32 = 100_000;

/// The steps-per-click menu: "1 step ▾" with 1, 10 and 100, the current
/// one marked, and a Custom field (§3.1, §5.14).
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
            for n in STEPS_CHOICES {
                let mut item = p.spawn((menu_item(&t, steps_label(n), None, None), StepsOption(n)));
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

/// The steps trigger.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StepsTrigger;

/// The steps menu's Custom field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CustomSteps;

/// "1 step", "10 steps".
pub fn steps_label(n: u32) -> String {
    if n == 1 {
        "1 step".into()
    } else {
        format!("{n} steps")
    }
}

fn search_trigger(theme: &Theme, map: &InputMap) -> impl Bundle {
    let cap = shortcut_label(map, "ui.command_palette").unwrap_or_default();
    let icon_px = theme.s.icon_sm;
    let visuals = Visuals::new(ColorRole::Field, ColorRole::Text3)
        .stroked(ColorRole::Line)
        .keep_ink();
    (
        SearchTrigger,
        Node {
            width: px(SEARCH_W),
            height: px(theme.s.input_h),
            // Narrow windows squeeze the search first (down to its icon).
            flex_shrink: 1.0,
            min_width: px(theme.s.input_h + SPACE[4]),
            padding: UiRect {
                left: px(SPACE[5]),
                right: px(SPACE[3]),
                ..default()
            },
            column_gap: px(SPACE[4]),
            border: UiRect::all(px(border::HAIRLINE)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            border_radius: rounded(radius::MD),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, "Search or run a command"),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(content_icon(IconId::Search, icon_px, ColorRole::Text3));
            // Text can't clip itself: a clipping box around it.
            p.spawn(Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                min_width: px(0),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_child((
                text(TypeStyle::BODY, ColorRole::Text3, SEARCH_PLACEHOLDERS[0]),
                SearchPlaceholder::default(),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            p.spawn((
                Node {
                    padding: UiRect::axes(px(SPACE[2] + 1.0), px(1)),
                    border: UiRect::all(px(border::HAIRLINE)),
                    flex_shrink: 0.0,
                    border_radius: rounded(radius::XS),
                    ..default()
                },
                Stroke(ColorRole::Line),
                children![text(KEYCAP_TYPE, ColorRole::Text3, cap)],
            ));
        })),
    )
}

/// The search trigger's placeholder, longest first: the longest that fits
/// shows, so a narrow trigger (or a wide key cap, "Ctrl+K") never cuts a
/// word (bevy_ui text has no ellipsis).
pub const SEARCH_PLACEHOLDERS: [&str; 3] = ["Search or run a command", "Search…", ""];

/// The search placeholder: the widths its texts measured, and which shows.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchPlaceholder {
    pub widths: [Option<f32>; SEARCH_PLACEHOLDERS.len()],
    pub shown: usize,
}

impl SearchPlaceholder {
    /// The text to show in `room` logical pixels: the longest known to fit,
    /// or a longer one not measured yet (tried, then measured).
    pub fn pick(&self, room: f32) -> usize {
        self.widths
            .iter()
            .position(|w| w.is_none_or(|w| w <= room + 0.5))
            .unwrap_or(SEARCH_PLACEHOLDERS.len() - 1)
    }
}

/// Measures the placeholder shown (its box doesn't shrink, so its width is
/// the text's) against the clipping box around it, and swaps texts.
pub fn fit_search_placeholder(
    mut texts: Query<(&mut Text, &mut SearchPlaceholder, &ComputedNode, &ChildOf)>,
    boxes: Query<&ComputedNode>,
) {
    for (mut text, mut fit, node, parent) in &mut texts {
        let Ok(room) = boxes.get(parent.parent()) else {
            continue;
        };
        let k = node.inverse_scale_factor();
        let width = node.size().x * k;
        if width <= 0.0 && fit.shown != SEARCH_PLACEHOLDERS.len() - 1 {
            // Not laid out yet.
            continue;
        }
        let shown = fit.shown;
        if fit.widths[shown] != Some(width) {
            fit.widths[shown] = Some(width);
        }
        let pick = fit.pick(room.size().x * room.inverse_scale_factor());
        if pick != shown {
            fit.shown = pick;
            set(&mut text, SEARCH_PLACEHOLDERS[pick]);
        }
    }
}

fn transport_group(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    (
        button_group("Simulation", false),
        Children::spawn(SpawnWith(move |g: &mut ChildSpawner| {
            g.spawn((
                bar_button(&t, IconId::Reset, tip(&map, "sim.reset", "Reset world")),
                Transport::Reset,
            ));
            g.spawn((
                bar_button(&t, IconId::Play, tip(&map, "sim.play_pause", "Play")),
                Transport::PlayPause,
            ));
            g.spawn((
                bar_button(&t, IconId::Step, tip(&map, "sim.step", "Step")),
                Transport::Step,
            ));
            g.spawn(steps_menu(&t));
            g.spawn(separator());
            g.spawn(Node {
                padding: UiRect {
                    left: px(SPACE[3]),
                    right: px(SPACE[5]),
                    ..default()
                },
                column_gap: px(SPACE[6]),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    text(TypeStyle::CLOCK, ColorRole::Text, format::clock(0.0)),
                    TopText::Clock,
                ));
                r.spawn((
                    RtfReadout,
                    Hovered::default(),
                    Tooltip::new("Real-time factor").detail(format::rtf_detail(None)),
                    Node {
                        column_gap: px(SPACE[2] + 1.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ))
                .with_children(|r| {
                    r.spawn((dot(t.s.status_dot, ColorRole::Disabled), RtfDot));
                    r.spawn((
                        text(TypeStyle::MONO_SMALL, ColorRole::Text2, "RTF —"),
                        TopText::Rtf,
                    ));
                });
            });
        })),
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
            b.spawn(world_button(&t));
            b.spawn(separator());
            b.spawn(button_group("Tools", true))
                .with_children(|g| {
                    for tool in Tool::ALL {
                        let id = match tool {
                            Tool::Navigate => IconId::Navigate,
                            Tool::BoxSelect => IconId::BoxSelect,
                            Tool::Translate => IconId::Translate,
                            Tool::Rotate => IconId::Rotate,
                        };
                        g.spawn((
                            bar_tool_button(
                                &t,
                                id,
                                tip(&map, tool.binding(), tool.name()),
                                tool == Tool::Navigate,
                            ),
                            ToolChoice(tool),
                        ));
                    }
                    g.spawn((
                        text_button_parts(&t, TextButton::new("Snap 0.25").mono().chevron()),
                        Headless,
                        Tooltip::new("Snap"),
                    ));
                })
                .observe(on_tool_change);
            b.spawn((spacer(), pass_through()));
            b.spawn(transport_group(&t, &map));
            b.spawn((spacer(), pass_through()));
            b.spawn(search_trigger(&t, &map));
        })),
    )
}

/// A tool button click selects the tool.
pub fn on_tool_change(
    ev: On<ValueChange<Entity>>,
    choices: Query<&ToolChoice>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(choice) = choices.get(ev.value) {
        out.write(UiCommand::SetTool(choice.0));
    }
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

/// Enter in the Custom field sets the count and closes the menu. A commit
/// from leaving the field (the menu closing on a click elsewhere, or on
/// another choice) is dropped: dismissing a menu doesn't choose.
pub fn on_custom_steps(
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

/// The tool buttons show [`Tool`] (keys change it too).
pub fn tool_states(
    tool: Res<Tool>,
    buttons: Query<(Entity, &ToolChoice, Has<Checked>)>,
    mut commands: Commands,
) {
    if !tool.is_changed() {
        return;
    }
    for (e, choice, checked) in &buttons {
        let want = choice.0 == *tool;
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
    }
}

/// Whether a transport control works now (DECISIONS.md D-34, D-37): Reset
/// needs a live world; Play/Pause also the sim's stats (they say which
/// way to toggle); Step a paused sim. The inspector's Reset uses it too.
pub fn transport_enabled(which: Transport, phase: Phase, sim: Option<&SimStats>) -> bool {
    let live = phase == Phase::Live;
    match which {
        Transport::Reset => live,
        Transport::PlayPause => live && sim.is_some(),
        Transport::Step => live && sim.is_some_and(|s| s.paused),
    }
}

/// Play/Pause shows `pause` while running, `play` while paused, and is
/// named so to AccessKit; [`transport_enabled`] says which work.
#[allow(clippy::type_complexity)]
pub fn transport_states(
    status: Option<Res<GzStatus>>,
    phase: Res<CurrentPhase>,
    mut buttons: Query<(
        Entity,
        &Transport,
        &mut Tooltip,
        &mut AccessibilityNode,
        &Children,
        Has<InteractionDisabled>,
    )>,
    mut icons: Query<&mut Icon>,
    mut commands: Commands,
) {
    if !phase.is_changed() && !status.as_ref().is_some_and(Res::is_changed) {
        return;
    }
    let sim = status.and_then(|s| s.sim);
    let running = sim.is_some_and(|s| !s.paused);
    for (e, which, mut tip, mut access, children, disabled) in &mut buttons {
        let enabled = transport_enabled(*which, phase.0, sim.as_ref());
        if enabled == disabled {
            if enabled {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
        if *which == Transport::PlayPause {
            let (id, title) = if running {
                (IconId::Pause, "Pause")
            } else {
                (IconId::Play, "Play")
            };
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

/// The world name, clock, RTF and its tooltip (live while open), and the
/// steps label, its accessible name and the menu's current choice; run
/// when the stats, the scene or the count change.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn top_bar_texts(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    steps: Res<StepsPerClick>,
    mut texts: Query<(&TopText, Option<&mut Text>, Option<&mut TextSpan>)>,
    mut dots: Query<&mut Fill, With<RtfDot>>,
    mut readouts: Query<&mut Tooltip, With<RtfReadout>>,
    mut triggers: Query<(&Children, &mut AccessibilityNode), With<StepsTrigger>>,
    mut labels: Query<&mut Text, Without<TopText>>,
    mut options: Query<(Entity, &StepsOption, Has<Active>)>,
    mut custom: Query<&mut NumberField, With<CustomSteps>>,
    mut commands: Commands,
) {
    if steps.is_changed() {
        let label = steps_label(steps.0);
        for (children, mut access) in &mut triggers {
            for &c in children {
                if let Ok(mut t) = labels.get_mut(c) {
                    set(&mut t, &label);
                }
            }
            if access.label() != Some(label.as_str()) {
                access.set_label(label.as_str());
            }
        }
        for (e, option, active) in &mut options {
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
    let changed =
        status.as_ref().is_some_and(Res::is_changed) || index.as_ref().is_some_and(Res::is_changed);
    if !changed {
        return;
    }
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
}

/// The world's name: the server's world, else the scene's.
pub fn world_name(status: Option<&GzStatus>, index: Option<&GzIndex>) -> Option<String> {
    status
        .and_then(|s| s.world.clone())
        .or_else(|| index.and_then(|i| i.scene.as_ref().map(|s| s.name.clone())))
        .filter(|n| !n.is_empty())
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_transport)
        .add_observer(on_steps_option)
        .add_observer(on_custom_steps);
}
