//! The Teleop panel's body (spec §3.3): the target, the mode, the WASD
//! pad, the sliders or the gamepad's state, the speed limits, and a
//! slider per commandable joint. Built from the panel's resources whenever
//! the shell spawns the body (DECISIONS.md D-58, D-104); the parts that
//! depend on the target rebuild in place ([`systems`](super::systems)).

use accesskit::Role;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, RadioButton, RadioGroup};
use gz_bevy::view::layout::{KeyLayout, US};

use super::model::{DriveMode, DriveTarget, TeleopState};
use super::TeleopBody;
use crate::assets::fonts::text;
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{border, radius, ColorRole, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::button::Checkedness;
use crate::widgets::layout::{column, display, row};
use crate::widgets::{
    a11y, clickable, number_field, rounded, select, slider_row, AxisTag, NumberField, SliderProps,
};

/// A pad key's side (§3.3: 40 px).
pub(crate) const KEY: f32 = 40.0;

/// A mode tab.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ModeChoice(pub(crate) DriveMode);

/// A mode's content.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ModePane(pub(crate) DriveMode);

/// The target row's parent (rebuilt when the targets change).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TargetRow;

/// The target select: its options' topics, in order.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct TargetSelect(pub(crate) Vec<String>);

/// A pad key: the direction it drives.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PadKey {
    Forward,
    Left,
    Back,
    Right,
}

impl PadKey {
    pub(super) const ALL: [PadKey; 4] =
        [PadKey::Forward, PadKey::Left, PadKey::Back, PadKey::Right];

    /// Its drive key: a place on the keyboard, W A S D's on a US layout
    /// (shortcuts.json `teleop.drive` is positional, D-119).
    pub(super) fn key(self) -> KeyCode {
        match self {
            PadKey::Forward => KeyCode::KeyW,
            PadKey::Left => KeyCode::KeyA,
            PadKey::Back => KeyCode::KeyS,
            PadKey::Right => KeyCode::KeyD,
        }
    }

    /// What its key's cap shows on `layout` (`Z` for Forward on AZERTY).
    pub(super) fn cap(self, layout: &KeyLayout) -> String {
        layout.cap(self.key())
    }
}

/// The pad's note, which names the drive keys.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PadNote;

/// The note under the pad, with the drive keys as `layout` prints them.
pub(super) fn pad_note(layout: &KeyLayout) -> String {
    let keys: Vec<String> = PadKey::ALL.iter().map(|k| k.cap(layout)).collect();
    format!(
        "Press a key to drive; then {} drive from the view too · X stops",
        keys.join(" ")
    )
}

/// The sliders' parent (rebuilt when the limits change).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct DriveSliders;

/// A slider row: what it commands.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) enum TeleopSlider {
    Linear,
    Angular,
    /// A joint, by its command topic.
    Joint(String),
}

/// A speed limit's field.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LimitField {
    Linear,
    Angular,
}

/// The gamepad pane's status line.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct GamepadStatus;

/// The joints section's parent (rebuilt when the target changes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct JointsSection;

/// Builds the body.
pub(super) fn build_teleop(
    ev: On<Add<TeleopBody>>,
    theme: Res<Theme>,
    state: Res<TeleopState>,
    layout: Option<Res<KeyLayout>>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let state = state.clone();
    let layout = layout.as_deref().unwrap_or(&US);
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            padding: UiRect::all(px(SPACE[4])),
            ..column(SPACE[5])
        })
        .with_children(|c| {
            c.spawn((
                TargetRow,
                Node {
                    flex_shrink: 0.0,
                    ..column(SPACE[2])
                },
            ));
            c.spawn((
                RadioGroup,
                a11y(Role::TabList, "Drive mode"),
                Node {
                    padding: UiRect::all(px(SPACE[1])),
                    border_radius: rounded(radius::MD),
                    flex_shrink: 0.0,
                    ..row(SPACE[1])
                },
                crate::theme::Fill(ColorRole::Bg),
            ))
            .with_children(|tabs| {
                for m in DriveMode::ALL {
                    tabs.spawn((mode_tab(&t, m, m == state.mode), ModeChoice(m)));
                }
            });
            spawn_pad(c, &state, layout);
            c.spawn((
                ModePane(DriveMode::Sliders),
                DriveSliders,
                Node {
                    flex_shrink: 0.0,
                    display: display(state.mode == DriveMode::Sliders),
                    ..column(SPACE[2])
                },
            ));
            c.spawn((
                ModePane(DriveMode::Gamepad),
                Node {
                    flex_shrink: 0.0,
                    display: display(state.mode == DriveMode::Gamepad),
                    ..column(SPACE[2])
                },
            ))
            .with_children(|g| {
                g.spawn(text(
                    TypeStyle::SMALL,
                    ColorRole::Text2,
                    "Left stick: forward and back, and turn",
                ));
                g.spawn((GamepadStatus, text(TypeStyle::SMALL, ColorRole::Text3, "")));
            });
            spawn_limits(c, &t, &state);
            c.spawn((
                JointsSection,
                Node {
                    flex_shrink: 0.0,
                    border: UiRect::top(px(border::HAIRLINE)),
                    padding: UiRect::top(px(SPACE[4])),
                    display: Display::None,
                    ..column(SPACE[2])
                },
                Stroke(ColorRole::Line),
            ));
        });
    });
}

