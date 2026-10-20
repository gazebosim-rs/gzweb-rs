//! Select (§5.10): a 32 px field (radius 6, `field` fill, `line` border,
//! Geist 13) that opens a [`menu`](super::menu()) of options; the current one
//! shows as the menu's current choice.

use accesskit::Role;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::layout::flex_spacer;
use super::menu::{menu, menu_item, MenuItemProps, MenuSide};
use super::{clickable, rounded};
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Active, Content, Visuals};
use crate::theme::{border, radius, ColorRole, Theme, TypeStyle, SPACE};

/// A select's options and choice (on the menu root).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Select {
    pub options: Vec<String>,
    pub selected: usize,
}

/// Option `n` of the enclosing [`Select`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectOption(pub usize);

/// The trigger's label.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SelectLabel;

/// Sent to the [`Select`] root when the user picks an option.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectChanged {
    pub entity: Entity,
    pub index: usize,
}

/// A select with `options`, showing `selected`. `name` is its accessible name.
pub fn select(
    theme: &Theme,
    name: impl Into<String>,
    options: Vec<String>,
    selected: usize,
    width: f32,
) -> impl Bundle {
    let current = options.get(selected).cloned().unwrap_or_default();
    let icon_px = theme.s.icon_sm;
    let visuals = Visuals::new(ColorRole::Field, ColorRole::Text)
        .stroked(ColorRole::Line)
        .keep_ink();
    let trigger = (
        Node {
            width: px(width),
            height: px(theme.s.input_h),
            padding: UiRect {
                left: px(SPACE[5]),
                right: px(SPACE[4]),
                ..default()
            },
            column_gap: px(SPACE[3]),
            border: UiRect::all(px(border::HAIRLINE)),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        clickable(visuals, Role::ComboBox, name),
        children![
            (
                SelectLabel,
                text(TypeStyle::BODY, ColorRole::Text, current),
                Content
            ),
            flex_spacer(),
            content_icon(IconId::ChevronDown, icon_px, ColorRole::Text2),
        ],
    );
    let item_theme = theme.clone();
    let items = options.clone();
    (
        Select { options, selected },
        menu(
            theme,
            MenuSide::Below,
            trigger,
            move |p: &mut ChildSpawner| {
                for (i, label) in items.into_iter().enumerate() {
                    let mut e = p.spawn((
                        menu_item(&item_theme, MenuItemProps::new(label)),
                        SelectOption(i),
                    ));
                    if i == selected {
                        e.insert(Active);
                    }
                }
            },
        ),
    )
}

/// A select's parts: its options, its label, and the hierarchy between.
#[derive(SystemParam)]
pub(crate) struct SelectTree<'w, 's> {
    options: Query<'w, 's, (&'static SelectOption, &'static ChildOf)>,
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    labels: Query<'w, 's, (), With<SelectLabel>>,
}

impl SelectTree<'_, '_> {
    /// The option `entity` is (its index) and the select it belongs to
    /// (the root above its popup).
    fn option(&self, entity: Entity) -> Option<(usize, Entity)> {
        let (option, popup) = self.options.get(entity).ok()?;
        let root = self.parents.get(popup.parent()).ok()?.parent();
        Some((option.0, root))
    }

    /// The select's options (entity, index) and its label, under `root`.
    fn parts(&self, root: Entity) -> (Vec<(Entity, usize)>, Option<Entity>) {
        let mut options = Vec::new();
        let mut label = None;
        for e in self.children.iter_descendants(root) {
            if let Ok((o, _)) = self.options.get(e) {
                options.push((e, o.0));
            }
            if self.labels.contains(e) {
                label = Some(e);
            }
        }
        (options, label)
    }
}

/// Picking an option: updates the [`Select`], the current-choice look and
/// the trigger label, and sends [`SelectChanged`].
pub(crate) fn on_select_option(
    activate: On<Activate>,
    tree: SelectTree,
    mut selects: Query<&mut Select>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let Some((index, root)) = tree.option(activate.entity) else {
        return;
    };
    let Ok(mut select) = selects.get_mut(root) else {
        return;
    };
    select.selected = index;
    let label = select.options.get(index).cloned().unwrap_or_default();
    let (options, label_entity) = tree.parts(root);
    for (e, i) in options {
        if i == index {
            commands.entity(e).insert(Active);
        } else {
            commands.entity(e).remove::<Active>();
        }
    }
    if let Some(mut t) = label_entity.and_then(|e| texts.get_mut(e).ok()) {
        t.0.clone_from(&label);
    }
    commands.trigger(SelectChanged {
        entity: root,
        index,
    });
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_select_option);
}
