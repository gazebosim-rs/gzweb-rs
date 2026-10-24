//! The top bar's tools (spec §3.1, §6.1): Navigate, Box select, Translate
//! and Rotate as a radio group of bar buttons, and the Snap popover
//! (§7.6; DECISIONS.md D-108): the translate step (m) and the rotate step
//! (°) that Shift snaps the gizmo to; the scale step is reserved and the
//! gizmo moves along the world's axes, so Local waits.

use bevy::a11y::AccessibilityNode;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::ValueChange;
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::edits_allowed;
use gz_bevy::view::gizmo::{Snap, TransformTool};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::BoxSelectTool;

use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::shell::commands::UiCommand;
use crate::theme::{ColorRole, Theme, TypeStyle, SPACE};
use crate::widgets::button::{text_button_parts, TextButtonProps};
use crate::widgets::layout::row;
use crate::widgets::menu::{menu, menu_group_label, MenuSide};
use crate::widgets::text::set_text;
use crate::widgets::{
    bar_tool_button, button_group, number_field, AxisTag, NumberCommit, NumberField, Tooltip, Unit,
};

/// The top-bar tools (spec §6.1). Translate and Rotate edit, so they need
/// a server that takes edits (DECISIONS.md D-76).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum Tool {
    #[default]
    Navigate,
    BoxSelect,
    Translate,
    Rotate,
}

impl Tool {
    /// The tool changes the world (its gizmo sends poses).
    pub fn edits(self) -> bool {
        matches!(self, Tool::Translate | Tool::Rotate)
    }

    pub const ALL: [Tool; 4] = [
        Tool::Navigate,
        Tool::BoxSelect,
        Tool::Translate,
        Tool::Rotate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Navigate => "Navigate",
            Tool::BoxSelect => "Box select",
            Tool::Translate => "Translate",
            Tool::Rotate => "Rotate",
        }
    }

    /// Its `shortcuts.json` binding.
    pub fn binding(self) -> &'static str {
        match self {
            Tool::Navigate => "tool.navigate",
            Tool::BoxSelect => "tool.box_select",
            Tool::Translate => "tool.translate",
            Tool::Rotate => "tool.rotate",
        }
    }

    /// The status bar's hint for it (§3.6 item 7).
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Navigate => "Click select · Drag orbit · Right-drag pan · Wheel zoom",
            Tool::BoxSelect => "Drag a box to select · Shift adds · Esc to finish",
            Tool::Translate => "Drag an axis to move · Shift to snap · Esc to finish",
            Tool::Rotate => "Drag a ring to rotate · Shift to snap · Esc to finish",
        }
    }

    pub(crate) fn icon(self) -> IconId {
        match self {
            Tool::Navigate => IconId::Navigate,
            Tool::BoxSelect => IconId::BoxSelect,
            Tool::Translate => IconId::Translate,
            Tool::Rotate => IconId::Rotate,
        }
    }
}

/// A tool button's tool.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolChoice(pub Tool);

/// Spawns the tool group: a radio [`button_group`] of the tools, then Snap.
pub(super) fn spawn_tools(b: &mut ChildSpawner, t: &Theme, map: &InputMap) {
    b.spawn(button_group("Tools", true))
        .with_children(|g| {
            for tool in Tool::ALL {
                g.spawn((
                    bar_tool_button(
                        t,
                        tool.icon(),
                        Tooltip::new(tool.name()).binding(map, tool.binding()),
                        tool == Tool::Navigate,
                    ),
                    ToolChoice(tool),
                ));
            }
        })
        .observe(on_tool_change);
    b.spawn(snap_menu(t, &Snap::default()));
}

/// The Snap popover's trigger ("Snap 0.25").
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SnapTrigger;

/// The Snap popover's fields.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapField {
    /// The translate step, m.
    Length,
    /// The rotate step, °.
    Angle,
}

/// "Snap 0.25": the translate step, as short as it reads.
pub(crate) fn snap_label(snap: &Snap) -> String {
    let s = format!("{:.3}", snap.length);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("Snap {s}")
}

