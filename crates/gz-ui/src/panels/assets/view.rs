//! The Assets panel's body (spec §3.3): the tabs, then the tab's content
//! (shape and light tiles, the Fuel search and its results, a URI for a
//! local model), and the inserts on their way. Built from the panel's
//! resources whenever the shell spawns the body (DECISIONS.md D-58,
//! D-102); the Fuel rows, the inserts and every state follow in place
//! ([`systems`](super::systems)).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{Button as Headless, RadioButton, RadioGroup, ScrollArea};
use gz_core::fuel::{size_label, FuelModel, BROWSE_HOST};
use gz_core::insert::{Insertable, LightKind, Shape};

use super::model::{AssetTab, AssetsState};
use super::AssetsBody;
use crate::assets::fonts::text;
use crate::assets::icons::{content_icon, IconId};
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::button::Checkedness;
use crate::widgets::layout::{column, display, row};
use crate::widgets::{
    a11y, button, clickable, filter_field, icon_button, rounded, text_button, text_field,
    ButtonVariant, IconButtonSize, TextButtonProps, Tooltip,
};

/// A tile's height (§3.3: 64 px tiles).
pub(crate) const TILE_H: f32 = 64.0;
/// A Fuel row's thumbnail (§3.3: 44 px).
pub(crate) const THUMB: f32 = 44.0;

/// The hint under the tiles.
pub(crate) const PLACE_HINT: &str =
    "Click one, then click in the view to place it · Shift to place several · Esc cancels";

/// A tab.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TabChoice(pub(crate) AssetTab);

/// A tab's content.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TabPane(pub(crate) AssetTab);

/// A shape or light tile: what it inserts.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct Tile(pub(crate) Insertable);

/// Why nothing can be inserted, while that holds.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct AssetsNote;

/// The Fuel search field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FuelQuery;

/// The spinner in the Fuel search field: searching (§3.3.1).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FuelSpinner;

/// The spinner's side.
pub(crate) const SPINNER: f32 = 14.0;

/// What a Cancel says it does: it stops waiting, it can't take the
/// `create` back (D-121).
pub(crate) const CANCEL_DETAIL: &str = "Stop waiting for it: the server may still add it";

/// The Fuel tab's message: "Search Fuel…", "No models match", "Fuel is
/// unreachable".
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FuelMessage;

/// The Fuel tab's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FuelButton {
    Retry,
    More,
}

/// What shows or hides a Fuel button.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FuelButtonBox(pub(crate) FuelButton);

/// The Fuel results' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct FuelRows;

/// A result row's Insert, and the row: the model it inserts.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct FuelInsert(pub(crate) FuelModel);

/// A result row's bar, while its model is on its way.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct RowBar(pub(crate) String);

/// A result row's Cancel, in Insert's place while its model is on its
/// way: the model it stops waiting for.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct FuelCancel(pub(crate) FuelModel);

/// What shows or hides a result row's Insert (`false`) or Cancel
/// (`true`): its model's URI.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct RowButton(pub(crate) String, pub(crate) bool);

/// The Local tab's URI field and Insert.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct LocalUri;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct LocalInsert;

/// The inserts' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct InsertRows;

/// An insert row's dismiss button, by insert id.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dismiss(pub(crate) u32);

/// An insert row's Cancel, while it's under way: its insert's id and
/// label.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct CancelInsert(pub(crate) u32, pub(crate) String);

/// A moving 2 px `accent` segment: work under way (§3.3.1).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Indeterminate;

/// Builds the body from the panel's resources.
pub(super) fn build_assets(
    ev: On<Add<AssetsBody>>,
    theme: Res<Theme>,
    state: Res<AssetsState>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let state = state.clone();
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
            c.spawn((
                RadioGroup,
                a11y(Role::TabList, "Asset kind"),
                Node {
                    padding: UiRect::all(px(SPACE[4])),
                    column_gap: px(SPACE[2]),
                    flex_shrink: 0.0,
                    ..row(SPACE[2])
                },
            ))
            .with_children(|tabs| {
                for tab in AssetTab::ALL {
                    tabs.spawn((tab_button(&t, tab, tab == state.tab), TabChoice(tab)));
                }
            });
            c.spawn((
                AssetsNote,
                text(TypeStyle::SMALL, ColorRole::Text2, ""),
                Node {
                    margin: UiRect::new(px(SPACE[5]), px(SPACE[5]), px(0), px(SPACE[3])),
                    display: Display::None,
                    ..default()
                },
            ));
            let shapes = Shape::ALL.map(|s| (Insertable::Shape(s), shape_icon(s)));
            spawn_tiles(c, &t, AssetTab::Shapes, &state, &shapes);
            let lights = LightKind::ALL.map(|l| (Insertable::Light(l), light_icon(l)));
            spawn_tiles(c, &t, AssetTab::Lights, &state, &lights);
            spawn_fuel(c, &t, &state);
            spawn_local(c, &t, &state);
            super::parts::spawn_parts(c, &t, &state);
            c.spawn((
                InsertRows,
                AccessibilityNode(accesskit::Node::new(Role::List)),
                Node {
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(px(SPACE[4])),
                    ..column(SPACE[2])
                },
            ));
        });
    });
}

