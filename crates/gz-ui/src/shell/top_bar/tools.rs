//! The top bar's tools (spec §3.1, §6.1): Navigate, Box select, Translate
//! and Rotate as a radio group of bar buttons, and the Snap text button.
//! Only the selection shows until M5.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Button as Headless, ValueChange};
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::edits_allowed;
use gz_bevy::view::gizmo::TransformTool;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::BoxSelectTool;

use crate::assets::icons::IconId;
use crate::shell::commands::UiCommand;
use crate::theme::Theme;
use crate::widgets::button::{text_button_parts, TextButtonProps};
use crate::widgets::{bar_tool_button, button_group, Tooltip};

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

    fn icon(self) -> IconId {
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
            g.spawn((
                text_button_parts(t, TextButtonProps::new("Snap 0.25").mono().chevron()),
                Headless,
                Tooltip::new("Snap"),
            ));
        })
        .observe(on_tool_change);
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
    if tool.edits() && !allowed {
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
