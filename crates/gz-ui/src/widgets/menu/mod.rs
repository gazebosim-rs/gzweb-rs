//! Menu and MenuItem (§5.14) and Popover (§5.16), on Bevy's headless menu.
//!
//! A menu is a root node holding its trigger (a `MenuButton`) and a popup
//! (`MenuPopup`, hidden until opened). The popup is placed by Bevy's
//! `Popover`: below its trigger (or left of a rail button), flipping to
//! stay on screen. It closes on Esc, a click outside (focus leaves it) or
//! a selection. Arrow keys, Home and End move between items, Enter
//! activates, and typing a letter jumps to the next item starting with it.
//!
//! Menu: `surface`, 1px `line`, radius 10, padding 6, shadow
//! `elevation.menu`, min width `menu_min_w`. MenuItem: 30 tall, radius 6,
//! padding 0 10, gap 10, Geist 13 `text`, leading icon 15 `text_2`,
//! trailing shortcut mono 11 `text_3`; the current choice in `accent_soft`
//! / `accent_text`. A disabled item dims all of it: the icon, shortcut and
//! checkbox keep their colours ([`OwnInk`]) but take the 40% opacity.
//!
//! A submenu ([`submenu`]) is a menu inside a menu's popup: a row like an
//! item, with a chevron, whose popup opens to its right on hover, a click,
//! Enter or →; ← or Esc closes it alone, and choosing in it closes the
//! whole stack (DECISIONS.md D-109).

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::{NavAction, TabIndex};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, OverrideClip};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy::ui_widgets::{
    observe, Activate, MenuAction, MenuEvent, MenuFocusState, MenuItem, MenuPopup,
};

use super::layout::flex_spacer;
use super::{a11y, rounded};
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::visuals::{Content, HoverOnFocus, OwnInk, Visuals};
use crate::theme::{
    border, elevation, radius, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE,
};

/// Shortcut text type: mono 11/400.
pub(crate) const SHORTCUT_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);

/// Where a menu opens relative to its trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuSide {
    /// Below, flipping above.
    #[default]
    Below,
    /// Left of a rail button, flipping right.
    Left,
    /// Above (status-bar menus), flipping below.
    Above,
    /// Right of a submenu's row, flipping left.
    Right,
}

impl MenuSide {
    /// The popover placements, preferred first.
    pub fn popover(self) -> Popover {
        let (side, align) = match self {
            MenuSide::Below => (PopoverSide::Bottom, PopoverAlign::Start),
            MenuSide::Left => (PopoverSide::Left, PopoverAlign::Start),
            MenuSide::Above => (PopoverSide::Top, PopoverAlign::Start),
            MenuSide::Right => (PopoverSide::Right, PopoverAlign::Start),
        };
        // A submenu sits against its row, over the menu's padding.
        let gap = if self == MenuSide::Right {
            SPACE[3]
        } else {
            SPACE[2]
        };
        let at = |side: PopoverSide, align| PopoverPlacement { side, align, gap };
        Popover {
            // Preferred, flipped, shifted, and both: a menu clipped below
            // and to the right still fits above and to the left.
            positions: vec![
                at(side, align),
                at(side.mirror(), align),
                at(side, PopoverAlign::End),
                at(side.mirror(), PopoverAlign::End),
            ],
            window_margin: SPACE[4],
        }
    }
}

/// The menu card's box: min width `menu_min_w`, padding 6, a 1px border,
/// radius 10.
fn panel_node(theme: &Theme) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        min_width: px(theme.s.menu_min_w),
        padding: UiRect::all(px(SPACE[3])),
        border: UiRect::all(px(border::HAIRLINE)),
        border_radius: rounded(radius::LG),
        flex_shrink: 0.0,
        ..default()
    }
}

/// The menu card: `surface`, 1px `line`, radius 10, padding 6, shadow.
pub fn menu_panel(theme: &Theme) -> impl Bundle {
    (
        panel_node(theme),
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
        elevation::MENU.shadow(),
    )
}

/// The root of a menu (or popover): owns the trigger and the popup.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Menu;

/// The popup of a [`Menu`].
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MenuPopupOf;

/// A menu's trigger (spawn it with [`menu`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MenuTrigger;