fn shape_icon(s: Shape) -> IconId {
    match s {
        Shape::Box => IconId::ShapeBox,
        Shape::Sphere => IconId::ShapeSphere,
        Shape::Cylinder => IconId::ShapeCylinder,
        Shape::Capsule => IconId::ShapeCapsule,
        Shape::Cone => IconId::ShapeCone,
        Shape::Plane => IconId::ShapePlane,
    }
}

fn light_icon(l: LightKind) -> IconId {
    match l {
        LightKind::Directional => IconId::LightDirectional,
        LightKind::Point => IconId::LightPoint,
        LightKind::Spot => IconId::LightSpot,
    }
}

/// A tab: a radio button, `bg` when chosen.
fn tab_button(theme: &Theme, tab: AssetTab, chosen: bool) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text2);
    visuals.active_fill = ColorRole::Bg;
    visuals.active_ink = ColorRole::Text;
    (
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(theme.s.input_h),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        RadioButton,
        clickable(visuals, Role::Tab, tab.label()),
        Checkedness(chosen),
        children![(
            text(TypeStyle::LABEL, ColorRole::Text2, tab.label()),
            Content
        )],
    )
}

/// A tab of tiles, three to a row, and the hint.
fn spawn_tiles(
    c: &mut ChildSpawnerCommands,
    t: &Theme,
    tab: AssetTab,
    state: &AssetsState,
    tiles: &[(Insertable, IconId)],
) {
    c.spawn((
        TabPane(tab),
        Node {
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[4])),
            display: display(state.tab == tab),
            ..column(SPACE[4])
        },
    ))
    .with_children(|p| {
        p.spawn(Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::flex(3, 1.0),
            column_gap: px(SPACE[3]),
            row_gap: px(SPACE[3]),
            ..default()
        })
        .with_children(|g| {
            for (what, icon) in tiles {
                let mut e = g.spawn((tile(t, what, *icon), Tile(what.clone())));
                if state.armed.as_ref() == Some(what) {
                    e.insert(crate::theme::visuals::Active);
                }
            }
        });
        p.spawn(text(TypeStyle::SMALL, ColorRole::Text2, PLACE_HINT));
    });
}

/// A 64 px tile: the icon over the label, `line` bordered; armed, it
/// shows active.
fn tile(theme: &Theme, what: &Insertable, icon: IconId) -> impl Bundle {
    let visuals = Visuals::new(ColorRole::Surface, ColorRole::Text).stroked(ColorRole::Line);
    let label = match what {
        Insertable::Light(l) => l.label().to_string(),
        _ => what.label(),
    };
    (
        Node {
            height: px(TILE_H),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::MD),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..column(SPACE[2])
        },
        Headless,
        clickable(visuals, Role::Button, format!("Insert {}", what.label())),
        Tooltip::new(format!("Insert {}", what.label())),
        children![
            content_icon(icon, theme.s.icon, ColorRole::Text),
            (text(TypeStyle::SMALL, ColorRole::Text, label), Content),
        ],
    )
}

