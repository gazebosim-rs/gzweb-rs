//! The world menu button (spec §3.1, §7.1): the world's name in `text`
//! 500, `.sdf` in `text_3` 400, the unsaved dot while the world is dirty
//! (an edit to undo, §7.1) and a chevron. It is the World menu's trigger
//! ([`world_menu`](super::world_menu)).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Button as Headless;
use gz_bevy::connection::GzStatus;
use gz_bevy::spawn::GzIndex;
use gz_bevy::undo::UndoStack;

use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::shell::connection::world_name;
use crate::theme::visuals::Visuals;
use crate::theme::{ColorRole, FontFamily, Theme, TypeStyle, SPACE};
use crate::widgets::button::text_button_node;
use crate::widgets::layout::{row, set_shown};
use crate::widgets::text::set_text;
use crate::widgets::{clickable, Tooltip};

/// The world menu button (the World menu's trigger).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WorldButton;

/// The world menu button: name in `text` 500, `.sdf` in `text_3` 400, the
/// unsaved dot when dirty, chevron.
pub(super) fn world_button(theme: &Theme) -> impl Bundle {
    let name = TypeStyle::new(FontFamily::Ui, 15.0, 500);
    let ext = TypeStyle::new(FontFamily::Ui, 15.0, 400);
    let icon_px = theme.s.icon_sm;
    let dot_px = theme.s.status_dot;
    (
        WorldButton,
        text_button_node(theme),
        Headless,
        clickable(
            Visuals::new(ColorRole::Clear, ColorRole::Text),
            Role::Button,
            "World menu",
        ),
        Tooltip::new("World").detail("World menu"),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(Node {
                align_items: AlignItems::Baseline,
                ..row(SPACE[2] + 1.0)
            })
            .with_children(|n| {
                n.spawn((text(name, ColorRole::Text, "No world"), WorldText::Name));
                n.spawn((text(ext, ColorRole::Text3, ""), WorldText::Ext));
            });
            // Shown by `unsaved_dot`.
            p.spawn((
                crate::widgets::chip::dot(dot_px, ColorRole::Warn),
                UnsavedDot,
            ));
            p.spawn(content_icon(IconId::ChevronDown, icon_px, ColorRole::Text));
        })),
    )
}

/// The world button's two texts.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorldText {
    /// The world's name, "No world" without one.
    Name,
    /// ".sdf" after the name.
    Ext,
}

/// The world's name and its extension follow the server (when it or the
/// scene changes).
pub(crate) fn world_texts(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut texts: Query<(&WorldText, &mut Text)>,
) {
    let changed =
        status.as_ref().is_some_and(Res::is_changed) || index.as_ref().is_some_and(Res::is_changed);
    if !changed {
        return;
    }
    let world = world_name(status.as_deref(), index.as_deref());
    for (which, mut t) in &mut texts {
        let want = match which {
            WorldText::Name => world.clone().unwrap_or_else(|| "No world".into()),
            WorldText::Ext => world.as_ref().map_or_else(String::new, |_| ".sdf".into()),
        };
        set_text(&mut t, &want);
    }
}

/// The world button's unsaved dot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct UnsavedDot;

/// The dot shows while the world is dirty.
pub(crate) fn unsaved_dot(
    stack: Option<Res<UndoStack>>,
    added: Query<(), Added<UnsavedDot>>,
    mut dots: Query<&mut Node, With<UnsavedDot>>,
) {
    let changed = stack.as_ref().is_some_and(Res::is_changed);
    if !changed && added.is_empty() {
        return;
    }
    let dirty = stack.is_some_and(|s| s.is_dirty());
    for mut node in &mut dots {
        set_shown(&mut node, dirty);
    }
}