/// A menu: `trigger` (a bundle that looks like a button; it gets
/// `MenuButton`) and a popup whose content `items` spawns. Popovers
/// (§5.16) are menus with arbitrary content.
pub fn menu<T, F>(theme: &Theme, side: MenuSide, trigger: T, items: F) -> impl Bundle
where
    T: Bundle,
    F: FnOnce(&mut ChildSpawner) + Send + Sync + 'static,
{
    let popup_node = Node {
        position_type: PositionType::Absolute,
        ..panel_node(theme)
    };
    (
        Menu,
        Node {
            flex_shrink: 0.0,
            ..default()
        },
        observe(on_menu_event),
        children![
            (trigger, MenuTrigger, bevy::ui_widgets::MenuButton),
            (
                MenuPopupOf,
                MenuPopup::default(),
                popup_node,
                Fill(ColorRole::Surface),
                Stroke(ColorRole::Line),
                elevation::MENU.shadow(),
                side.popover(),
                Visibility::Hidden,
                GlobalZIndex(900),
                OverrideClip,
                Children::spawn(SpawnWith(items)),
            ),
        ],
    )
}

/// Opens, closes and refocuses a [`Menu`] (Bevy's headless menu sends the
/// [`MenuEvent`]s; this owns the visibility, like `bevy_feathers`).
pub(crate) fn on_menu_event(
    mut ev: On<MenuEvent>,
    menus: Query<&Children, With<Menu>>,
    items: Query<(), With<MenuItem>>,
    mut popups: Query<&mut Visibility, With<MenuPopupOf>>,
    triggers: Query<(), With<MenuTrigger>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Ok(children) = menus.get(ev.event_target()) else {
        return;
    };
    let popup = children.iter().find(|c| popups.contains(*c));
    let trigger = children.iter().find(|c| triggers.contains(*c));
    let (Some(popup), Some(trigger)) = (popup, trigger) else {
        return;
    };
    // A choice in a submenu closes the menus it is in too; a submenu
    // losing focus to its parent closes alone.
    let chosen = items.contains(ev.original_event_target());
    ev.propagate(chosen && matches!(ev.event().action, MenuAction::CloseAll));
    let open = popups.get(popup).is_ok_and(|v| *v == Visibility::Visible);
    match ev.event().action {
        MenuAction::Open(nav) => open_popup(&mut commands, popup, nav),
        MenuAction::Toggle if open => close_popup(&mut popups, popup),
        MenuAction::Toggle => open_popup(&mut commands, popup, NavAction::First),
        MenuAction::CloseAll => close_popup(&mut popups, popup),
        MenuAction::FocusRoot => focus.set(trigger, FocusCause::Navigated),
    }
}

fn open_popup(commands: &mut Commands, popup: Entity, nav: NavAction) {
    commands
        .entity(popup)
        .try_insert((Visibility::Visible, MenuFocusState::Opening(nav)));
}

fn close_popup(popups: &mut Query<&mut Visibility, With<MenuPopupOf>>, popup: Entity) {
    if let Ok(mut v) = popups.get_mut(popup) {
        v.set_if_neq(Visibility::Hidden);
    }
}

/// An item's text, for type-to-search.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuLabel(pub String);

fn item_node(theme: &Theme) -> Node {
    Node {
        height: px(theme.s.menu_item_h),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(px(SPACE[5])),
        column_gap: px(SPACE[5]),
        align_items: AlignItems::Center,
        border_radius: rounded(radius::SM),
        ..default()
    }
}

fn item_parts(label: &str, role: Role) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    if role == Role::MenuItemCheckBox {
        // The box shows the state; the row keeps the plain look.
        visuals.checked_is_active = false;
    }
    (
        MenuItem,
        MenuLabel(label.to_string()),
        HoverOnFocus,
        Hovered::default(),
        TabIndex(0),
        visuals,
        Fill(ColorRole::Clear),
        a11y(role, label),
    )
}

/// A menu item's (or checkbox item's) label, leading icon and shortcut
/// text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuItemProps {
    pub label: String,
    /// A leading icon (15 `text_2`); a checkbox item draws it after its
    /// box.
    pub icon: Option<IconId>,
    /// The trailing shortcut text (mono 11 `text_3`).
    pub shortcut: Option<String>,
}

impl MenuItemProps {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..default()
        }
    }

    pub fn icon(mut self, id: IconId) -> Self {
        self.icon = Some(id);
        self
    }

    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// The shortcut from the input map, if the binding has keys
    /// ([`shortcut_label`](crate::widgets::shortcut::shortcut_label)).
    pub fn shortcut_opt(mut self, keys: Option<String>) -> Self {
        self.shortcut = keys;
        self
    }
}

