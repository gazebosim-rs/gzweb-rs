//! What the user asks the shell for (spec §3, §6.5): [`UiCommand`]
//! messages from buttons and keys, and the one system that applies them,
//! so buttons, keys and tests share one path. Sim requests are `gz-bevy`
//! messages (`GzCommand`, `GzReconnect`) instead.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gz_bevy::connection::GzCommand;
use gz_bevy::edit::edits_allowed;
use gz_bevy::undo::UndoCommand;
use gz_bevy::view::controls::{ActiveScopes, RunBinding, ViewRouting};
use gz_bevy::view::editing::{EditCommand, EditingModel};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::model_edits::{EditModels, ModelEdit};
use gz_bevy::view::Selection;

use super::layout::{ShellLayout, Side, Slot};
use super::sidebar::body::ActionButton;
use super::sidebar::HeaderButton;
use super::top_bar::tools::Tool;
use super::viewport::views::LayoutCommand;
use super::{PanelStatus, ShownPanels};
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
    /// The theme commands: the command palette's "Theme: light / dark /
    /// follow system" (D-107), and Settings.
    SetTheme(ThemeMode),
    ToggleTheme,
    /// A splitter double-click.
    ResetWidth(Side),
    /// A collapsed panel's bar button: expand it at its previous width.
    Show(Side),
    /// A slot header's Pop out (§7.11).
    PopOut(PanelId),
    /// A pop-out window's Dock: back to its slot.
    Dock(PanelId),
    /// A pop-out window's Close.
    ClosePopOut(PanelId),
    /// A press in a pop-out window: it comes to the top.
    RaisePopOut(PanelId),
    /// The World menu's commands (§7.1), from it, ⌘S, ⇧⌘S and the
    /// palette.
    World(super::top_bar::world_menu::WorldCommand),
    /// ⌘K or the search trigger: the command palette (§7.7).
    OpenPalette,
    /// Esc, a run, or a click outside: the palette closes.
    ClosePalette,
    /// A pop-out header drag: the window's top-left corner, in the
    /// viewport area.
    MovePopOut {
        panel: PanelId,
        at: Vec2,
    },
    /// A pop-out edge or corner drag: `edges` moved `delta` from `start`.
    ResizePopOut {
        panel: PanelId,
        start: Rect,
        edges: super::layout::Edges,
        delta: Vec2,
    },
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
    /// And placing: Esc stops it (D-102).
    placing: Option<Res<'w, gz_bevy::view::place::Placing>>,
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
            || self.gizmo.as_ref().is_some_and(|g| g.takes_escape())
            || self.placing.as_ref().is_some_and(|p| p.0);
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

/// What Esc reads and writes while a model is edited: the tool, the
/// editing, the selection and the leaving.
type EditKeys<'w> = (
    Res<'w, Tool>,
    Option<Res<'w, EditingModel>>,
    ResMut<'w, Selection>,
    MessageWriter<'w, EditCommand>,
);

