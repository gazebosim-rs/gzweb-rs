//! TreeRow (§5.7): 28 tall, radius 6, padding 0 8, gap 6, Geist 13.
//! Indent (depth × 14), caret (14 wide), entity badge (16, radius 4, letter
//! in mono 10/500), name, and a visibility toggle at the right.
//!
//! Selected: `accent_soft` fill, weight 500, the badge inverts (`surface` on
//! `accent`). Hidden: `eye_off` in `text_3`, the row at 50% opacity;
//! visible rows show `eye` only on hover. Carets use the chevron icons:
//! Geist has no ▾/▸ glyphs (DECISIONS.md D-26).
//!
//! Recyclable (DECISIONS.md D-67): every part follows the row's
//! [`TreeRowSpec`], so a virtualized list reuses a row for another item
//! by writing its spec. Controlled, like `bevy_feathers`' widgets: the
//! caret and the eye don't change the spec, they trigger
//! [`TreeRowExpand`] and [`TreeRowVisibility`] on the row, and whoever
//! owns the tree's state answers by writing the spec. A row isn't a tab
//! stop: the tree it sits in is one (§6.5's arrow keys move within it).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as Headless};

use super::layout::flex_spacer;
use super::{a11y, focus, rounded};
use crate::assets::fonts::{text, Type};
use crate::assets::icons::{icon, Icon, IconId};
use crate::theme::visuals::{Active, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Ink, Opacity, Ring, Theme, TypeStyle, SPACE};
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

/// One row's content. Writing it repaints the row (a virtualized list
/// reuses rows this way).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct TreeRowSpec {
    pub depth: u32,
    pub kind: EntityKind,
    pub name: String,
    /// `Some(expanded)` for rows with children, `None` for leaves.
    pub expanded: Option<bool>,
    /// Hidden in the views: `eye_off`, the row at 50%.
    pub hidden: bool,
    /// It has a visibility toggle (entities that draw something).
    pub hideable: bool,
}

/// The row's caret was clicked: `expanded` is what the row asks for. The
/// tree's owner answers by writing the row's [`TreeRowSpec`].
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRowExpand {
    /// The row.
    pub entity: Entity,
    pub expanded: bool,
}

/// The row's eye was clicked: `hidden` is what the row asks for. The
/// owner of the entity's visibility answers by writing the row's
/// [`TreeRowSpec`].
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRowVisibility {
    /// The row.
    pub entity: Entity,
    pub hidden: bool,
}

/// The row's parts, found by `tree_row_system`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeRowPart {
    Indent,
    Caret,
    CaretGlyph,
    Badge,
    Letter,
    Name,
    Eye,
    EyeGlyph,
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
    let badge = s.entity_badge;
    let indent = s.tree_indent;
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
        Hovered::default(),
        focus::PressFocusless,
        // The tree's keyboard cursor (the tree holds the focus): drawn
        // inside the row, which its neighbours touch.
        Ring(ColorRole::Clear),
        Outline::new(px(border::FOCUS_RING), px(-border::FOCUS_RING), Color::NONE),
        visuals,
        Fill(visuals.fill),
        a11y(Role::TreeItem, spec.name.clone()),
        spec,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn((
                TreeRowPart::Indent,
                Node {
                    width: px(indent_w(indent, row.depth)),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            p.spawn((
                TreeRowPart::Caret,
                Node {
                    width: px(CARET_W),
                    height: px(CARET_W),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Headless,
                a11y(Role::Button, caret_label(row.expanded)),
                children![(
                    TreeRowPart::CaretGlyph,
                    icon(
                        caret_icon(row.expanded),
                        CARET_ICON,
                        caret_ink(row.expanded)
                    )
                )],
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
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
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
                    display: display_eye(row.hideable),
                    ..default()
                },
                Headless,
                Hovered::default(),
                a11y(Role::Button, eye_label(&row)),
                children![(
                    TreeRowPart::EyeGlyph,
                    icon(IconId::Eye, EYE_ICON, ColorRole::Clear)
                )],
            ));
        })),
    )
}

/// The indent spacer's width at `depth`, `unit` a level.
fn indent_w(unit: f32, depth: u32) -> f32 {
    depth as f32 * unit
}

/// ▾ open, ▸ closed; a leaf draws nothing (the glyph is clear).
fn caret_icon(expanded: Option<bool>) -> IconId {
    if expanded == Some(true) {
        IconId::ChevronDown
    } else {
        IconId::ChevronRight
    }
}

fn caret_ink(expanded: Option<bool>) -> ColorRole {
    if expanded.is_some() {
        ColorRole::Text3
    } else {
        ColorRole::Clear
    }
}

fn caret_label(expanded: Option<bool>) -> &'static str {
    match expanded {
        Some(true) => "Collapse",
        Some(false) => "Expand",
        None => "",
    }
}

fn eye_label(spec: &TreeRowSpec) -> String {
    let verb = if spec.hidden { "Show" } else { "Hide" };
    format!("{verb} {}", spec.name)
}