/// A menu item: optional leading icon, label and shortcut text. Add
/// `Active` to mark the current choice.
pub fn menu_item(theme: &Theme, props: MenuItemProps) -> impl Bundle {
    let MenuItemProps {
        label,
        icon: leading,
        shortcut,
    } = props;
    let icon_px = theme.s.icon_sm;
    let text_label = label.clone();
    (
        item_node(theme),
        item_parts(&label, Role::MenuItem),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if let Some(id) = leading {
                p.spawn((icon(id, icon_px, ColorRole::Text2), Content, OwnInk));
            }
            p.spawn((text(TypeStyle::BODY, ColorRole::Text, text_label), Content));
            p.spawn(flex_spacer());
            if let Some(keys) = shortcut {
                p.spawn(shortcut_text(keys));
            }
        })),
    )
}

/// A submenu's row (the trigger of its [`Menu`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SubmenuRow;

/// A submenu: a row like an item (`label`, optional icon, a chevron) that
/// opens `items` to its right.
pub(crate) fn submenu<F>(
    theme: &Theme,
    label: &str,
    leading: Option<IconId>,
    items: F,
) -> impl Bundle
where
    F: FnOnce(&mut ChildSpawner) + Send + Sync + 'static,
{
    let icon_px = theme.s.icon_sm;
    let text_label = label.to_string();
    let row = (
        Node {
            // The full width of the menu.
            flex_grow: 1.0,
            ..item_node(theme)
        },
        SubmenuRow,
        MenuLabel(label.to_string()),
        HoverOnFocus,
        Hovered::default(),
        TabIndex(0),
        Visuals::new(ColorRole::Clear, ColorRole::Text),
        Fill(ColorRole::Clear),
        a11y(Role::MenuItem, label),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if let Some(id) = leading {
                p.spawn((icon(id, icon_px, ColorRole::Text2), Content, OwnInk));
            }
            p.spawn((text(TypeStyle::BODY, ColorRole::Text, text_label), Content));
            p.spawn(flex_spacer());
            p.spawn((
                icon(IconId::ChevronRight, icon_px, ColorRole::Text3),
                Content,
                OwnInk,
            ));
        })),
    );
    menu(theme, MenuSide::Right, row, items)
}

/// The [`Menu`] a submenu row opens, and its popup.
fn submenu_of(
    row: Entity,
    parents: &Query<&ChildOf>,
    menus: &Query<&Children, With<Menu>>,
    popups: &Query<&Visibility, With<MenuPopupOf>>,
) -> Option<(Entity, Entity, bool)> {
    let root = parents.get(row).ok()?.parent();
    let popup = menus.get(root).ok()?.iter().find(|c| popups.contains(*c))?;
    let open = popups.get(popup).is_ok_and(|v| *v == Visibility::Visible);
    Some((root, popup, open))
}

/// Hovering a submenu's row opens it; hovering another row of the same
/// menu closes it (the row hovered takes the keys, so the menu stays).
#[allow(clippy::type_complexity)]
pub(crate) fn hover_submenus(
    hovered: Query<(Entity, &Hovered), (Changed<Hovered>, Or<(With<MenuItem>, With<SubmenuRow>)>)>,
    rows: Query<Entity, With<SubmenuRow>>,
    parents: Query<&ChildOf>,
    menus: Query<&Children, With<Menu>>,
    popups: Query<&Visibility, With<MenuPopupOf>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    for (e, h) in &hovered {
        if !h.get() {
            continue;
        }
        // The popup this row is in.
        let Some(card) = parents.iter_ancestors(e).find(|a| popups.contains(*a)) else {
            continue;
        };
        for row in &rows {
            let Some((root, popup, open)) = submenu_of(row, &parents, &menus, &popups) else {
                continue;
            };
            let sibling = parents.iter_ancestors(row).find(|a| popups.contains(*a)) == Some(card);
            if !sibling {
                continue;
            }
            if row == e && !open {
                commands.trigger(MenuEvent {
                    source: root,
                    action: MenuAction::Open(NavAction::First),
                });
            } else if row != e && open {
                commands.entity(popup).try_insert(Visibility::Hidden);
                focus.set(e, FocusCause::Navigated);
            }
        }
    }
}