/// ⌘B, ⌘⌥I, the tool keys, ⌘Z and ⇧⌘Z, ⌘C ⌘V ⌘D and Delete (D-109; none
/// mid-drag or mid-scrub), Alt+1–4 (the workspaces, D-110), the
/// layout keys (⇧Space, ⌘W, ⌥⌘→, ⌥⌘↓), ⇧⌘R (Reset, on a live world), ⌘K,
/// the themes and the panels' own shortcuts; and [`RunBinding`]s of the
/// same (the command palette, D-107).
#[allow(clippy::too_many_arguments)]
pub(crate) fn shortcut_keys(
    shortcuts: Shortcuts,
    panels: Res<PanelRegistry>,
    phase: Option<Res<super::CurrentPhase>>,
    mut runs: MessageReader<RunBinding>,
    mut out: MessageWriter<UiCommand>,
    mut undo: MessageWriter<UndoCommand>,
    mut edits: MessageWriter<EditModels>,
    mut workspaces: MessageWriter<super::workspaces::WorkspaceCommand>,
    mut layout: MessageWriter<LayoutCommand>,
    mut sim: MessageWriter<GzCommand>,
    (tool, editing, mut selection, mut edit_out): EditKeys,
) {
    let ran: Vec<String> = runs.read().map(|r| r.0.clone()).collect();
    for id in shortcuts.pressed().into_iter().chain(ran) {
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
            "ui.command_palette" => UiCommand::OpenPalette,
            "file.save" => UiCommand::World(super::top_bar::world_menu::WorldCommand::Save),
            "file.save_as" => UiCommand::World(super::top_bar::world_menu::WorldCommand::SaveAs),
            "ui.theme_light" => UiCommand::SetTheme(ThemeMode::Light),
            "ui.theme_dark" => UiCommand::SetTheme(ThemeMode::Dark),
            "ui.theme_system" => UiCommand::SetTheme(ThemeMode::System),
            "sim.reset" => {
                if phase.as_deref().is_some_and(|p| p.0 == super::Phase::Live) {
                    sim.write(GzCommand::Reset);
                }
                continue;
            }
            "edit.copy" | "edit.paste" | "edit.duplicate" | "edit.delete" => {
                if !shortcuts.editing() {
                    edits.write(EditModels(match id.as_str() {
                        "edit.copy" => ModelEdit::Copy,
                        "edit.paste" => ModelEdit::Paste,
                        "edit.duplicate" => ModelEdit::Duplicate,
                        _ => ModelEdit::Delete,
                    }));
                }
                continue;
            }
            "ui.workspace_1" | "ui.workspace_2" | "ui.workspace_3" | "ui.workspace_4" => {
                // Not under an edit: the switch would move its camera
                // or close its field.
                if !shortcuts.editing() {
                    let n = usize::from(id.as_bytes()[id.len() - 1] - b'1');
                    workspaces.write(super::workspaces::WorkspaceCommand::Switch(n));
                }
                continue;
            }
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
            // Editing a model, Esc in Navigate clears the selection, then
            // leaves (§6.6; one step per Esc, §6.3).
            "tool.navigate"
                if *tool == Tool::Navigate
                    && editing.as_deref().is_some_and(|e| e.model().is_some()) =>
            {
                if selection.is_empty() {
                    edit_out.write(EditCommand::Done);
                } else {
                    *selection = Selection::default();
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
    hosts: Res<ShownPanels>,
    panels: Res<PanelRegistry>,
    status: Res<PanelStatus>,
    buttons: Query<(Entity, &ActionButton), With<HeaderButton>>,
    mut commands: Commands,
) {
    let pressed = shortcuts.pressed();
    if pressed.is_empty() {
        return;
    }
    let shown = panels.iter().filter(|d| hosts.is_shown(d.id));
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
    mut palette: ResMut<super::palette::PaletteOpen>,
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
            UiCommand::PopOut(panel) => {
                if panels.get(panel).is_some() {
                    layout.pop_out(panel);
                }
            }
            UiCommand::Dock(panel) => layout.dock(panel),
            UiCommand::ClosePopOut(panel) => layout.close_popout(panel),
            UiCommand::RaisePopOut(panel) => {
                // A press in the top window changes nothing.
                if layout.popouts.last().is_some_and(|p| p.panel != panel) {
                    layout.raise(panel);
                }
            }
            UiCommand::MovePopOut { panel, at } => layout.move_popout(panel, at),
            // `run_world_commands` runs them.
            UiCommand::World(_) => {}
            UiCommand::OpenPalette => {
                palette.set_if_neq(super::palette::PaletteOpen(true));
            }
            UiCommand::ClosePalette => {
                palette.set_if_neq(super::palette::PaletteOpen(false));
            }
            UiCommand::ResizePopOut {
                panel,
                start,
                edges,
                delta,
            } => layout.resize_popout(panel, start, edges, delta),
        }
    }
}
