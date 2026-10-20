//! Select (§5.10): a 32 px field (radius 6, `field` fill, `line` border,
//! Geist 13) that opens a [`menu`](super::menu()) of options; the current one
//! shows as the menu's current choice.

use accesskit::Role;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::menu::{menu, menu_item, MenuSide};
use super::{clickable, rounded, spacer};
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
pub struct SelectOption(pub usize);

/// The trigger's label.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SelectLabel;

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
            spacer(),
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
                    let mut e =
                        p.spawn((menu_item(&item_theme, label, None, None), SelectOption(i)));
                    if i == selected {
                        e.insert(Active);
                    }
                }
            },
        ),
    )
}

/// Picking an option: updates the [`Select`], the current-choice look and
/// the trigger label, and sends [`SelectChanged`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_select_option(
    activate: On<Activate>,
    options: Query<(&SelectOption, &ChildOf)>,
    parents: Query<&ChildOf>,
    mut selects: Query<&mut Select>,
    children: Query<&Children>,
    labels: Query<(), With<SelectLabel>>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let Ok((option, popup)) = options.get(activate.entity) else {
        return;
    };
    let Ok(root) = parents.get(popup.parent()).map(ChildOf::parent) else {
        return;
    };
    let Ok(mut select) = selects.get_mut(root) else {
        return;
    };
    select.selected = option.0;
    let label = select.options.get(option.0).cloned().unwrap_or_default();
    for e in children.iter_descendants(root) {
        if let Ok((o, _)) = options.get(e) {
            if o.0 == option.0 {
                commands.entity(e).insert(Active);
            } else {
                commands.entity(e).remove::<Active>();
            }
        }
        if labels.contains(e) {
            if let Ok(mut t) = texts.get_mut(e) {
                t.0.clone_from(&label);
            }
        }
    }
    commands.trigger(SelectChanged {
        entity: root,
        index: option.0,
    });
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_select_option);
}