/// → on a submenu's row opens it; ← in a submenu closes it, back on its
/// row.
pub(crate) fn on_submenu_keys(
    mut ev: On<FocusedInput<KeyboardInput>>,
    rows: Query<(), With<SubmenuRow>>,
    parents: Query<&ChildOf>,
    menus: Query<&Children, With<Menu>>,
    popups: Query<&Visibility, With<MenuPopupOf>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if ev.input.state != ButtonState::Pressed {
        return;
    }
    let focused = ev.focused_entity;
    match ev.input.key_code {
        KeyCode::ArrowRight if rows.contains(focused) => {
            if let Some((root, ..)) = submenu_of(focused, &parents, &menus, &popups) {
                ev.propagate(false);
                commands.trigger(MenuEvent {
                    source: root,
                    action: MenuAction::Open(NavAction::First),
                });
            }
        }
        KeyCode::ArrowLeft => {
            // The nearest open submenu holding the focus, and its row.
            let popup = parents
                .iter_ancestors(focused)
                .find(|a| popups.contains(*a));
            let Some(popup) = popup else { return };
            let Some(root) = parents.get(popup).ok().map(|p| p.parent()) else {
                return;
            };
            let row = menus
                .get(root)
                .ok()
                .and_then(|c| c.iter().find(|c| rows.contains(*c)));
            if let Some(row) = row {
                ev.propagate(false);
                commands.entity(popup).try_insert(Visibility::Hidden);
                focus.set(row, FocusCause::Navigated);
            }
        }
        _ => {}
    }
}

/// An item's trailing shortcut: mono 11 `text_3`, dimmed with the item.
fn shortcut_text(keys: String) -> impl Bundle {
    (text(SHORTCUT_TYPE, ColorRole::Text3, keys), Content, OwnInk)
}

/// A menu checkbox's box.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct MenuCheckBox;

/// The 14 px box of a checkbox item (§5.14).
pub(crate) const CHECK_BOX: f32 = 14.0;

/// A checkbox item: a 14 px box (radius 4; off: 1.5px `text_3` border, on:
/// `accent` fill), the optional icon, label and shortcut. Activating it
/// toggles `Checked`.
pub fn menu_checkbox(theme: &Theme, props: MenuItemProps, checked: bool) -> impl Bundle {
    let MenuItemProps {
        label,
        icon: leading,
        shortcut,
    } = props;
    let icon_px = theme.s.icon_sm;
    let text_label = label.clone();
    (
        item_node(theme),
        item_parts(&label, Role::MenuItemCheckBox),
        bevy::ui::Checkable,
        super::button::Checkedness(checked),
        CheckItem,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn((
                MenuCheckBox,
                Node {
                    width: px(CHECK_BOX),
                    height: px(CHECK_BOX),
                    flex_shrink: 0.0,
                    border: UiRect::all(px(CHECK_BORDER)),
                    border_radius: rounded(radius::XS),
                    ..default()
                },
                Fill(ColorRole::Clear),
                Stroke(ColorRole::Text3),
                Content,
                OwnInk,
            ));
            if let Some(id) = leading {
                p.spawn((icon(id, icon_px, ColorRole::Text2), Content, OwnInk));
            }
            p.spawn((text(TypeStyle::BODY, ColorRole::Text, text_label), Content));
            p.spawn(flex_spacer());
            if let Some(keys) = shortcut {
                p.spawn(shortcut_text(keys));
            }
        })),
    )
}

/// A checkbox menu item; its state is `Checked`, not the item's active look.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CheckItem;

/// A group label (`caps` type, padding 6 10 4, `text_3`).
pub fn menu_group_label(label: impl Into<String>) -> impl Bundle {
    (
        Node {
            padding: UiRect {
                left: px(SPACE[5]),
                right: px(SPACE[5]),
                top: px(SPACE[3]),
                bottom: px(SPACE[2]),
            },
            ..default()
        },
        children![text(TypeStyle::CAPS, ColorRole::Text3, label)],
    )
}

/// A separator: 1px `line_soft`, margin 4 6.
pub fn menu_separator() -> impl Bundle {
    (
        Node {
            height: px(border::HAIRLINE),
            margin: UiRect::axes(px(SPACE[3]), px(SPACE[2])),
            flex_shrink: 0.0,
            ..default()
        },
        Fill(ColorRole::LineSoft),
    )
}

