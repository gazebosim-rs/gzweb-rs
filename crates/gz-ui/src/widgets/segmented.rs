//! SegmentedGroup (§5.4): a `bg` container, radius 8, padding 2, gap 2,
//! holding icon buttons. The tool group has radio semantics: its children
//! are [`tool_button`](super::tool_button)s and exactly one is `Checked`.

use accesskit::Role;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, radio_self_update, RadioGroup};

use super::{a11y, rounded};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, SPACE};

/// A segmented group container. `radio` makes it a radio group whose
/// buttons select themselves; `bordered` adds the transport group's 1px
/// `line` border (§3.1). Spawn the buttons as children.
pub fn segmented_group(label: impl Into<String>, radio: bool, bordered: bool) -> impl Bundle {
    let node = Node {
        padding: UiRect::all(px(SPACE[1])),
        column_gap: px(SPACE[1]),
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        border: if bordered {
            UiRect::all(px(border::HAIRLINE))
        } else {
            UiRect::ZERO
        },
        border_radius: rounded(if bordered { 9.0 } else { radius::MD }),
        ..default()
    };
    let stroke = Stroke(if bordered {
        ColorRole::Line
    } else {
        ColorRole::Clear
    });
    let role = if radio { Role::RadioGroup } else { Role::Group };
    (
        node,
        Fill(ColorRole::Bg),
        stroke,
        a11y(role, label),
        SegmentedGroup { radio },
    )
}

/// Marks a segmented group; radio groups get `RadioGroup` and self-update
/// on spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(on_add = add_radio)]
pub struct SegmentedGroup {
    pub radio: bool,
}

fn add_radio(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    if world
        .get::<SegmentedGroup>(ctx.entity)
        .is_some_and(|g| g.radio)
    {
        world
            .commands()
            .entity(ctx.entity)
            .insert((RadioGroup, observe(radio_self_update)));
    }
}