/// The Fuel tab: its source, the search field and its bar, a message
/// (with Retry when Fuel is unreachable), the results and Load more.
fn spawn_fuel(c: &mut ChildSpawnerCommands, t: &Theme, state: &AssetsState) {
    c.spawn((
        TabPane(AssetTab::Fuel),
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: display(state.tab == AssetTab::Fuel),
            ..column(0.0)
        },
    ))
    .with_children(|p| {
        p.spawn(Node {
            padding: UiRect::new(px(SPACE[5]), px(SPACE[4]), px(SPACE[1]), px(SPACE[3])),
            align_items: AlignItems::Baseline,
            ..row(SPACE[3])
        })
        .with_children(|h| {
            h.spawn(text(TypeStyle::SECTION, ColorRole::Text2, "Fuel"));
            h.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text2, BROWSE_HOST));
        });
        p.spawn(Node {
            padding: UiRect::horizontal(px(SPACE[4])),
            flex_shrink: 0.0,
            ..column(0.0)
        })
        .with_children(|f| {
            f.spawn((
                filter_field(t, "Search Fuel", "Search models", state.query.clone()),
                FuelQuery,
            ))
            .with_child(spinner());
        });
        p.spawn(Node {
            padding: UiRect::all(px(SPACE[4])),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            ..column(SPACE[3])
        })
        .with_children(|m| {
            m.spawn((
                FuelMessage,
                text(TypeStyle::BODY, ColorRole::Text2, ""),
                Node {
                    display: Display::None,
                    ..default()
                },
            ));
            m.spawn((
                FuelButtonBox(FuelButton::Retry),
                Node {
                    display: Display::None,
                    ..default()
                },
            ))
            .with_child((button(t, "Retry", ButtonVariant::Normal), FuelButton::Retry));
        });
        p.spawn((
            ScrollArea,
            a11y(Role::List, "Fuel models"),
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                min_height: px(THUMB * 2.0),
                overflow: Overflow::scroll_y(),
                padding: UiRect::new(px(SPACE[4]), px(SPACE[4]), px(0), px(SPACE[4])),
                ..column(SPACE[3])
            },
        ))
        .with_children(|l| {
            l.spawn((
                FuelRows,
                AccessibilityNode(accesskit::Node::new(Role::GenericContainer)),
                Node {
                    flex_shrink: 0.0,
                    ..column(SPACE[3])
                },
            ));
            l.spawn((
                FuelButtonBox(FuelButton::More),
                Node {
                    display: Display::None,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_child((
                button(t, "Load more", ButtonVariant::Normal),
                FuelButton::More,
            ));
        });
    });
}

/// The search field's spinner (§3.3.1: "spinner in the input"): an
/// `accent` arc turning over a `line_soft` ring, at the field's right end
/// while a search is under way (`systems::fuel_states` shows it,
/// `systems::spin` turns it). To AccessKit, "Searching Fuel" while it
/// shows.
fn spinner() -> impl Bundle {
    let ring = |border: UiRect, role: ColorRole| {
        (
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                border,
                border_radius: rounded(SPINNER / 2.0),
                ..default()
            },
            Stroke(role),
            Pickable::IGNORE,
        )
    };
    let arc = UiRect {
        top: px(2),
        right: px(2),
        ..UiRect::ZERO
    };
    let mut access = a11y(Role::ProgressIndicator, "Searching Fuel");
    access.set_hidden();
    (
        FuelSpinner,
        access,
        Node {
            width: px(SPINNER),
            height: px(SPINNER),
            flex_shrink: 0.0,
            margin: UiRect::left(px(SPACE[3])),
            display: Display::None,
            ..default()
        },
        UiTransform::default(),
        Pickable::IGNORE,
        children![
            ring(UiRect::all(px(2)), ColorRole::LineSoft),
            ring(arc, ColorRole::Accent),
        ],
    )
}

/// A Fuel result (§3.3): its thumbnail, name, owner and size, and
/// Insert; while its model is on its way, a bar under it and Cancel in
/// Insert's place (§3.3.1). The server doesn't say how far a download
/// has got, so the bar moves without measuring (D-121).
pub(super) fn fuel_row(
    theme: &Theme,
    model: &FuelModel,
    thumb: Option<Handle<Image>>,
    loading: bool,
) -> impl Bundle {
    let detail = match model.filesize {
        0 => model.owner.clone(),
        n => format!("{} · {}", model.owner, size_label(n)),
    };
    let image = thumb.map(|h| {
        (
            ImageNode::new(h),
            // Its own aspect, fitted to the width.
            Node {
                width: px(THUMB),
                ..default()
            },
        )
    });
    (
        Node {
            padding: UiRect::all(px(SPACE[3])),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::MD),
            flex_shrink: 0.0,
            ..column(0.0)
        },
        Stroke(ColorRole::LineSoft),
        a11y(Role::ListItem, model.name.clone()),
        Hovered::default(),
        children![
            (
                Node {
                    align_items: AlignItems::Center,
                    ..row(SPACE[4])
                },
                children![
                    (
                        Node {
                            width: px(THUMB),
                            height: px(THUMB),
                            flex_shrink: 0.0,
                            border_radius: rounded(radius::SM),
                            overflow: Overflow::clip(),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Fill(ColorRole::Hover),
                        Children::spawn(SpawnIter(image.into_iter())),
                    ),
                    (
                        Node {
                            flex_grow: 1.0,
                            min_width: px(0),
                            overflow: Overflow::clip(),
                            ..column(SPACE[1])
                        },
                        children![
                            (
                                text(TypeStyle::BODY, ColorRole::Text, model.name.clone()),
                                TextLayout::no_wrap(),
                            ),
                            (
                                text(TypeStyle::MONO_SMALL, ColorRole::Text2, detail),
                                TextLayout::no_wrap(),
                            ),
                        ],
                    ),
                    (
                        RowButton(model.uri(), false),
                        Node {
                            display: display(!loading),
                            ..default()
                        },
                        children![(
                            button(theme, "Insert", ButtonVariant::Normal),
                            FuelInsert(model.clone()),
                        )],
                    ),
                    (
                        RowButton(model.uri(), true),
                        Node {
                            display: display(loading),
                            ..default()
                        },
                        children![(
                            button(theme, "Cancel", ButtonVariant::Normal),
                            FuelCancel(model.clone()),
                            Tooltip::new("Cancel").detail(CANCEL_DETAIL),
                        )],
                    ),
                ],
            ),
            (
                RowBar(model.uri()),
                Node {
                    height: px(2),
                    margin: UiRect::top(px(SPACE[3])),
                    overflow: Overflow::clip(),
                    display: display(loading),
                    ..default()
                },
                children![indeterminate()],
            ),
        ],
    )
}

