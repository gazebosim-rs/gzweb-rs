//! The Scene panel's ready body (spec §3.3; board 04): the filter bar,
//! the type chips and the virtualized tree, built from the panel's
//! resources whenever the shell spawns the body (DECISIONS.md D-58,
//! D-66).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::ecs::system::SystemParam;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, RadioGroup, ScrollArea};
use bevy::window::PrimaryWindow;
use gz_bevy::view::editing::{is_under, EditingModel};
use gz_bevy::view::Selection;

use super::model::{Chip, Row, TreeModel, TreeScroll, TreeState};
use super::systems::TreeRows;
use super::SceneTree;
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Active, Visuals};
use crate::theme::{border, radius, ColorRole, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, display, row};
use crate::widgets::{
    a11y, choice_chip, clickable, filter_field, rounded, tree_row, Tooltip, TreeRowSpec,
};

/// The filter field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FilterField;

/// The button that shows or hides the type chips.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FilterButton;

/// The type chips' row (a radio group).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct ChipRow;

/// A type chip.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChipChoice(pub(crate) Chip);

/// The tree's scrolling list: the tab stop that takes the arrow keys.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TreeList;

/// The list's content, as tall as every row.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct TreeContent;

/// A pooled row: which of [`TreeRows`] it shows (none: hidden).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TreeSlot(pub(crate) Option<usize>);

/// "No entities match", while a filter hides every row.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct NoMatches;

/// What a body and its rows are built from.
#[derive(SystemParam)]
pub(super) struct TreeSources<'w, 's> {
    pub(crate) theme: Res<'w, Theme>,
    pub(crate) model: Res<'w, TreeModel>,
    pub(crate) state: Res<'w, TreeState>,
    pub(crate) rows: Res<'w, TreeRows>,
    pub(crate) visibility: Query<'w, 's, &'static Visibility>,
    pub(crate) selection: Res<'w, Selection>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    /// While a model is edited, the rows outside it dim (§6.6).
    pub(crate) editing: Option<Res<'w, EditingModel>>,
    parents: Query<'w, 's, &'static ChildOf>,
}

/// Row `row` is in the model being edited (its own row, its links',
/// their joints' and sensors'), or no model is.
pub(crate) fn in_edited(
    model: &TreeModel,
    row: Row,
    editing: Option<&EditingModel>,
    parents: &Query<&ChildOf>,
) -> bool {
    let Some(edited) = editing.and_then(EditingModel::model) else {
        return true;
    };
    let e = model
        .node(row)
        .entity
        .or_else(|| model.part(row).map(|p| p.owner));
    e.is_some_and(|e| is_under(parents, e, edited))
}

impl TreeSources<'_, '_> {
    /// What row `i` of [`TreeRows`] shows.
    pub(crate) fn spec(&self, row: Row) -> TreeRowSpec {
        let n = self.model.node(row);
        TreeRowSpec {
            depth: n.depth,
            kind: n.kind,
            name: n.name.clone(),
            expanded: row.expanded,
            hidden: n
                .entity
                .and_then(|e| self.visibility.get(e).ok())
                .is_some_and(|v| *v == Visibility::Hidden),
            hideable: n.entity.is_some(),
            dimmed: !self.in_edited(row),
        }
    }

    /// Row `row` is in the model being edited (its own row, its links',
    /// their joints' and sensors'), or no model is.
    pub(crate) fn in_edited(&self, row: Row) -> bool {
        in_edited(&self.model, row, self.editing.as_deref(), &self.parents)
    }

    /// Row `row`'s entity, or its joint or sensor, is selected (§5.7: the
    /// row's `Active` look).
    pub(crate) fn selected(&self, row: Row) -> bool {
        match self.model.node(row).entity {
            Some(e) => self.selection.entities.contains(&e),
            None => self
                .model
                .part(row)
                .is_some_and(|p| self.selection.parts.contains(&p)),
        }
    }

    /// The row height.
    pub(crate) fn row_h(&self) -> f32 {
        self.theme.s.tree_row_h
    }

    /// Rows a list as tall as the window shows: the pool a new body starts
    /// with, so it shows its rows in its first frame.
    fn first_pool(&self) -> usize {
        let h = self.windows.single().map_or(900.0, Window::height);
        pool_size(h, self.row_h())
    }
}

/// The first row any of which shows with the list scrolled `y` down. The
/// rows sit inside the list's top padding and the list clips at its
/// padding box, so the row above `y / row_h` shows for the padding's
/// height after each row boundary.
pub(crate) fn first_row(y: f32, row_h: f32) -> usize {
    ((y - SPACE[4]).max(0.0) / row_h).floor() as usize
}

/// The rows that cover a view `view_h` tall from [`first_row`]: both ends
/// can be partly in view.
pub(crate) fn pool_size(view_h: f32, row_h: f32) -> usize {
    (view_h / row_h).ceil() as usize + 2
}

