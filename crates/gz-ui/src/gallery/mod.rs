//! The widget gallery (`gzweb --ui gallery`, `?ui=gallery`): every widget,
//! and every state of the clickable ones, in one theme at a time, laid out
//! after board 06 (`design/ui/assets/boards/06-theme-tokens-light-vs-dark.png`).
//! The left column is the board's component strip and a states grid; the
//! right one the board's token list, then the remaining widgets. T (or the
//! button at the top right) switches light ↔ dark at runtime. No server
//! connection is involved.
//!
//! With [`GalleryCapture`] it saves a screenshot once the page has settled
//! and exits (`--ui gallery --screenshot out.png`).

mod components;
mod states;
mod systems;
mod tokens;
mod widgets;

use std::path::PathBuf;

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::text::Underline;
use gz_bevy::view::input::InputMap;

use crate::assets::fonts::text;
use crate::theme::{border, radius, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::button::TextButtonProps;
use crate::widgets::layout::{column, flex_spacer, row};
use crate::widgets::splitter::SplitAxis;
use crate::widgets::{
    rounded, splitter, status_chip, text_button, tooltip_card, Splitter, StatusKind, Tooltip,
};
use crate::GzUiSystems;

// The shell's tests check colours by their hex value too.
#[cfg(test)]
pub(crate) use tokens::hex;

/// The gallery's window size (the spec's screenshot size, §12).
pub const SIZE: (u32, u32) = (1440, 900);

/// Adds the gallery screen (and a 2D camera for it).
pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_gallery)
            .add_systems(
                Update,
                (
                    systems::toggle_theme_key,
                    systems::gallery_texts,
                    systems::focus_example,
                )
                    .in_set(GzUiSystems::Behaviour),
            )
            .add_systems(Last, systems::capture_gallery)
            .add_observer(systems::on_theme_button);
    }
}

/// Save a screenshot here once the gallery has settled, then exit.
#[derive(Resource, Clone, Debug)]
pub struct GalleryCapture {
    pub path: PathBuf,
    /// Frames to wait (fonts, atlases, layout and the glyph cache).
    pub frames: u32,
}

impl GalleryCapture {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            frames: 30,
        }
    }
}

/// Text that names the current theme ("Light" / "Dark").
#[derive(Component, Clone, Copy, Debug)]
pub(crate) enum ThemeText {
    Name,
    Caption,
}

/// The button that switches theme.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ThemeToggle;

const COLUMN_W: f32 = 674.0;
const TITLE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 22.0, 600);

fn card_node(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..node
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
    )
}

/// A column of the page, `width` wide.
fn page_column(width: f32, gap: f32) -> Node {
    Node {
        width: px(width),
        flex_shrink: 0.0,
        ..column(gap)
    }
}

/// A row of samples that keeps its size.
fn sample_row(gap: f32) -> Node {
    Node {
        flex_shrink: 0.0,
        ..row(gap)
    }
}

/// Spawns the gallery (and a UI camera).
pub(crate) fn spawn_gallery(mut commands: Commands, theme: Res<Theme>, map: Option<Res<InputMap>>) {
    let map = map.map_or_else(InputMap::builtin, |m| m.clone());
    commands.spawn((Camera2d, IsDefaultUiCamera));
    commands
        .spawn((
            GalleryRoot,
            // Tab walks the gallery's widgets.
            TabGroup::new(0),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::axes(px(SPACE[10]), px(SPACE[9])),
                column_gap: px(SPACE[10]),
                overflow: Overflow::clip(),
                ..default()
            },
            Fill(ColorRole::Bg),
        ))
        .with_children(|p| {
            p.spawn(page_column(COLUMN_W, SPACE[8]))
                .with_children(|c| left_column(c, &theme, &map));
            p.spawn(page_column(COLUMN_W + 20.0, SPACE[8]))
                .with_children(|c| right_column(c, &theme, &map));
        });
}

/// The gallery's root node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct GalleryRoot;

/// A column's header row: 32 tall, its title left and its notes right.
fn header_row() -> Node {
    Node {
        height: px(32),
        flex_shrink: 0.0,
        ..row(SPACE[6])
    }
}

fn header(c: &mut ChildSpawnerCommands) {
    c.spawn(header_row()).with_children(|h| {
        h.spawn((ThemeText::Name, text(TITLE_TYPE, ColorRole::Text, "")));
        h.spawn(flex_spacer());
        h.spawn((
            ThemeText::Caption,
            text(TypeStyle::CAPS, ColorRole::Text2, ""),
        ));
    });
}

fn left_column(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    header(c);
    components::top_bar(c, theme, map);
    c.spawn(Node {
        align_items: AlignItems::FlexStart,
        ..sample_row(SPACE[8])
    })
    .with_children(|r| {
        components::activity_card(r, theme, map);
        components::rail_card(r, theme, map);
        components::static_menu(r, theme, map);
        components::inspector_card(r, theme);
    });
    c.spawn(sample_row(SPACE[8])).with_children(|r| {
        r.spawn(status_chip(
            theme,
            StatusKind::Warning,
            "Mesh for crate_01 not found",
            None,
        ));
        r.spawn(tooltip_card(
            theme,
            &Tooltip::new("Workspace: Build")
                .detail("Click to switch")
                .shortcut("Alt+1–4"),
            false,
        ));
        r.spawn((
            text(TypeStyle::ROW, ColorRole::AccentText, "All components 27 ›"),
            Underline,
        ));
    });
    states::states_grid(c, theme);
}

fn right_column(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    c.spawn(header_row()).with_children(|h| {
        h.spawn(text(TypeStyle::CAPS, ColorRole::Text2, "Color tokens"));
        h.spawn(flex_spacer());
        h.spawn(text(TypeStyle::SMALL, ColorRole::Text3, "T switches theme"));
        h.spawn((
            text_button(theme, TextButtonProps::new("Dark theme")),
            ThemeToggle,
        ));
    });
    tokens::token_list(c);
    c.spawn(Node {
        flex_grow: 1.0,
        min_height: px(0),
        ..default()
    })
    .with_children(|bottom| {
        let tree = bottom
            .spawn(card_node(Node {
                width: px(320),
                overflow: Overflow::clip(),
                ..column(0.0)
            }))
            .with_children(|t| widgets::scene_panel(t, theme))
            .id();
        bottom.spawn(splitter(
            theme,
            Splitter {
                axis: SplitAxis::Columns,
                pane: tree,
                sign: 1.0,
                min: 240.0,
                max: 420.0,
            },
            "Resize the scene panel",
        ));
        bottom
            .spawn(Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                padding: UiRect::left(px(SPACE[3])),
                ..column(SPACE[4])
            })
            .with_children(|w| widgets::more_widgets(w, theme, map));
    });
}

#[cfg(test)]
mod tests;