/// A mode tab: a radio button, `surface` when chosen.
fn mode_tab(theme: &Theme, mode: DriveMode, chosen: bool) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text2);
    visuals.active_fill = ColorRole::Surface;
    visuals.active_ink = ColorRole::Text;
    (
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(theme.s.input_h),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        RadioButton,
        clickable(visuals, Role::Tab, mode.label()),
        Checkedness(chosen),
        children![(
            text(TypeStyle::LABEL, ColorRole::Text2, mode.label()),
            Content
        )],
    )
}

/// The WASD pad: W over A S D, as the layout prints them.
fn spawn_pad(c: &mut ChildSpawnerCommands, state: &TeleopState, layout: &KeyLayout) {
    c.spawn((
        ModePane(DriveMode::Keyboard),
        Node {
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            display: display(state.mode == DriveMode::Keyboard),
            ..column(SPACE[3])
        },
    ))
    .with_children(|p| {
        p.spawn(row(SPACE[3]))
            .with_child(pad_key(PadKey::Forward, layout));
        p.spawn(row(SPACE[3])).with_children(|r| {
            for k in [PadKey::Left, PadKey::Back, PadKey::Right] {
                r.spawn(pad_key(k, layout));
            }
        });
        p.spawn((
            text(TypeStyle::SMALL, ColorRole::Text2, pad_note(layout)),
            PadNote,
        ));
    });
}

/// A 40 px pad key (§3.3: pressed = `accent_soft`).
fn pad_key(key: PadKey, layout: &KeyLayout) -> impl Bundle {
    let cap = key.cap(layout);
    let visuals = Visuals::new(ColorRole::Surface, ColorRole::Text).stroked(ColorRole::Line);
    (
        Node {
            width: px(KEY),
            height: px(KEY),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::MD),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, format!("Drive {cap}")),
        key,
        children![(text(TypeStyle::CLOCK, ColorRole::Text, cap), Content)],
    )
}

/// The drive sliders: linear and angular, up to the limits either way.
pub(super) fn spawn_sliders(p: &mut ChildSpawnerCommands, t: &Theme, state: &TeleopState) {
    #[allow(clippy::cast_possible_truncation)]
    let (ml, ma) = (state.max_lin as f32, state.max_ang as f32);
    #[allow(clippy::cast_possible_truncation)]
    let (l, a) = (state.lin as f32, state.ang as f32);
    p.spawn((
        slider_row(t, SliderProps::new("linear m/s", -ml, ml, l.clamp(-ml, ml))),
        TeleopSlider::Linear,
    ));
    p.spawn((
        slider_row(
            t,
            SliderProps::new("angular rad/s", -ma, ma, a.clamp(-ma, ma)),
        ),
        TeleopSlider::Angular,
    ));
}

/// The speed limits: max linear (m/s) and angular (rad/s).
fn spawn_limits(c: &mut ChildSpawnerCommands, t: &Theme, state: &TeleopState) {
    c.spawn(Node {
        flex_shrink: 0.0,
        align_items: AlignItems::Center,
        ..row(SPACE[4])
    })
    .with_children(|r| {
        for (label, which, value) in [
            ("max lin", LimitField::Linear, state.max_lin),
            ("max ang", LimitField::Angular, state.max_ang),
        ] {
            r.spawn(text(TypeStyle::SMALL, ColorRole::Text2, label));
            let mut field = NumberField::new(value);
            field.min = Some(0.0);
            field.precision = 2;
            field.name = Some(format!("{label} speed"));
            r.spawn(Node {
                flex_grow: 1.0,
                min_width: px(0),
                ..default()
            })
            .with_child((
                number_field(t, AxisTag::Label(String::new()), field, true),
                which,
            ));
        }
    });
}

/// The target row: "Target" and the select (or why there's none).
pub(super) fn spawn_target(
    r: &mut ChildSpawnerCommands,
    t: &Theme,
    targets: &[DriveTarget],
    chosen: Option<&str>,
) {
    r.spawn(text(TypeStyle::SMALL, ColorRole::Text2, "Target"));
    let topics: Vec<String> = targets.iter().map(|d| d.topic.clone()).collect();
    let labels: Vec<String> = targets.iter().map(DriveTarget::label).collect();
    let selected = chosen
        .and_then(|c| topics.iter().position(|t| t == c))
        .unwrap_or(0);
    r.spawn((
        select(t, "Teleop target", labels, selected, 260.0),
        TargetSelect(topics),
    ));
}

/// The joints section: its header, then a slider per joint.
pub(super) fn spawn_joints(
    s: &mut ChildSpawnerCommands,
    t: &Theme,
    target: &DriveTarget,
    state: &TeleopState,
) {
    s.spawn(Node {
        align_items: AlignItems::Baseline,
        ..row(SPACE[3])
    })
    .with_children(|h| {
        h.spawn(text(TypeStyle::SECTION, ColorRole::Text2, "Joints"));
        h.spawn(text(
            TypeStyle::MONO_SMALL,
            ColorRole::Text2,
            format!("{} · {}", target.model, target.joints.len()),
        ));
    });
    for j in &target.joints {
        let value = state.joints.get(&j.topic).copied().unwrap_or(0.0);
        #[allow(clippy::cast_possible_truncation)]
        let (lo, hi, v) = (j.lower as f32, j.upper as f32, value as f32);
        let mut props = SliderProps::new(j.joint.clone(), lo, hi, v.clamp(lo, hi));
        props.label_w = 96.0;
        s.spawn((slider_row(t, props), TeleopSlider::Joint(j.topic.clone())));
    }
}