/// The Local tab: a URI the server resolves, and Insert.
fn spawn_local(c: &mut ChildSpawnerCommands, t: &Theme, state: &AssetsState) {
    c.spawn((
        TabPane(AssetTab::Local),
        Node {
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[4])),
            display: display(state.tab == AssetTab::Local),
            ..column(SPACE[4])
        },
    ))
    .with_children(|p| {
        p.spawn(text(
            TypeStyle::SMALL,
            ColorRole::Text2,
            "A model the server can find: model://name, a Fuel URL, or a path on the server. \
             The Gazebo websocket server doesn't list its local models.",
        ));
        p.spawn(Node {
            align_items: AlignItems::Center,
            ..row(SPACE[3])
        })
        .with_children(|r| {
            r.spawn(Node {
                flex_grow: 1.0,
                min_width: px(0),
                ..column(0.0)
            })
            .with_child((text_field(t, "Model URI", state.uri.clone()), LocalUri));
            r.spawn((button(t, "Insert", ButtonVariant::Normal), LocalInsert));
        });
        p.spawn(text(TypeStyle::SMALL, ColorRole::Text2, PLACE_HINT));
    });
}

/// An insert's row: what, its status under it, and while under way a bar
/// and Cancel (D-121); once it failed, a dismiss button (`danger_soft`,
/// §3.3.1).
pub(super) fn insert_row(
    theme: &Theme,
    id: u32,
    label: String,
    status: String,
    failed: bool,
) -> impl Bundle {
    let (fill, ink) = if failed {
        (ColorRole::DangerSoft, ColorRole::Danger)
    } else {
        (ColorRole::Clear, ColorRole::Text2)
    };
    let cancel = (!failed).then(|| {
        (
            text_button(theme, TextButtonProps::new("Cancel")),
            CancelInsert(id, label.clone()),
            Tooltip::new("Cancel").detail(CANCEL_DETAIL),
        )
    });
    (
        Node {
            padding: UiRect::axes(px(SPACE[4]), px(SPACE[3])),
            border_radius: rounded(radius::SM),
            flex_shrink: 0.0,
            margin: UiRect::bottom(px(SPACE[2])),
            ..column(SPACE[2])
        },
        Fill(fill),
        a11y(Role::ListItem, format!("{label}: {status}")),
        children![
            (
                Node {
                    align_items: AlignItems::Center,
                    ..row(SPACE[3])
                },
                children![
                    // What and how it goes, over each other: beside each
                    // other they'd push the button out of a narrow panel.
                    (
                        Node {
                            flex_grow: 1.0,
                            min_width: px(0),
                            overflow: Overflow::clip(),
                            ..column(SPACE[1])
                        },
                        children![
                            (
                                text(TypeStyle::LABEL, ColorRole::Text, label),
                                TextLayout::no_wrap()
                            ),
                            text(TypeStyle::SMALL, ink, status),
                        ],
                    ),
                    (
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Children::spawn(SpawnIter(cancel.into_iter()))
                    ),
                    (
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Children::spawn(SpawnIter(
                            failed
                                .then(|| {
                                    (
                                        icon_button(
                                            theme,
                                            IconId::Close,
                                            IconButtonSize::Small,
                                            Tooltip::new("Dismiss"),
                                        ),
                                        Dismiss(id),
                                    )
                                })
                                .into_iter()
                        )),
                    ),
                ],
            ),
            (
                Node {
                    height: px(2),
                    overflow: Overflow::clip(),
                    display: display(!failed),
                    ..default()
                },
                children![indeterminate()],
            ),
        ],
    )
}

/// The moving segment of a 2 px bar.
fn indeterminate() -> impl Bundle {
    (
        Indeterminate,
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            bottom: px(0),
            width: percent(30),
            left: percent(0),
            border_radius: rounded(1.0),
            ..default()
        },
        Fill(ColorRole::Accent),
    )
}
