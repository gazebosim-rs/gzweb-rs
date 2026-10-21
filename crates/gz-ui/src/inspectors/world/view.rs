//! The World inspector's body.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use bevy::ecs::spawn::SpawnWith;

use crate::assets::text;
use crate::assets::IconId;
use crate::inspectors::common::{CRUMB_TYPE, TAG_TYPE};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{self, column, flex_spacer, group_header, property_row, row};
use crate::widgets::{button, icon_button, rounded, ButtonVariant, IconButtonSize, Tooltip};

/// The world quick actions (§3.5): Save (M8), Reset, Statistics (M4).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorldAction {
    Save,
    Reset,
    Statistics,
}

/// Text the World inspector keeps up to date.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorldField {
    Name,
    Source,
    Step,
}

/// The World inspector's content marker.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct WorldBody;

/// Builds the body into a fresh content entity.
pub(super) fn build_world(ev: On<Add<WorldBody>>, theme: Res<Theme>, mut commands: Commands) {
    let t = theme.clone();
    commands
        .entity(ev.entity)
        .insert(Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            world_inspector(b, &t);
        })));
}

/// The body: title row, source, quick actions, groups and links.
fn world_inspector(body: &mut ChildSpawner, t: &Theme) {
    // Title row: padding 14 16 10.
    body.spawn(Node {
        padding: UiRect {
            left: px(SPACE[8]),
            right: px(SPACE[8]),
            top: px(SPACE[7]),
            bottom: px(SPACE[2]),
        },
        flex_shrink: 0.0,
        ..row(SPACE[4])
    })
    .with_children(|r| {
        r.spawn((
            Node {
                padding: UiRect::axes(px(SPACE[3]), px(SPACE[1])),
                flex_shrink: 0.0,
                border_radius: rounded(radius::XS),
                ..default()
            },
            Fill(ColorRole::EntityBadgeBg),
            children![text(TAG_TYPE, ColorRole::Text2, "WORLD")],
        ));
        r.spawn((
            text(TypeStyle::ENTITY_NAME, ColorRole::Text, "No world"),
            WorldField::Name,
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 1.0,
                min_width: px(0),
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        r.spawn(flex_spacer());
        r.spawn(icon_button(
            t,
            IconId::More,
            IconButtonSize::Small,
            Tooltip::new("More actions"),
        ));
    });
    body.spawn((
        text(CRUMB_TYPE, ColorRole::Text3, ""),
        WorldField::Source,
        Node {
            margin: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                bottom: px(SPACE[6]),
                ..default()
            },
            ..default()
        },
    ));
    // Quick actions: three equal buttons, never destructive (Q3).
    body.spawn((
        Node {
            padding: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                bottom: px(SPACE[7]),
                ..default()
            },
            flex_shrink: 0.0,
            border: UiRect::bottom(px(border::HAIRLINE)),
            ..row(SPACE[3])
        },
        Stroke(ColorRole::Line),
    ))
    .with_children(|r| {
        for (label, which, live) in [
            ("Save", WorldAction::Save, false),
            ("Reset", WorldAction::Reset, true),
            ("Statistics", WorldAction::Statistics, false),
        ] {
            let mut e = r.spawn((button(t, label, ButtonVariant::Normal), which));
            e.insert(Node {
                height: px(t.s.button_h),
                flex_grow: 1.0,
                flex_basis: px(0),
                border: UiRect::all(px(border::HAIRLINE)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: rounded(radius::SM),
                ..default()
            });
            if !live {
                e.insert((
                    InteractionDisabled,
                    Tooltip::new(label).detail("Not available yet"),
                ));
            }
        }
    });
    group(
        body,
        "Physics",
        &[
            ("Engine", None),
            ("Step size", Some(WorldField::Step)),
            ("RTF target", None),
            ("Gravity", None),
        ],
    );
    group(
        body,
        "Scene",
        &[
            ("Ambient", None),
            ("Background", None),
            ("Shadows", None),
            ("Grid", None),
        ],
    );
    group(body, "Plugins", &[("—", None)]);
    body.spawn(Node {
        padding: UiRect::axes(px(SPACE[8]), px(SPACE[4])),
        flex_shrink: 0.0,
        ..column(0.0)
    })
    .with_children(|links| {
        for name in [
            "Spherical coordinates",
            "Wind & atmosphere",
            "All components",
        ] {
            links
                .spawn(Node {
                    height: px(36),
                    flex_shrink: 0.0,
                    ..row(0.0)
                })
                .with_children(|row| {
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text, name));
                    row.spawn(flex_spacer());
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text3, "›"));
                });
        }
    });
}

/// A group (§3.5 item 5): heading 13/600, rows of label and a "—" value.
fn group(body: &mut ChildSpawner, title: &str, rows: &[(&str, Option<WorldField>)]) {
    body.spawn(layout::group()).with_children(|g| {
        g.spawn(group_header(title, None));
        for &(label, which) in rows {
            let value = text(TypeStyle::MONO_SMALL, ColorRole::Text3, "—");
            match which {
                Some(w) => g.spawn(property_row(label, (value, w))),
                None => g.spawn(property_row(label, value)),
            };
        }
    });
}
