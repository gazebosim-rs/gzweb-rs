//! The bar a collapsed side panel leaves between the activity bar (or the
//! window's right edge) and the view (DECISIONS.md D-50), with the button
//! that brings the panel back.

use bevy::prelude::*;
use gz_bevy::view::input::InputMap;

use super::commands::UiCommand;
use super::layout::{self, Side};
use super::regions::{region, Region};
use crate::theme::{border, ColorRole, Theme, SPACE};
use crate::widgets::layout::column;

/// A collapsed panel's bar button: shows that side again.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExpandButton(pub Side);

/// The bar a collapsed side panel leaves between the activity bar (or the
/// window's right edge) and the view (DECISIONS.md D-50):
/// [`COLLAPSED_BAR_W`](layout::COLLAPSED_BAR_W) wide, `surface`, a 1px
/// `line` border facing the view, and at the top a 20 px button
/// ([`IconButtonSize::Bar`](crate::widgets::IconButtonSize::Bar)) with a
/// chevron pointing at the view that brings the panel back.
pub(crate) fn collapsed_bar(theme: &Theme, side: Side, map: &InputMap) -> impl Bundle {
    use crate::assets::icons::IconId;
    use crate::widgets::{icon_button, IconButtonSize, Tooltip, TooltipSide};
    let (region_kind, border_side, icon, title, binding, tip_side) = match side {
        Side::Sidebar => (
            Region::SidebarBar,
            UiRect::right(px(border::HAIRLINE)),
            IconId::ChevronRight,
            "Show sidebar",
            "ui.sidebar",
            TooltipSide::Right,
        ),
        Side::Inspector => (
            Region::InspectorBar,
            UiRect::left(px(border::HAIRLINE)),
            IconId::ChevronLeft,
            "Show inspector",
            "ui.inspector",
            TooltipSide::Left,
        ),
    };
    let tip = Tooltip::new(title).binding(map, binding).side(tip_side);
    (
        region(
            region_kind,
            Node {
                width: px(layout::COLLAPSED_BAR_W),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::top(px(SPACE[3])),
                border: border_side,
                display: Display::None,
                ..column(0.0)
            },
            ColorRole::Surface,
        ),
        children![(
            ExpandButton(side),
            icon_button(theme, icon, IconButtonSize::Bar, tip),
        )],
    )
}

/// A collapsed bar's button expands its side.
pub(crate) fn on_expand(
    ev: On<bevy::ui_widgets::Activate>,
    buttons: Query<&ExpandButton>,
    mut out: MessageWriter<UiCommand>,
) {
    if let Ok(b) = buttons.get(ev.entity) {
        out.write(UiCommand::Show(b.0));
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_expand);
}
