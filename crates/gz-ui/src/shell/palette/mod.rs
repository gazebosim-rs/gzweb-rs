//! The command palette (spec §7.7; DECISIONS.md D-107): ⌘K or the top
//! bar's search opens it over everything; typing ranks every command,
//! the world's entities, the server's topics, the panels and the
//! settings by a fuzzy match (Gazebo GUI plugin names find what stands
//! for them); ↑ and ↓ choose, Enter or a click runs, Esc or a click
//! outside closes.
//!
//! A command from `shortcuts.json` runs as its key would: a
//! [`RunBinding`](gz_bevy::view::controls::RunBinding) that every reader
//! of key bindings reads, so a command does exactly what its shortcut
//! does. A shown panel's header and footer actions are commands too.

pub(crate) mod model;
mod systems;
mod view;

// The spec checks read the icons commands get (§13).
#[cfg(test)]
pub(crate) use systems::command_icon;

use bevy::prelude::*;

use super::ShellSystems;
use model::Item;

/// Opens the palette at start with this query typed, for a capture
/// (`gzweb --palette QUERY`); removed once typed.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StartPalette(pub String);

/// The palette is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PaletteOpen(pub bool);

/// The open palette's query, its results and the chosen one.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PaletteState {
    pub query: String,
    pub results: Vec<Item>,
    pub selected: Option<usize>,
    /// The results need ranking again.
    dirty: bool,
    /// The rows show these results.
    built: bool,
    /// Where focus was before the palette took it.
    return_focus: Option<Entity>,
    /// ↑ or ↓ chose the result: a refresh keeps it (else the first
    /// available is chosen again).
    moved: bool,
    /// ↑ or ↓ moved the field's caret too: it goes back to the end.
    caret_end: bool,
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<PaletteOpen>()
        .init_resource::<PaletteState>()
        .add_systems(
            Update,
            systems::sync_palette
                .in_set(ShellSystems::Layout)
                .after(super::commands::apply_ui_commands),
        )
        .add_systems(
            Update,
            (
                systems::apply_start,
                systems::focus_field,
                systems::keep_focus,
                systems::caret_to_end,
                systems::query_text,
                systems::refresh_results,
                systems::sync_rows,
                systems::reveal_selected,
            )
                .chain()
                .in_set(ShellSystems::Readouts),
        )
        .add_observer(systems::on_field_key)
        .add_observer(systems::on_submit)
        .add_observer(systems::on_row)
        .add_observer(systems::on_backdrop)
        .add_observer(systems::on_trigger);
}

#[cfg(test)]
mod tests;
