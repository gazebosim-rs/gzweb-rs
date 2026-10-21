//! What every inspector body shares (spec §3.5 items 2 to 6): the title
//! row (type tag, name, more actions), the breadcrumb, the quick actions,
//! the groups with their live dot, and the link rows; and the "(removed)"
//! state (§2 states) with its Dismiss.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::view::Selection;

use super::{InspectKind, Inspected};
use crate::assets::text;
use crate::assets::IconId;
use crate::theme::{
    border, radius, ColorRole, Fill, FontFamily, Ink, Opacity, Stroke, Theme, TypeStyle, SPACE,
};
use crate::widgets::layout::{self, column, display, flex_spacer, group_header, property_row, row};
use crate::widgets::text::set_text;
use crate::widgets::{button, icon_button, rounded, ButtonVariant, IconButtonSize, Tooltip};

/// The type tag's type: mono 11/500 (§3.5).
pub(crate) const TAG_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 500);
/// The breadcrumb's type: mono 11 `text_3`.
pub(crate) const CRUMB_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 11.0, 400);
/// The JOINT, LIGHT and SENSOR tags' fill: their colour at 12%.
pub(crate) const TINT_OPACITY: f32 = 0.12;
/// The live dot at a group header's right (§3.5: 6px `ok`).
pub(crate) const LIVE_DOT: f32 = 6.0;

/// A type tag (§3.5): its label, text colour, and fill (tinted: the fill
/// colour at [`TINT_OPACITY`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeTag {
    pub(crate) label: &'static str,
    pub(crate) ink: ColorRole,
    pub(crate) fill: ColorRole,
    pub(crate) tinted: bool,
}

impl TypeTag {
    pub(crate) fn of(kind: InspectKind) -> TypeTag {
        let tag = |label, ink, fill, tinted| TypeTag {
            label,
            ink,
            fill,
            tinted,
        };
        match kind {
            InspectKind::World => tag("WORLD", ColorRole::Text2, ColorRole::EntityBadgeBg, false),
            InspectKind::Model => tag("MODEL", ColorRole::AccentText, ColorRole::AccentSoft, false),
            InspectKind::Link => tag("LINK", ColorRole::Text2, ColorRole::EntityBadgeBg, false),
            InspectKind::Joint => tag(
                "JOINT",
                ColorRole::EntityJoint,
                ColorRole::EntityJoint,
                true,
            ),
            InspectKind::Light => tag(
                "LIGHT",
                ColorRole::EntityLight,
                ColorRole::EntityLight,
                true,
            ),
            InspectKind::Sensor => tag(
                "SENSOR",
                ColorRole::EntitySensor,
                ColorRole::EntitySensor,
                true,
            ),
            InspectKind::Multi => tag(
                "SELECTION",
                ColorRole::Text2,
                ColorRole::EntityBadgeBg,
                false,
            ),
        }
    }
}

/// The title row's name.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InspectorTitle(pub(crate) String);

/// The breadcrumb's text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct InspectorCrumb;

/// A quick action (disabled while the subject is removed).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct QuickAction;

/// The title row's Dismiss, shown while the subject is removed.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DismissButton;

