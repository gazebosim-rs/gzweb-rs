//! TreeRow (§5.7), static for now (virtualization comes with the Scene
//! panel in M4): 28 tall, radius 6, padding 0 8, gap 6, Geist 13. Indent
//! (depth × 14), caret (14 wide), entity badge (16, radius 4, letter in
//! mono 10/500), name, and a visibility toggle at the right.
//!
//! Selected: `accent_soft` fill, weight 500, the badge inverts (`surface` on
//! `accent`). Hidden: `eye_off` in `text_3`, the row at 50% opacity;
//! visible rows show `eye` only on hover. Carets use the chevron icons:
//! Geist has no ▾/▸ glyphs (DECISIONS.md D-26).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as Headless};

use super::layout::flex_spacer;
use super::{a11y, clickable, rounded};
use crate::assets::fonts::{text, Type};
use crate::assets::icons::{icon, Icon, IconId};
use crate::theme::visuals::{Active, Visuals};
use crate::theme::{radius, ColorRole, Fill, Ink, Opacity, Theme, TypeStyle, SPACE};
use crate::GzUiSystems;

/// The caret column's width (§5.7: "caret 14 wide").
pub(crate) const CARET_W: f32 = 14.0;
/// The caret icon (chevron) size, the size of the spec's mono 10 caret.
pub(crate) const CARET_ICON: f32 = 10.0;
/// The visibility toggle's box (§5.7: 16×16).
pub(crate) const EYE_BOX: f32 = 16.0;
/// The eye icon inside it.
pub(crate) const EYE_ICON: f32 = 14.0;

/// Entity kinds and their badge letters (§5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    World,
    Model,
    Light,
    Link,
    Joint,
    Sensor,
}

impl EntityKind {
    pub fn letter(self) -> &'static str {
        match self {
            EntityKind::World => "W",
            EntityKind::Model => "M",
            EntityKind::Light => "L",
            EntityKind::Link => "K",
            EntityKind::Joint => "J",
            EntityKind::Sensor => "S",
        }
    }

    pub fn role(self) -> ColorRole {
        match self {
            EntityKind::World => ColorRole::EntityWorld,
            EntityKind::Model => ColorRole::EntityModel,
            EntityKind::Light => ColorRole::EntityLight,
            EntityKind::Link => ColorRole::EntityLink,
            EntityKind::Joint => ColorRole::EntityJoint,
            EntityKind::Sensor => ColorRole::EntitySensor,
        }
    }
}

/// One row's content.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct TreeRowSpec {
    pub depth: u32,
    pub kind: EntityKind,
    pub name: String,
    /// `Some(expanded)` for rows with children, `None` for leaves.
    pub expanded: Option<bool>,
    pub hidden: bool,
}

/// The row's parts, found by `tree_row_system`.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) enum TreeRowPart {
    Badge,
    Letter,
    Name,
    Eye,
}

/// The type of a row's name: Geist 13, 500 when selected.
pub(crate) fn name_type(selected: bool) -> TypeStyle {
    TypeStyle {
        weight: if selected { 500 } else { 400 },
        ..TypeStyle::BODY
    }
}

/// A tree row. Add `Active` to select it.
pub fn tree_row(theme: &Theme, spec: TreeRowSpec) -> impl Bundle {
    let s = &theme.s;
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    visuals.active_ink = ColorRole::Text;
    let indent = spec.depth as f32 * s.tree_indent;
    let badge = s.entity_badge;
    let caret = spec.expanded.map(|open| {
        if open {
            IconId::ChevronDown
        } else {
            IconId::ChevronRight
        }
    });
    let row = spec.clone();
    (
        Node {
            height: px(s.tree_row_h),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[4])),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Headless,
        clickable(visuals, Role::TreeItem, spec.name.clone()),
        spec,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(Node {
                width: px(indent),
                flex_shrink: 0.0,
                ..default()
            });
            p.spawn((
                Node {
                    width: px(CARET_W),
                    height: px(CARET_W),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                    if let Some(caret) = caret {
                        p.spawn(icon(caret, CARET_ICON, ColorRole::Text3));
                    }
                })),
            ));
            p.spawn((
                TreeRowPart::Badge,
                Node {
                    width: px(badge),
                    height: px(badge),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::XS),
                    ..default()
                },
                Fill(ColorRole::EntityBadgeBg),
                children![(
                    TreeRowPart::Letter,
                    text(TypeStyle::TINY, row.kind.role(), row.kind.letter())
                )],
            ));
            p.spawn((
                TreeRowPart::Name,
                text(name_type(false), ColorRole::Text, row.name.clone()),
            ));
            p.spawn(flex_spacer());
            p.spawn((
                TreeRowPart::Eye,
                Node {
                    width: px(EYE_BOX),
                    height: px(EYE_BOX),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::XS),
                    ..default()
                },
                Headless,
                Hovered::default(),
                a11y(Role::Button, if row.hidden { "Show" } else { "Hide" }),
                children![icon(IconId::Eye, EYE_ICON, ColorRole::Clear)],
            ));
        })),
    )
}

