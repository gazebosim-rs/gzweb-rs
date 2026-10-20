//! SectionHeader (§5.6, §3.3): 36 tall; title in `section` type (12/600
//! uppercase, `text_2`), an optional count in mono 11 `text_3`, a flexible
//! spacer, then up to three 28 px icon buttons (the caller's children),
//! and a `line_soft` bottom border.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;

use super::{a11y, spacer};
use crate::assets::fonts::text;
use crate::theme::{border, ColorRole, FontFamily, Stroke, Theme, TypeStyle, SPACE};

/// The count's type: mono 11/400.
pub const COUNT_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);

/// A panel section header. Spawn its action buttons as further children.
pub fn section_header(
    theme: &Theme,
    title: impl Into<String>,
    count: Option<String>,
) -> impl Bundle {
    let title = title.into();
    let label = title.clone();
    (
        Node {
            height: px(theme.s.section_header_h),
            flex_shrink: 0.0,
            padding: UiRect {
                left: px(SPACE[7]),
                right: px(SPACE[3]),
                ..default()
            },
            column_gap: px(SPACE[4]),
            align_items: AlignItems::Center,
            border: UiRect::bottom(px(border::HAIRLINE)),
            ..default()
        },
        Stroke(ColorRole::LineSoft),
        a11y(Role::Heading, label),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(text(TypeStyle::SECTION, ColorRole::Text2, title));
            if let Some(count) = count {
                p.spawn(text(COUNT_TYPE, ColorRole::Text3, count));
            }
            p.spawn(spacer());
        })),
    )
}