/// Title row (padding 14 16 10): the type tag, the name, Dismiss (while
/// removed) and more actions.
pub(crate) fn title_row(t: &Theme, kind: InspectKind, name: &str) -> impl Bundle {
    let tag = TypeTag::of(kind);
    let t = t.clone();
    let name = name.to_string();
    (
        Node {
            padding: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                top: px(SPACE[7]),
                bottom: px(SPACE[2]),
            },
            flex_shrink: 0.0,
            ..row(SPACE[4])
        },
        Children::spawn(SpawnWith(move |r: &mut ChildSpawner| {
            r.spawn(type_tag(tag));
            r.spawn((
                text(TypeStyle::ENTITY_NAME, ColorRole::Text, name.clone()),
                InspectorTitle(name),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            r.spawn(flex_spacer());
            r.spawn((
                button(&t, "Dismiss", ButtonVariant::Normal),
                DismissButton,
                Node {
                    display: Display::None,
                    height: px(t.s.button_h),
                    padding: UiRect::horizontal(px(SPACE[4])),
                    border: UiRect::all(px(border::HAIRLINE)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::SM),
                    ..default()
                },
            ));
            r.spawn(icon_button(
                &t,
                IconId::More,
                IconButtonSize::Small,
                Tooltip::new("More actions"),
            ));
        })),
    )
}

/// The type tag: mono 11/500 on its fill, radius 4, padding 2 6.
fn type_tag(tag: TypeTag) -> impl Bundle {
    let node = Node {
        padding: UiRect::axes(px(SPACE[3]), px(SPACE[1])),
        flex_shrink: 0.0,
        border_radius: rounded(radius::XS),
        ..default()
    };
    let fill = (
        Fill(tag.fill),
        Opacity(if tag.tinted { TINT_OPACITY } else { 1.0 }),
        Node {
            border_radius: rounded(radius::XS),
            ..layout::overlay()
        },
    );
    (node, children![fill, text(TAG_TYPE, tag.ink, tag.label)])
}

/// The breadcrumb (mono 11 `text_3`).
pub(crate) fn breadcrumb(crumb: impl Into<String>) -> impl Bundle {
    (
        text(CRUMB_TYPE, ColorRole::Text3, crumb.into()),
        InspectorCrumb,
        Node {
            margin: UiRect {
                left: px(SPACE[8]),
                right: px(SPACE[8]),
                bottom: px(SPACE[6]),
                ..default()
            },
            ..default()
        },
    )
}

/// The quick actions' row; spawn three [`quick_action`]s in it.
pub(crate) fn quick_actions() -> impl Bundle {
    (
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
    )
}

/// One of three equal quick actions (never destructive, Q3); `unavailable`
/// disables it with that reason.
pub(crate) fn quick_action(t: &Theme, label: &str, unavailable: Option<&str>) -> impl Bundle {
    let label = label.to_string();
    let tooltip = match unavailable {
        Some(why) => Tooltip::new(label.clone()).detail(why.to_string()),
        None => Tooltip::new(label.clone()),
    };
    (
        button(t, label.clone(), ButtonVariant::Normal),
        QuickAction,
        Node {
            height: px(t.s.button_h),
            flex_grow: 1.0,
            flex_basis: px(0),
            border: UiRect::all(px(border::HAIRLINE)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Disabled(unavailable.is_some()),
        tooltip,
    )
}

/// A quick action that is unavailable whatever the subject's state.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Disabled(pub(crate) bool);

/// A group's heading with the live dot (§3.5: fields that stream from the
/// sim).
pub(crate) fn live_header(title: &str) -> impl Bundle {
    (
        row(SPACE[3]),
        children![
            text(TypeStyle::HEADING, ColorRole::Text, title.to_string()),
            flex_spacer(),
            (
                Node {
                    width: px(LIVE_DOT),
                    height: px(LIVE_DOT),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                Fill(ColorRole::Ok),
                Tooltip::new("Live").detail("Streams from the simulation"),
            )
        ],
    )
}

/// A group of rows of label and value, the values "—" until filled, each
/// carrying `marker(i)`.
pub(crate) fn group<M: Component>(
    body: &mut ChildSpawner,
    title: &str,
    live: bool,
    rows: &[&str],
    marker: impl Fn(usize) -> M,
) {
    body.spawn(layout::group()).with_children(|g| {
        if live {
            g.spawn(live_header(title));
        } else {
            g.spawn(group_header(title, None));
        }
        for (i, label) in rows.iter().enumerate() {
            let value = text(TypeStyle::MONO_SMALL, ColorRole::Text, "—");
            g.spawn(property_row(*label, (value, marker(i))));
        }
    });
}

/// The link rows at the bottom (§3.5 item 6): each opens a sub-view, the
/// last is All components. None opens yet: they show what they will be.
pub(crate) fn links(body: &mut ChildSpawner, names: &[&str]) {
    body.spawn(Node {
        padding: UiRect::axes(px(SPACE[8]), px(SPACE[4])),
        flex_shrink: 0.0,
        ..column(0.0)
    })
    .with_children(|links| {
        for name in names {
            links
                .spawn((
                    Node {
                        height: px(36),
                        flex_shrink: 0.0,
                        ..row(0.0)
                    },
                    Tooltip::new(*name).detail("Not available yet"),
                ))
                .with_children(|row| {
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text3, name.to_string()));
                    row.spawn(flex_spacer());
                    row.spawn(text(TypeStyle::ROW, ColorRole::Text3, "›"));
                });
        }
    });
}

/// While the subject is removed: the name in `text_3` with "(removed)",
/// the quick actions disabled, Dismiss shown (§2 states).
pub(crate) fn removed_state(
    inspected: Res<Inspected>,
    added: Query<(), Added<InspectorTitle>>,
    mut titles: Query<(&InspectorTitle, &mut Text, &mut Ink)>,
    mut dismiss: Query<&mut Node, With<DismissButton>>,
    actions: Query<(Entity, &Disabled, Has<InteractionDisabled>), With<QuickAction>>,
    mut commands: Commands,
) {
    if !inspected.is_changed() && added.is_empty() {
        return;
    }
    let removed = inspected.removed;
    for (title, mut text, mut ink) in &mut titles {
        let want = if removed {
            format!("{} (removed)", title.0)
        } else {
            title.0.clone()
        };
        set_text(&mut text, &want);
        ink.set_if_neq(Ink(if removed {
            ColorRole::Text3
        } else {
            ColorRole::Text
        }));
    }
    for mut node in &mut dismiss {
        let want = display(removed);
        if node.display != want {
            node.display = want;
        }
    }
    for (e, disabled, has) in &actions {
        let want = removed || disabled.0;
        if want != has {
            if want {
                commands.entity(e).insert(InteractionDisabled);
            } else {
                commands.entity(e).remove::<InteractionDisabled>();
            }
        }
    }
}

/// Dismiss: back to the World inspector.
pub(crate) fn on_dismiss(
    ev: On<Activate>,
    buttons: Query<(), With<DismissButton>>,
    mut inspected: ResMut<Inspected>,
    mut selection: ResMut<Selection>,
) {
    if buttons.contains(ev.entity) {
        *inspected = Inspected::default();
        if !selection.entities.is_empty() {
            selection.entities.clear();
        }
    }
}