/// A labelled row of the popover.
fn snap_row(p: &mut ChildSpawner, label: &str, field: impl Bundle) {
    p.spawn(Node {
        align_items: AlignItems::Center,
        padding: UiRect::axes(px(SPACE[5]), px(SPACE[2])),
        ..row(SPACE[5])
    })
    .with_children(|r| {
        r.spawn((
            text(TypeStyle::SMALL, ColorRole::Text2, label.to_string()),
            Node {
                width: px(SNAP_LABEL_W),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        r.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            ..default()
        })
        .with_child(field);
    });
}

/// The labels' column width.
const SNAP_LABEL_W: f32 = 96.0;
/// The popover's content width: room for a field that reads "0.125 m".
const SNAP_W: f32 = 260.0;

/// The Snap popover (§7.6).
fn snap_menu(theme: &Theme, snap: &Snap) -> impl Bundle {
    let t = theme.clone();
    let snap = *snap;
    menu(
        theme,
        MenuSide::Below,
        (
            text_button_parts(
                theme,
                TextButtonProps::new(snap_label(&snap)).mono().chevron(),
            ),
            Tooltip::new("Snap").detail("Shift snaps a gizmo drag to these steps"),
            SnapTrigger,
        ),
        move |p: &mut ChildSpawner| {
            p.spawn(Node {
                width: px(SNAP_W),
                ..default()
            });
            p.spawn(menu_group_label("Snap"));
            let length = NumberField::new(f64::from(snap.length))
                .unit(Unit::Length)
                .precision(3, 0.05)
                .range(0.001, 1000.0)
                .named("Translate step");
            snap_row(
                p,
                "Translate step",
                (
                    number_field(&t, AxisTag::Label(String::new()), length, true),
                    SnapField::Length,
                ),
            );
            let angle = NumberField::new(f64::from(snap.angle.to_degrees()))
                .unit(Unit::Angle)
                .precision(1, 5.0)
                .range(0.1, 180.0)
                .named("Rotate step");
            snap_row(
                p,
                "Rotate step",
                (
                    number_field(&t, AxisTag::Label(String::new()), angle, true),
                    SnapField::Angle,
                ),
            );
            snap_row(
                p,
                "Scale step",
                (
                    text(TypeStyle::SMALL, ColorRole::Text3, "Reserved"),
                    Tooltip::new("Scale step").detail("Reserved: there's no scale tool yet"),
                ),
            );
            snap_row(
                p,
                "Space",
                (
                    text(TypeStyle::SMALL, ColorRole::Text, "World"),
                    Tooltip::new("Space: World")
                        .detail("Local waits: the gizmo moves along the world's axes"),
                ),
            );
        },
    )
}

/// A field sets its step (a degree field in radians underneath).
pub(crate) fn on_snap_field(
    ev: On<NumberCommit>,
    fields: Query<&SnapField>,
    snap: Option<ResMut<Snap>>,
) {
    let (Ok(field), Some(mut snap)) = (fields.get(ev.entity), snap) else {
        return;
    };
    #[allow(clippy::cast_possible_truncation)]
    let v = ev.value as f32;
    let mut next = *snap;
    match field {
        SnapField::Length => next.length = v.max(0.001),
        SnapField::Angle => next.angle = v.clamp(0.1, 180.0).to_radians(),
    }
    snap.set_if_neq(next);
}

/// The trigger reads the translate step, and the fields the steps.
pub(crate) fn snap_states(
    snap: Option<Res<Snap>>,
    mut triggers: Query<(&Children, &mut AccessibilityNode), With<SnapTrigger>>,
    mut labels: Query<&mut Text>,
    mut fields: Query<(&SnapField, &mut NumberField)>,
) {
    let Some(snap) = snap.filter(|s| s.is_changed()) else {
        return;
    };
    let label = snap_label(&snap);
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
    for (which, mut field) in &mut fields {
        let want = match which {
            SnapField::Length => f64::from(snap.length),
            SnapField::Angle => f64::from(snap.angle.to_degrees()),
        };
        if (field.value - want).abs() > 1e-6 {
            field.value = want;
        }
    }
}

/// A tool button click selects the tool.
pub(crate) fn on_tool_change(
    ev: On<ValueChange<Entity>>,
    choices: Query<&ToolChoice>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(choice) = choices.get(ev.value) {
        out.write(UiCommand::SetTool(choice.0));
    }
}

/// The tool buttons show [`Tool`] (keys change it too).
pub(crate) fn tool_states(
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

/// The views draw a marquee with a plain left drag while Box select is on
/// (spec §6.1).
pub(crate) fn box_select_tool(tool: Res<Tool>, box_tool: Option<ResMut<BoxSelectTool>>) {
    if let (true, Some(mut b)) = (tool.is_changed(), box_tool) {
        b.set_if_neq(BoxSelectTool(*tool == Tool::BoxSelect));
    }
}

/// Why the editing tools are off on a server that doesn't take edits.
pub(crate) const NO_EDITS: &str =
    "This server can't take edits: it needs the websocket server fix (gz-sim pull request 4093)";

/// Translate and Rotate follow whether the server takes edits: off with
/// why while it doesn't (and back to Navigate if one of them was on).
pub(crate) fn edit_tools(
    status: Option<Res<GzStatus>>,
    map: Option<Res<InputMap>>,
    mut tool: ResMut<Tool>,
    buttons: Query<(Entity, &ToolChoice, Has<InteractionDisabled>)>,
    added: Query<(), Added<ToolChoice>>,
    mut commands: Commands,
) {
    let changed = status.as_ref().is_some_and(|s| s.is_changed());
    if !changed && added.is_empty() {
        return;
    }
    let allowed = edits_allowed(status.as_deref());
    // Only a server that can't take edits ends the tool: while a reconnect
    // asks again the tool waits (its button is off).
    let refused = status.as_ref().is_none_or(|s| s.edits == Some(false));
    if tool.edits() && refused {
        *tool = Tool::Navigate;
    }
    let map = map.map_or_else(InputMap::builtin, |m| m.clone());
    for (e, choice, disabled) in &buttons {
        let off = choice.0.edits() && !allowed;
        if off == disabled {
            continue;
        }
        let tip = Tooltip::new(choice.0.name()).binding(&map, choice.0.binding());
        if off {
            commands
                .entity(e)
                .insert((InteractionDisabled, tip.detail(NO_EDITS)));
        } else {
            commands
                .entity(e)
                .remove::<InteractionDisabled>()
                .insert(tip);
        }
    }
}

/// The views' gizmo follows the tool.
pub(crate) fn transform_tool(tool: Res<Tool>, gizmo: Option<ResMut<TransformTool>>) {
    if let (true, Some(mut g)) = (tool.is_changed(), gizmo) {
        g.set_if_neq(match *tool {
            Tool::Translate => TransformTool::Translate,
            Tool::Rotate => TransformTool::Rotate,
            _ => TransformTool::None,
        });
    }
}
