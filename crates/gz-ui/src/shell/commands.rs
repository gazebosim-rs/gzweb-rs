//! What the user asks the shell for (spec §3, §6.5): [`UiCommand`]
//! messages from buttons and keys, and the one system that applies them,
//! so buttons, keys and tests share one path. Sim requests are `gz-bevy`
//! messages (`GzCommand`, `GzReconnect`) instead.

use bevy::prelude::*;
use gz_bevy::view::controls::{ActiveScopes, ViewRouting};
use gz_bevy::view::input::InputMap;

use super::layout::{ShellLayout, Side, Slot};
use super::top_bar::tools::Tool;
use crate::panels::{PanelId, PanelRegistry};
use crate::theme::{Theme, ThemeMode};
use crate::widgets::focus::MenuTakesEscape;

/// What the shell's controls and keys ask for.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// ⌘B, the sidebar header's chevron.
    ToggleSidebar,
    /// ⌘⌥I, the inspector header's chevron.
    ToggleInspector,
    /// An activity button: plain click or Shift+click (§3.2).
    Activity {
        panel: PanelId,
        shift: bool,
    },
    OpenPanel {
        panel: PanelId,
        slot: Slot,
    },
    SwapSlots,
    CloseSecondary,
    SetTool(Tool),
    /// The theme command: Settings for now (⌘K, which will list "Theme:
    /// light / dark / follow system", is M8; DECISIONS.md).
    SetTheme(ThemeMode),
    ToggleTheme,
    /// A splitter double-click.
    ResetWidth(Side),
    /// A collapsed panel's bar button: expand it at its previous width.
    Show(Side),
}

/// ⌘B, ⌘⌥I and the tool keys, from the input map, in the scopes live now
/// (none while typing; viewport keys only when the view has them). An Esc
/// that closes a menu does only that (§13: one step per Esc).
pub(crate) fn shortcut_keys(
    keys: Res<ButtonInput<KeyCode>>,
    map: Option<Res<InputMap>>,
    scopes: Option<Res<ActiveScopes>>,
    routing: Res<ViewRouting>,
    panels: Res<PanelRegistry>,
    menu_esc: Res<MenuTakesEscape>,
    mut out: MessageWriter<UiCommand>,
) {
    let (Some(map), Some(scopes)) = (map, scopes) else {
        return;
    };
    if keys.get_just_pressed().next().is_none() {
        return;
    }
    let scopes = routing.key_scopes(&scopes.0);
    for b in map.just_pressed(&keys, &scopes) {
        if menu_esc.0
            && b.chords(map.platform)
                .iter()
                .any(|c| c.key == KeyCode::Escape)
        {
            continue;
        }
        let cmd = match b.id.as_str() {
            "ui.sidebar" => UiCommand::ToggleSidebar,
            "ui.inspector" => UiCommand::ToggleInspector,
            id => match Tool::ALL.into_iter().find(|t| t.binding() == id) {
                Some(tool) => UiCommand::SetTool(tool),
                None => match panels.for_binding(id) {
                    Some(panel) => UiCommand::OpenPanel {
                        panel,
                        slot: Slot::Primary,
                    },
                    None => continue,
                },
            },
        };
        out.write(cmd);
    }
}

/// Applies [`UiCommand`]s. A panel id nothing registered stands for the
/// first panel in the activity bar ([`PanelRegistry::resolve`]); with no
/// panels at all, the command does nothing.
pub(crate) fn apply_ui_commands(
    mut commands: MessageReader<UiCommand>,
    panels: Res<PanelRegistry>,
    mut layout: ResMut<ShellLayout>,
    mut tool: ResMut<Tool>,
    mut theme: ResMut<Theme>,
) {
    for cmd in commands.read() {
        match *cmd {
            UiCommand::ToggleSidebar => layout.toggle(Side::Sidebar),
            UiCommand::ToggleInspector => layout.toggle(Side::Inspector),
            UiCommand::Activity { panel, shift } => {
                if let Some(panel) = panels.resolve(panel) {
                    layout.activity_click(panel, shift);
                }
            }
            UiCommand::OpenPanel { panel, slot } => {
                if let Some(panel) = panels.resolve(panel) {
                    layout.open_in(panel, slot);
                }
            }
            UiCommand::SwapSlots => layout.swap_slots(),
            UiCommand::CloseSecondary => layout.close_secondary(),
            UiCommand::SetTool(t) => {
                tool.set_if_neq(t);
            }
            UiCommand::SetTheme(mode) => theme.set_mode(mode),
            UiCommand::ToggleTheme => theme.toggle(),
            UiCommand::ResetWidth(side) => layout.reset_width(side),
            UiCommand::Show(side) => layout.show(side, true),
        }
    }
}
