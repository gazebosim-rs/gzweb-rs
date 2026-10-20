//! The top bar's tools (spec §3.1, §6.1): Navigate, Box select, Translate
//! and Rotate as a radio group of bar buttons, and the Snap text button.
//! Only the selection shows until M5.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{Button as Headless, ValueChange};
use gz_bevy::view::input::InputMap;

use crate::assets::icons::IconId;
use crate::shell::commands::UiCommand;
use crate::theme::Theme;
use crate::widgets::button::{text_button_parts, TextButtonProps};
use crate::widgets::{bar_tool_button, button_group, Tooltip};

/// The top-bar tools (spec §6.1). Only the selection shows until M5.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum Tool {
    #[default]
    Navigate,
    BoxSelect,
    Translate,
    Rotate,
}

impl Tool {
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
pub struct ToolChoice(pub Tool);

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
pub fn on_tool_change(
    ev: On<ValueChange<Entity>>,
    choices: Query<&ToolChoice>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(choice) = choices.get(ev.value) {
        out.write(UiCommand::SetTool(choice.0));
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
