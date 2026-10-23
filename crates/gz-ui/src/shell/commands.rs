//! What the user asks the shell for (spec §3, §6.5): [`UiCommand`]
//! messages from buttons and keys, and the one system that applies them,
//! so buttons, keys and tests share one path. Sim requests are `gz-bevy`
//! messages (`GzCommand`, `GzReconnect`) instead.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gz_bevy::edit::edits_allowed;
use gz_bevy::undo::UndoCommand;
use gz_bevy::view::controls::{ActiveScopes, ViewRouting};
use gz_bevy::view::input::InputMap;

use super::layout::{ShellLayout, Side, Slot};
use super::sidebar::body::ActionButton;
use super::sidebar::HeaderButton;
use super::top_bar::tools::Tool;
use super::viewport::views::LayoutCommand;
use super::PanelStatus;
use crate::panels::{PanelActionActivated, PanelId, PanelRegistry};
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

/// The shortcut bindings pressed this frame, from the input map in the
/// scopes live now (none while typing; viewport keys only when the view
/// has them). An Esc that closes a menu does only that (§13: one step per
/// Esc).
#[derive(SystemParam)]
pub(crate) struct Shortcuts<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    map: Option<Res<'w, InputMap>>,
    scopes: Option<Res<'w, ActiveScopes>>,
    routing: Res<'w, ViewRouting>,
    menu_esc: Res<'w, MenuTakesEscape>,
    /// A marquee being drawn takes Esc too (spec §6.3: cancel the drag
    /// before returning to Navigate); the views cancel it.
    marquee: Option<Res<'w, gz_bevy::view::Marquee>>,
    /// So does a gizmo drag (its Esc puts the selection back, D-84).
    gizmo: Option<Res<'w, gz_bevy::view::gizmo::GizmoState>>,
    scrubs: Query<'w, 's, (), With<crate::widgets::number_field::Scrub>>,
}

impl Shortcuts<'_, '_> {
    /// A gizmo drag is under way: the tool stays as it is until it ends.
    pub(crate) fn dragging(&self) -> bool {
        self.gizmo.as_ref().is_some_and(|g| g.captures())
    }

    /// An edit is under way (a gizmo drag or a field's scrub): undo waits
    /// for it to end, so it doesn't undo the edit's start under it.
    pub(crate) fn editing(&self) -> bool {
        self.dragging() || !self.scrubs.is_empty()
    }

    /// The ids of the bindings pressed this frame.
    pub(crate) fn pressed(&self) -> Vec<String> {
        let (Some(map), Some(scopes)) = (self.map.as_deref(), self.scopes.as_deref()) else {
            return Vec::new();
        };
        if self.keys.get_just_pressed().next().is_none() {
            return Vec::new();
        }
        let scopes = self.routing.key_scopes(&scopes.0);
        let esc_taken = self.menu_esc.0
            || self.marquee.as_ref().is_some_and(|m| m.0.is_some())
            || self.gizmo.as_ref().is_some_and(|g| g.takes_escape());
        map.just_pressed(&self.keys, &scopes)
            .filter(|b| {
                !(esc_taken
                    && b.chords(map.platform)
                        .iter()
                        .any(|c| c.key == KeyCode::Escape))
            })
            .map(|b| b.id.clone())
            .collect()
    }
}

