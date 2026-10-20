//! The inspector (spec §3.5), as a frame: the section header with pin and
//! collapse, the body of the inspector for what is selected
//! ([`InspectorRegistry`]; with nothing selected, always until M5, the
//! World inspector), and the splitter.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use super::regions::side_panel;
use super::splitter::side_splitter;
use super::{ShellLayout, Side, UiCommand};
use crate::assets::icons::IconId;
use crate::inspectors::InspectorRegistry;
use crate::theme::visuals::Active;
use crate::theme::Theme;
use crate::widgets::layout::column;
use crate::widgets::{icon_button, section_header, IconButtonSize, Tooltip};

/// The inspector header's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InspectorHeader {
    /// Keeps showing this entity when the selection changes (M5).
    Pin,
    Collapse,
}

/// The inspector region, with the body of the inspector for the
/// selection.
pub(crate) fn inspector(
    theme: &Theme,
    layout: &ShellLayout,
    map: &InputMap,
    inspectors: &InspectorRegistry,
) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    let body = inspectors.for_selection().map(|i| i.body);
    (
        side_panel(Side::Inspector, layout),
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn(section_header(&t, "Inspector", None))
                .with_children(|h| {
                    let small = IconButtonSize::Small;
                    h.spawn((
                        icon_button(
                            &t,
                            IconId::Pin,
                            small,
                            Tooltip::new("Pin")
                                .detail("Keep showing this when the selection changes"),
                        ),
                        InspectorHeader::Pin,
                    ));
                    h.spawn((
                        icon_button(
                            &t,
                            IconId::ChevronRight,
                            small,
                            Tooltip::new("Collapse inspector").binding(&map, "ui.inspector"),
                        ),
                        InspectorHeader::Collapse,
                    ));
                });
            p.spawn(Node {
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::clip(),
                ..column(0.0)
            })
            .with_children(|b| {
                if let Some(body) = body {
                    body(b, &t);
                }
            });
            p.spawn(side_splitter(&t, Side::Inspector));
        })),
    )
}

/// The header's buttons: Pin toggles, the chevron collapses the
/// inspector.
pub(crate) fn on_header_button(
    ev: On<Activate>,
    buttons: Query<(&InspectorHeader, Has<Active>)>,
    mut out: MessageWriter<UiCommand>,
    mut commands: Commands,
) {
    let Ok((which, active)) = buttons.get(ev.entity) else {
        return;
    };
    match which {
        InspectorHeader::Collapse => {
            out.write(UiCommand::ToggleInspector);
        }
        InspectorHeader::Pin => {
            if active {
                commands.entity(ev.entity).remove::<Active>();
            } else {
                commands.entity(ev.entity).insert(Active);
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_header_button);
}
