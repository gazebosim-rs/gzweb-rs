//! The inspector (spec §3.5), as a shell: the section header with pin and
//! collapse, and with nothing selected (always, until M5) the World
//! inspector: WORLD tag and the world's name, the source line, the quick
//! actions, and the Physics, Scene and Plugins groups as placeholders.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzCommand, GzStatus};
use gz_bevy::spawn::GzIndex;
use gz_bevy::view::input::InputMap;

use super::connection::{CurrentPhase, Phase};
use super::regions::side_panel;
use super::splitter::side_splitter;
use super::top_bar::world_button::world_name;
use super::{format, ShellLayout, Side, UiCommand};
use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::theme::visuals::Active;
use crate::theme::{border, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{self, column, flex_spacer, group_header, property_row, row};
use crate::widgets::text::set_text;
use crate::widgets::{
    button, icon_button, rounded, section_header, ButtonVariant, IconButtonSize, Tooltip,
};

/// The type tag's type: mono 11/500 (§3.5).
pub const TAG_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 500);
/// The breadcrumb's type: mono 11 `text_3`.
pub const CRUMB_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);

/// The inspector's header buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorButton {
    /// Keeps showing this entity when the selection changes (M5).
    Pin,
    Collapse,
    /// World quick actions (§3.5): Save (M8), Reset, Statistics (M4).
    Save,
    Reset,
    Statistics,
}

/// Text the inspector keeps up to date.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorText {
    Name,
    Source,
    Step,
}

/// The inspector region.
pub fn inspector(theme: &Theme, layout: &ShellLayout, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
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
                        InspectorButton::Pin,
                    ));
                    h.spawn((
                        icon_button(
                            &t,
                            IconId::ChevronRight,
                            small,
                            Tooltip::new("Collapse inspector").binding(&map, "ui.inspector"),
                        ),
                        InspectorButton::Collapse,
                    ));
                });
            p.spawn(Node {
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::clip(),
                ..column(0.0)
            })
            .with_children(|body| world_inspector(body, &t));
            p.spawn(side_splitter(&t, Side::Inspector));
        })),
    )
}

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
                border_radius: rounded(crate::theme::radius::XS),
                ..default()
            },
            Fill(ColorRole::EntityBadgeBg),
            children![text(TAG_TYPE, ColorRole::Text2, "WORLD")],
        ));
        r.spawn((
            text(TypeStyle::ENTITY_NAME, ColorRole::Text, "No world"),
            InspectorText::Name,
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
        InspectorText::Source,
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
            ("Save", InspectorButton::Save, false),
            ("Reset", InspectorButton::Reset, true),
            ("Statistics", InspectorButton::Statistics, false),
        ] {
            let mut e = r.spawn((button(t, label, ButtonVariant::Normal), which));
            e.insert(Node {
                height: px(t.s.button_h),
                flex_grow: 1.0,
                flex_basis: px(0),
                border: UiRect::all(px(border::HAIRLINE)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: rounded(crate::theme::radius::SM),
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
            ("Step size", Some(InspectorText::Step)),
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
fn group(body: &mut ChildSpawner, title: &str, rows: &[(&str, Option<InspectorText>)]) {
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

/// Header and quick-action clicks.
pub fn on_inspector_button(
    ev: On<Activate>,
    buttons: Query<(&InspectorButton, Has<InteractionDisabled>, Has<Active>)>,
    mut out: MessageWriter<UiCommand>,
    mut sim: MessageWriter<GzCommand>,
    mut commands: Commands,
) {
    let Ok((which, disabled, active)) = buttons.get(ev.entity) else {
        return;
    };
    if disabled {
        return;
    }
    match which {
        InspectorButton::Collapse => {
            out.write(UiCommand::ToggleInspector);
        }
        InspectorButton::Pin => {
            if active {
                commands.entity(ev.entity).remove::<Active>();
            } else {
                commands.entity(ev.entity).insert(Active);
            }
        }
        InspectorButton::Reset => {
            sim.write(GzCommand::Reset);
        }
        InspectorButton::Save | InspectorButton::Statistics => {}
    }
}

/// The world's name, its source (the server) and the step size; Reset
/// needs a live world.
#[allow(clippy::type_complexity)]
pub fn inspector_texts(
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    phase: Res<CurrentPhase>,
    mut texts: Query<(&InspectorText, &mut Text, &mut crate::theme::Ink)>,
    buttons: Query<(Entity, &InspectorButton, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let status = status.as_deref();
    let name = world_name(status, index.as_deref());
    let step = status
        .and_then(|s| s.sim)
        .map(|s| s.step_size)
        .filter(|s| *s > 0.0);
    for (which, mut t, mut ink) in &mut texts {
        let (want, role) = match which {
            InspectorText::Name => match &name {
                Some(n) => (n.clone(), ColorRole::Text),
                None => ("No world".into(), ColorRole::Text3),
            },
            InspectorText::Source => (
                status.map_or_else(String::new, |s| format::address(&s.url)),
                ColorRole::Text3,
            ),
            InspectorText::Step => match step {
                Some(s) => (format::step(s), ColorRole::Text),
                None => ("—".into(), ColorRole::Text3),
            },
        };
        set_text(&mut t, &want);
        ink.set_if_neq(crate::theme::Ink(role));
    }
    let live = phase.0 == Phase::Live;
    for (e, which, disabled) in &buttons {
        if *which == InspectorButton::Reset && live == disabled {
            if live {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_inspector_button);
}