/// ⌘B, ⌘⌥I, the tool keys, ⌘Z and ⇧⌘Z (not mid-drag or mid-scrub), the
/// layout keys (⇧Space, ⌘W, ⌥⌘→, ⌥⌘↓) and the panels'
/// own shortcuts.
pub(crate) fn shortcut_keys(
    shortcuts: Shortcuts,
    panels: Res<PanelRegistry>,
    mut out: MessageWriter<UiCommand>,
    mut undo: MessageWriter<UndoCommand>,
    mut layout: MessageWriter<LayoutCommand>,
) {
    for id in shortcuts.pressed() {
        let views = match id.as_str() {
            "view.maximize" => Some(LayoutCommand::Maximize(None)),
            "view.close" => Some(LayoutCommand::Close(None)),
            "view.split_right" => Some(LayoutCommand::Split { down: false }),
            "view.split_down" => Some(LayoutCommand::Split { down: true }),
            _ => None,
        };
        if let Some(cmd) = views {
            layout.write(cmd);
            continue;
        }
        let cmd = match id.as_str() {
            "ui.sidebar" => UiCommand::ToggleSidebar,
            "ui.inspector" => UiCommand::ToggleInspector,
            "edit.undo" | "edit.redo" => {
                if !shortcuts.editing() {
                    undo.write(if id == "edit.undo" {
                        UndoCommand::Undo
                    } else {
                        UndoCommand::Redo
                    });
                }
                continue;
            }
            id => match Tool::ALL.into_iter().find(|t| t.binding() == id) {
                Some(_) if shortcuts.dragging() => continue,
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

/// A header action's binding ([`HeaderAction::binding`]) activates it, as
/// its button would: [`PanelActionActivated`] on the button, while its
/// panel is shown and the action enabled.
///
/// [`HeaderAction::binding`]: crate::panels::HeaderAction
pub(crate) fn header_action_keys(
    shortcuts: Shortcuts,
    layout: Res<ShellLayout>,
    panels: Res<PanelRegistry>,
    status: Res<PanelStatus>,
    buttons: Query<(Entity, &ActionButton), With<HeaderButton>>,
    mut commands: Commands,
) {
    let pressed = shortcuts.pressed();
    if pressed.is_empty() || !layout.sidebar_open {
        return;
    }
    let shown = panels.iter().filter(|d| layout.is_active(d.id));
    for def in shown {
        for a in &def.header_actions {
            let bound = a.binding.is_some_and(|b| pressed.iter().any(|p| p == b));
            if !bound || !status.header_action_enabled(def.id, a) {
                continue;
            }
            let button = buttons
                .iter()
                .find(|(_, b)| b.panel == def.id && b.action == a.id);
            if let Some((entity, _)) = button {
                commands.trigger(PanelActionActivated {
                    entity,
                    panel: def.id,
                    action: a.id,
                });
            }
        }
    }
}

/// Applies [`UiCommand`]s. A panel id nothing registered stands for the
/// first panel in the activity bar when opening a panel
/// ([`PanelRegistry::resolve`]) and is ignored for an activity click; with
/// no panels at all, the command does nothing.
pub(crate) fn apply_ui_commands(
    mut commands: MessageReader<UiCommand>,
    panels: Res<PanelRegistry>,
    mut layout: ResMut<ShellLayout>,
    mut tool: ResMut<Tool>,
    mut theme: ResMut<Theme>,
    status: Option<Res<gz_bevy::connection::GzStatus>>,
) {
    for cmd in commands.read() {
        match *cmd {
            UiCommand::ToggleSidebar => layout.toggle(Side::Sidebar),
            UiCommand::ToggleInspector => layout.toggle(Side::Inspector),
            // An activity click names a button's own panel: an unknown id
            // is ignored (standing for the first panel, it would collapse
            // the sidebar when that one is active).
            UiCommand::Activity { panel, shift } => {
                if panels.get(panel).is_some() {
                    layout.activity_click(panel, shift);
                } else {
                    warn!("no panel '{panel}' is registered; ignoring its activity click");
                }
            }
            UiCommand::OpenPanel { panel, slot } => {
                if let Some(panel) = panels.resolve(panel) {
                    layout.open_in(panel, slot);
                }
            }
            UiCommand::SwapSlots => layout.swap_slots(),
            UiCommand::CloseSecondary => layout.close_secondary(),
            // The editing tools need a server that takes edits.
            UiCommand::SetTool(t) if t.edits() && !edits_allowed(status.as_deref()) => {}
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