/// Toggles a checkbox item on activation.
pub(crate) fn on_check_item_activate(
    activate: On<Activate>,
    items: Query<Has<Checked>, With<CheckItem>>,
    mut commands: Commands,
) {
    if let Ok(checked) = items.get(activate.entity) {
        if checked {
            commands.entity(activate.entity).remove::<Checked>();
        } else {
            commands.entity(activate.entity).insert(Checked);
        }
    }
}

/// Draws checkbox items' boxes from `Checked`: a filled box has no border
/// (a border the same colour leaves an anti-aliasing seam at the corners).
/// A checked item keeps the plain item look; the box shows the state.
#[allow(clippy::type_complexity)]
pub(crate) fn menu_checkbox_system(
    items: Query<(Entity, Has<Checked>), With<CheckItem>>,
    changed: Query<Entity, (With<CheckItem>, Or<(Added<CheckItem>, Added<Checked>)>)>,
    mut unchecked: RemovedComponents<Checked>,
    children: Query<&Children>,
    mut boxes: Query<(&mut Fill, &mut Node), With<MenuCheckBox>>,
) {
    for (item, checked) in changed
        .iter()
        .chain(unchecked.read())
        .filter_map(|e| items.get(e).ok())
    {
        for e in children.iter_descendants(item) {
            if let Ok((mut fill, mut node)) = boxes.get_mut(e) {
                let (f, b) = if checked {
                    (ColorRole::Accent, 0.0)
                } else {
                    (ColorRole::Clear, CHECK_BORDER)
                };
                fill.set_if_neq(Fill(f));
                let border = UiRect::all(px(b));
                if node.border != border {
                    node.border = border;
                }
            }
        }
    }
}

/// The unchecked box's border width.
pub(crate) const CHECK_BORDER: f32 = 1.5;

/// The item after `from` (wrapping) whose label starts with `c`.
pub(crate) fn type_to_search(labels: &[&str], from: Option<usize>, c: char) -> Option<usize> {
    let n = labels.len();
    let lower: String = c.to_lowercase().collect();
    let start = from.map_or(0, |i| i + 1);
    (0..n)
        .map(|k| (start + k) % n)
        .find(|&i| labels[i].to_lowercase().starts_with(&lower))
}

/// What type-to-search jumps between: items and submenu rows.
type Typed = Or<(With<MenuItem>, With<SubmenuRow>)>;

/// Type-to-search inside an open menu: plain letters only, so a chord
/// (Ctrl/Cmd/Alt+letter) is left to the shortcuts.
pub(crate) fn on_menu_typing(
    mut ev: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    items: Query<(&MenuLabel, &ChildOf), Typed>,
    popups: Query<&Children, With<MenuPopup>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
) {
    let input = &ev.input;
    let chord = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ]);
    if input.state != ButtonState::Pressed || chord || ev.event_target() != ev.focused_entity {
        return;
    }
    let Key::Character(ch) = &input.logical_key else {
        return;
    };
    let Some(c) = ch.chars().next() else {
        return;
    };
    if !items.contains(ev.focused_entity) {
        return;
    }
    // The popup it's in (its items may sit in a column inside it), and
    // its items and submenu rows in order, leaving out a submenu's items.
    let Some(popup) = parents
        .iter_ancestors(ev.focused_entity)
        .find(|a| popups.contains(*a))
    else {
        return;
    };
    let mut list: Vec<(Entity, &str)> = vec![];
    let mut stack: Vec<Entity> = popups
        .get(popup)
        .map(|c| c.iter().rev().collect())
        .unwrap_or_default();
    while let Some(e) = stack.pop() {
        if let Ok((l, _)) = items.get(e) {
            list.push((e, l.0.as_str()));
        } else if !popups.contains(e) {
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter().rev());
            }
        }
    }
    let labels: Vec<&str> = list.iter().map(|(_, l)| *l).collect();
    let from = list.iter().position(|(e, _)| *e == ev.focused_entity);
    if let Some(i) = type_to_search(&labels, from, c) {
        ev.propagate(false);
        focus.set(list[i].0, FocusCause::Navigated);
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_check_item_activate)
        .add_observer(on_menu_typing)
        .add_observer(on_submenu_keys)
        .add_systems(Update, hover_submenus)
        .add_systems(
            Update,
            menu_checkbox_system.in_set(crate::GzUiSystems::Visuals),
        );
}

#[cfg(test)]
mod tests;
