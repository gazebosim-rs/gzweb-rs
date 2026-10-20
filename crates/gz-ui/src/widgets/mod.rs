//! The spec §5 components, as builder functions that return bundles (spec
//! §5: "a Bevy UI bundle-builder function … returning an entity with
//! `Node`, the role components, and its behaviour marker"). They sit on
//! Bevy's headless `bevy_ui_widgets` where one exists (button, checkbox,
//! radio, slider, menu, popover positioning, text input), so focus,
//! keyboard and AccessKit behaviour come from Bevy.
//!
//! Sizes come from [`Theme::s`](crate::theme::Theme), colours from roles,
//! spacing and radii from the tokens. Every control gets an AccessKit name:
//! its tooltip or its label (spec §10).

pub mod activity;
pub mod badge;
pub mod button;
pub mod chip;
pub mod cursor;
pub mod focus;
pub mod layout;
pub mod menu;
pub mod number_field;
pub mod section;
pub mod segmented;
pub mod select;
pub mod shortcut;
pub mod slider;
pub mod splitter;
pub mod text;
pub mod text_field;
pub mod toggle;
pub mod tooltip;
pub mod tree_row;

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;

pub use crate::theme::visuals::ForceState;
use crate::theme::visuals::Visuals;
use crate::theme::{radius, Fill, FocusRing};

pub use activity::activity_button;
pub use badge::{badge, BadgeKind};
pub use button::{
    bar_button, bar_tool_button, button, icon_button, primary_button, text_button, tool_button,
    ButtonVariant, IconButtonSize, TextButtonProps,
};
pub use chip::{filter_chip, status_chip, StatusKind};
pub use menu::{
    menu, menu_checkbox, menu_group_label, menu_item, menu_panel, menu_separator, MenuItemProps,
};
pub use number_field::{number_field, AxisTag, NumberCommit, NumberField, Unit};
pub use section::section_header;
pub use segmented::{button_group, segmented_group};
pub use select::{select, SelectChanged};
pub use slider::{slider_row, SliderProps};
pub use splitter::{splitter, Splitter};
pub use text_field::{text_field, TextSubmit};
pub use toggle::toggle_row;
pub use tooltip::{tooltip_card, Tooltip, TooltipSide};
pub use tree_row::{tree_row, EntityKind, TreeRowSpec};

/// An AccessKit node with a name.
pub fn a11y(role: Role, label: impl Into<String>) -> AccessibilityNode {
    let mut node = accesskit::Node::new(role);
    node.set_label(label.into());
    AccessibilityNode(node)
}

/// What every clickable widget carries: pointer hover tracking, a tab stop,
/// the focus ring, its state visuals and an accessible name.
pub fn clickable(visuals: Visuals, role: Role, label: impl Into<String>) -> impl Bundle {
    (
        Hovered::default(),
        TabIndex(0),
        FocusRing,
        visuals,
        Fill(visuals.fill),
        a11y(role, label),
    )
}

/// A rounded rectangle node.
pub fn rounded(r: f32) -> BorderRadius {
    BorderRadius::all(px(r))
}

/// Default radius for small controls.
pub const CONTROL_RADIUS: f32 = radius::SM;

/// Registers every widget's systems and observers.
pub(crate) fn build(app: &mut App) {
    button::build(app);
    menu::build(app);
    number_field::build(app);
    select::build(app);
    slider::build(app);
    splitter::build(app);
    text_field::build(app);
    toggle::build(app);
    tooltip::build(app);
    tree_row::build(app);
}

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