fn display_eye(hideable: bool) -> Display {
    if hideable {
        Display::Flex
    } else {
        Display::None
    }
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
    texts: Query<'w, 's, &'static mut Text>,
    nodes: Query<'w, 's, &'static mut Node>,
    access: Query<'w, 's, &'static mut AccessibilityNode>,
    opacity: Query<'w, 's, &'static Opacity>,
    theme: Res<'w, Theme>,
    commands: Commands<'w, 's>,
}

impl RowParts<'_, '_> {
    /// Every part from `spec` and the row's selection and hover.
    fn paint(&mut self, row: Entity, spec: &TreeRowSpec, selected: bool, hovered: bool) {
        let alpha = if spec.hidden { 0.5 } else { 1.0 };
        if let Ok(mut a) = self.access.get_mut(row) {
            let node = &a.0;
            let level = spec.depth as usize + 1;
            if node.label() != Some(spec.name.as_str())
                || node.is_expanded() != spec.expanded
                || node.level() != Some(level)
                || node.is_selected() != Some(selected)
            {
                let n = &mut a.0;
                n.set_label(spec.name.clone());
                match spec.expanded {
                    Some(e) => n.set_expanded(e),
                    None => n.clear_expanded(),
                }
                n.set_level(level);
                n.set_selected(selected);
            }
        }
        let descendants: Vec<Entity> = self.children.iter_descendants(row).collect();
        for part_entity in descendants {
            if self.opacity.get(part_entity).map_or(true, |o| o.0 != alpha) {
                self.commands.entity(part_entity).insert(Opacity(alpha));
            }
            let Ok(&part) = self.parts.get(part_entity) else {
                continue;
            };
            match part {
                TreeRowPart::Indent => {
                    if let Ok(mut n) = self.nodes.get_mut(part_entity) {
                        let w = px(indent_w(self.theme.s.tree_indent, spec.depth));
                        if n.width != w {
                            n.width = w;
                        }
                    }
                }
                TreeRowPart::Caret => {
                    self.label(part_entity, caret_label(spec.expanded));
                }
                TreeRowPart::CaretGlyph => {
                    self.glyph(
                        part_entity,
                        caret_icon(spec.expanded),
                        caret_ink(spec.expanded),
                    );
                }
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
                    self.text(part_entity, spec.kind.letter());
                }
                TreeRowPart::Name => {
                    if let Ok(mut t) = self.types.get_mut(part_entity) {
                        t.set_if_neq(Type(name_type(selected)));
                    }
                    self.text(part_entity, &spec.name);
                }
                TreeRowPart::Eye => {
                    if let Ok(mut n) = self.nodes.get_mut(part_entity) {
                        let d = display_eye(spec.hideable);
                        if n.display != d {
                            n.display = d;
                        }
                    }
                    self.label(part_entity, &eye_label(spec));
                }
                TreeRowPart::EyeGlyph => {
                    let id = if spec.hidden {
                        IconId::EyeOff
                    } else {
                        IconId::Eye
                    };
                    let ink = if spec.hidden || hovered {
                        ColorRole::Text3
                    } else {
                        ColorRole::Clear
                    };
                    self.glyph(part_entity, id, ink);
                }
            }
        }
    }

    fn text(&mut self, e: Entity, value: &str) {
        if let Ok(mut t) = self.texts.get_mut(e) {
            crate::widgets::text::set_text(&mut t, value);
        }
    }

    fn label(&mut self, e: Entity, label: &str) {
        if let Ok(mut a) = self.access.get_mut(e) {
            if a.0.label() != Some(label) {
                a.0.set_label(label.to_string());
            }
        }
    }

    fn glyph(&mut self, e: Entity, id: IconId, ink: ColorRole) {
        if let Ok(mut i) = self.icons.get_mut(e) {
            if i.id != id {
                i.id = id;
            }
        }
        if let Ok(mut i) = self.inks.get_mut(e) {
            i.set_if_neq(Ink(ink));
        }
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

/// The caret asks for the row's other expansion, the eye for its other
/// visibility; neither changes the spec (controlled).
pub(crate) fn on_part_activate(
    activate: On<Activate>,
    parts: Query<(&TreeRowPart, &ChildOf)>,
    rows: Query<&TreeRowSpec>,
    mut commands: Commands,
) {
    let Ok((&part, parent)) = parts.get(activate.entity) else {
        return;
    };
    let row = parent.parent();
    let Ok(spec) = rows.get(row) else {
        return;
    };
    match part {
        TreeRowPart::Caret => {
            if let Some(open) = spec.expanded {
                commands.trigger(TreeRowExpand {
                    entity: row,
                    expanded: !open,
                });
            }
        }
        TreeRowPart::Eye if spec.hideable => {
            commands.trigger(TreeRowVisibility {
                entity: row,
                hidden: !spec.hidden,
            });
        }
        _ => {}
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_part_activate)
        .add_systems(Update, tree_row_system.in_set(GzUiSystems::Visuals));
}

#[cfg(test)]
mod tests;