/// A tree row's parts, and [`paint`](Self::paint) to write a row's state
/// into them.
#[derive(SystemParam)]
pub(crate) struct RowParts<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    parts: Query<'w, 's, &'static TreeRowPart>,
    fills: Query<'w, 's, &'static mut Fill>,
    inks: Query<'w, 's, &'static mut Ink>,
    types: Query<'w, 's, &'static mut Type>,
    icons: Query<'w, 's, &'static mut Icon>,
    opacity: Query<'w, 's, &'static Opacity>,
    commands: Commands<'w, 's>,
}

impl RowParts<'_, '_> {
    /// Badge, name weight, eye icon and opacity from `row`'s state.
    fn paint(&mut self, row: Entity, spec: &TreeRowSpec, selected: bool, hovered: bool) {
        let alpha = if spec.hidden { 0.5 } else { 1.0 };
        for part_entity in self.children.iter_descendants(row) {
            if self.opacity.get(part_entity).map_or(true, |o| o.0 != alpha) {
                self.commands.entity(part_entity).insert(Opacity(alpha));
            }
            let Ok(part) = self.parts.get(part_entity) else {
                continue;
            };
            match part {
                TreeRowPart::Badge => {
                    if let Ok(mut f) = self.fills.get_mut(part_entity) {
                        f.set_if_neq(Fill(if selected {
                            ColorRole::Accent
                        } else {
                            ColorRole::EntityBadgeBg
                        }));
                    }
                }
                TreeRowPart::Letter => {
                    if let Ok(mut i) = self.inks.get_mut(part_entity) {
                        i.set_if_neq(Ink(if selected {
                            ColorRole::Surface
                        } else {
                            spec.kind.role()
                        }));
                    }
                }
                TreeRowPart::Name => {
                    if let Ok(mut t) = self.types.get_mut(part_entity) {
                        t.set_if_neq(Type(name_type(selected)));
                    }
                }
                TreeRowPart::Eye => {
                    let shown = spec.hidden || hovered;
                    for &glyph in self.children.get(part_entity).into_iter().flatten() {
                        paint_eye(&mut self.icons, &mut self.inks, glyph, spec.hidden, shown);
                    }
                }
            }
        }
    }
}

/// An eye glyph: `eye_off` while `hidden`; drawn only while `shown`
/// (hidden or hovered).
fn paint_eye(
    icons: &mut Query<&mut Icon>,
    inks: &mut Query<&mut Ink>,
    glyph: Entity,
    hidden: bool,
    shown: bool,
) {
    if let Ok(mut i) = icons.get_mut(glyph) {
        let id = if hidden { IconId::EyeOff } else { IconId::Eye };
        if i.id != id {
            i.id = id;
        }
    }
    if let Ok(mut i) = inks.get_mut(glyph) {
        i.set_if_neq(Ink(if shown {
            ColorRole::Text3
        } else {
            ColorRole::Clear
        }));
    }
}

/// Badge, name weight, eye icon and opacity from each row's state, for
/// rows that are new or whose spec, selection or hover changed.
#[allow(clippy::type_complexity)]
pub(crate) fn tree_row_system(
    rows: Query<(Entity, &TreeRowSpec, Has<Active>, &Hovered)>,
    changed: Query<
        Entity,
        (
            With<TreeRowSpec>,
            Or<(Changed<TreeRowSpec>, Changed<Hovered>, Added<Active>)>,
        ),
    >,
    mut deselected: RemovedComponents<Active>,
    mut dirty: Local<Vec<Entity>>,
    mut parts: RowParts,
) {
    dirty.clear();
    dirty.extend(changed.iter().chain(deselected.read()));
    dirty.sort_unstable();
    dirty.dedup();
    for (row, spec, selected, hovered) in dirty.iter().filter_map(|&e| rows.get(e).ok()) {
        parts.paint(row, spec, selected, hovered.get());
    }
}

/// Clicking the eye toggles the row's hidden state.
pub(crate) fn on_eye_activate(
    activate: On<Activate>,
    parts: Query<(&TreeRowPart, &ChildOf)>,
    mut rows: Query<&mut TreeRowSpec>,
) {
    if let Ok((TreeRowPart::Eye, parent)) = parts.get(activate.entity) {
        if let Ok(mut spec) = rows.get_mut(parent.parent()) {
            spec.hidden = !spec.hidden;
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_eye_activate)
        .add_systems(Update, tree_row_system.in_set(GzUiSystems::Visuals));
}

#[cfg(test)]
mod tests;