/// Builds the ready body's content from the panel's resources.
pub(crate) fn build_tree(
    ev: On<Add<SceneTree>>,
    src: TreeSources,
    scroll: Res<TreeScroll>,
    mut commands: Commands,
) {
    let t = src.theme.clone();
    let row_h = src.row_h();
    let rows = &src.rows.0;
    let first = first_row(scroll.0, row_h);
    let slots: Vec<(usize, TreeRowSpec, bool)> = (first..rows.len())
        .take(src.first_pool())
        .map(|i| (i, src.spec(rows[i]), src.selected(rows[i])))
        .collect();
    let content_h = rows.len() as f32 * row_h;
    let chips_shown = src.state.chips_shown;
    let chip = src.state.chip;
    let filter = src.state.filter.clone();
    let empty = rows.is_empty() && src.state.filtering();
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
            // Filter bar: the field and the chip toggle (§3.3).
            c.spawn(Node {
                padding: UiRect {
                    left: px(SPACE[4]),
                    right: px(SPACE[4]),
                    top: px(SPACE[4]),
                    bottom: px(0),
                },
                flex_shrink: 0.0,
                ..row(SPACE[3])
            })
            .with_children(|bar| {
                bar.spawn((
                    filter_field(&t, "Filter entities", "Filter", filter),
                    FilterField,
                ));
                bar.spawn((filter_button(&t), FilterButton));
            });
            // Type chips.
            c.spawn((
                ChipRow,
                RadioGroup,
                a11y(Role::RadioGroup, "Entity type"),
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(SPACE[2]),
                    row_gap: px(SPACE[2]),
                    padding: UiRect {
                        left: px(SPACE[4]),
                        right: px(SPACE[4]),
                        top: px(SPACE[4]),
                        bottom: px(0),
                    },
                    flex_shrink: 0.0,
                    display: display(chips_shown),
                    ..default()
                },
            ))
            .with_children(|chips| {
                for c in Chip::ALL {
                    chips.spawn((choice_chip(c.label(), c == chip), ChipChoice(c)));
                }
            });
            // The tree.
            c.spawn((
                TreeList,
                ScrollArea,
                ScrollPosition(Vec2::new(0.0, scroll.0)),
                TabIndex(0),
                a11y(Role::Tree, "Entities"),
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::all(px(SPACE[4])),
                    ..column(0.0)
                },
            ))
            .with_children(|list| {
                list.spawn((
                    TreeContent,
                    // Bevy links an accessibility node only to its parent's,
                    // so the rows reach the Tree through this one.
                    AccessibilityNode(accesskit::Node::new(Role::GenericContainer)),
                    Node {
                        height: px(content_h),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|content| {
                    for (i, spec, selected) in slots {
                        spawn_slot(content, &t, Some(i), spec, selected);
                    }
                });
                list.spawn((
                    NoMatches,
                    text(TypeStyle::BODY, ColorRole::Text2, "No entities match"),
                    Node {
                        align_self: AlignSelf::Center,
                        margin: UiRect::top(px(SPACE[8])),
                        display: display(empty),
                        ..default()
                    },
                ));
            });
        });
    });
}

/// Spawns a pooled row showing row `i` (absolutely placed at its offset).
pub(super) fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    theme: &Theme,
    i: Option<usize>,
    spec: TreeRowSpec,
    selected: bool,
) {
    let row_h = theme.s.tree_row_h;
    let mut access = a11y(Role::TreeItem, spec.name.clone());
    expose(&mut access, i.is_some());
    let mut row = parent.spawn(tree_row(theme, spec));
    if selected {
        row.insert(Active);
    }
    row.insert((
        access,
        TreeSlot(i),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(i.unwrap_or(0) as f32 * row_h),
            height: px(row_h),
            padding: UiRect::horizontal(px(SPACE[4])),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            display: display(i.is_some()),
            ..default()
        },
    ));
}

/// Hides a pooled row's accessibility node while the row shows nothing, so
/// a screen reader skips it (Bevy exposes every node, displayed or not).
pub(super) fn expose(access: &mut AccessibilityNode, shown: bool) {
    if access.0.is_hidden() == shown {
        if shown {
            access.0.clear_hidden();
        } else {
            access.0.set_hidden();
        }
    }
}

/// The filter bar's button (board 04): a bordered 30 px square with the
/// funnel, in the active look while a type chip filters
/// ([`chip_states`](super::systems::chip_states)).
fn filter_button(theme: &Theme) -> impl Bundle {
    let side = theme.s.input_h_compact;
    let tip = Tooltip::new("Filter by type").detail("Show or hide the type chips");
    let visuals = Visuals::new(ColorRole::Surface2, ColorRole::Icon).stroked(ColorRole::Line);
    (
        Node {
            width: px(side),
            height: px(side),
            flex_shrink: 0.0,
            border: UiRect::all(px(border::HAIRLINE)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Headless,
        clickable(visuals, Role::Button, tip.accessible_name()),
        tip,
        children![content_icon(
            IconId::Filter,
            theme.s.icon_sm,
            ColorRole::Icon
        )],
    )
}
