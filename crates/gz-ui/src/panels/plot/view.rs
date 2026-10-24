//! The Plot panel's ready body (spec §3.3): the time window chips, the
//! field picker while it's open, and the charts, built from the panel's
//! resources whenever the shell spawns the body (DECISIONS.md D-58,
//! D-100). The charts, the picker's rows and every live value follow in
//! place ([`systems`](super::systems)).

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::asset::RenderAssetUsages;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Button as Headless, RadioGroup, ScrollArea};
use gz_core::fields::FieldPath;

use super::model::{Chart, Picker, PlotData, PlotState, Series, Target, TimeWindow, COLORS};
use super::systems::ChartFrame;
use super::PlotBody;
use crate::assets::fonts::text;
use crate::assets::icons::{icon, IconId};
use crate::theme::visuals::{Content, OwnInk, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{column, display, flex_spacer, row};
use crate::widgets::{
    a11y, choice_chip, filter_field, focus, icon_button, rounded, select, IconButtonSize, Tooltip,
};

/// A chart's plot height, in logical pixels.
pub(crate) const PLOT_H: f32 = 120.0;
/// The y axis labels' column, at least; wider labels widen it.
pub(crate) const GUTTER_W: f32 = 36.0;
/// An axis label's character width (Geist Mono 10).
pub(crate) const AXIS_CHAR_W: f32 = 6.0;
/// A series line's width, in logical pixels (§3.3: 2 px).
pub(crate) const LINE_W: f32 = 2.0;

/// A time window chip.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WindowChip(pub(crate) TimeWindow);

/// The charts' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct ChartList;

/// A chart, by id.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChartView(pub(crate) u32);

/// A series' legend row, by series id.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LegendRow(pub(crate) u32);

/// A legend row's parts that follow its series in place.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LegendPart {
    /// Its colour.
    Swatch(u32),
    /// ⚠, while its field no longer exists.
    Warn(u32),
    Label(u32),
    Value(u32),
}

/// A legend row's remove button, shown over the value while the row is
/// hovered or the button focused.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LegendRemove(pub(crate) u32);

/// A chart's plot: its image, drawn at its physical size.
#[derive(Component, Clone, Debug, PartialEq)]
pub(super) struct PlotArea {
    pub chart: u32,
    pub image: Handle<Image>,
}

/// Where the pointer is over a plot, from 0 (left) to 1 (right).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ChartHover {
    pub x: Option<f32>,
    /// The cursor reading it was last set from.
    pub(crate) from: Option<Vec2>,
}

/// A chart's y axis labels' parent, by chart id.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct YAxis(pub(crate) u32);

/// A chart's x axis labels' row, under the plot (past the y labels).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct XRow(pub(crate) u32);

/// A chart's x axis labels: how far back it starts, and "now".
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum XLabel {
    Start(u32),
    End(u32),
}

/// A chart's crosshair, by chart id.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Crosshair(pub(crate) u32);

/// A series' dot where the crosshair meets its line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HoverDot(pub(crate) u32);

/// A chart's crosshair readout: the time and each series' value.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HoverTip(pub(crate) u32);

/// The field picker's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PickerRoot;

/// The picker's filter field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PickerFilter;

/// The picker's rows' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PickerRows;

/// A picker row: a topic to list the fields of, or a field to add.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) enum PickRow {
    Topic(String),
    Field(FieldPath),
}

/// A picker row's detail: a field row's live value (its path), or a
/// topic's type.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct PickValue(pub(crate) Option<FieldPath>);

/// "Waiting for first message…", "No numbers in this message" and the
/// like, under the picker's filter.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PickerStatus;

/// The picker header's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickerButton {
    /// Back to the topics.
    Back,
    Close,
}

/// The picker's "Add to" select: its options' targets, in order.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct PickerTarget(pub(crate) Vec<Target>);

/// What Export CSV said.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct PlotNotice;

/// Builds the ready body's content from the panel's resources.
pub(super) fn build_plot(
    ev: On<Add<PlotBody>>,
    theme: Res<Theme>,
    state: Res<PlotState>,
    data: Res<PlotData>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let window = state.window;
    let picker = state.picker.clone();
    let charts = data.charts.clone();
    let mut handles: Vec<Handle<Image>> = charts.iter().map(|_| images.add(blank())).collect();
    commands.entity(ev.entity).with_children(|b| {
        b.spawn(Node {
            flex_grow: 1.0,
            min_height: px(0),
            ..column(0.0)
        })
        .with_children(|c| {
            c.spawn((
                RadioGroup,
                a11y(Role::RadioGroup, "Time window"),
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(SPACE[2]),
                    row_gap: px(SPACE[2]),
                    padding: UiRect {
                        left: px(SPACE[4]),
                        right: px(SPACE[4]),
                        top: px(SPACE[4]),
                        bottom: px(SPACE[2]),
                    },
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|chips| {
                for w in TimeWindow::ALL {
                    chips.spawn((choice_chip(w.label(), w == window), WindowChip(w)));
                }
            });
            let mut root = c.spawn((
                PickerRoot,
                Node {
                    flex_shrink: 0.0,
                    display: display(picker.is_some()),
                    border: UiRect::vertical(px(border::HAIRLINE)),
                    ..column(0.0)
                },
                Stroke(ColorRole::Line),
            ));
            if let Some(p) = &picker {
                root.with_children(|r| spawn_picker(r, &t, p, &charts));
            }
            c.spawn((
                ChartList,
                ScrollArea,
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_height: px(0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::horizontal(px(SPACE[4])),
                    ..column(0.0)
                },
            ))
            .with_children(|list| {
                for chart in &charts {
                    spawn_chart(list, &t, chart, handles.remove(0));
                }
            });
            c.spawn((
                PlotNotice,
                text(TypeStyle::SMALL, ColorRole::Text2, ""),
                Node {
                    margin: UiRect::all(px(SPACE[4])),
                    display: Display::None,
                    ..default()
                },
            ));
        });
    });
}

/// A 1×1 transparent image, until a chart is drawn at its size.
pub(super) fn blank() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// A chart (§3.3): its title and unit, a legend row per series, the plot
/// with its y labels at the left, and the x labels under it.
pub(super) fn spawn_chart(
    list: &mut ChildSpawnerCommands,
    t: &Theme,
    chart: &Chart,
    image: Handle<Image>,
) {
    let id = chart.id;
    list.spawn((
        ChartView(id),
        Node {
            padding: UiRect::vertical(px(SPACE[4])),
            border: UiRect::bottom(px(border::HAIRLINE)),
            flex_shrink: 0.0,
            ..column(SPACE[2])
        },
        Stroke(ColorRole::LineSoft),
    ))
    .with_children(|c| {
        c.spawn(Node {
            align_items: AlignItems::Baseline,
            ..row(SPACE[3])
        })
        .with_children(|h| {
            h.spawn((
                text(TypeStyle::HEADING, ColorRole::Text, chart.title.clone()),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            h.spawn(flex_spacer());
            h.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text2, chart.unit));
        });
        for s in &chart.series {
            spawn_legend_row(c, t, s);
        }
        c.spawn(Node {
            height: px(PLOT_H),
            margin: UiRect::top(px(SPACE[3])),
            ..row(0.0)
        })
        .with_children(|p| {
            p.spawn((
                YAxis(id),
                Node {
                    width: px(GUTTER_W),
                    flex_shrink: 0.0,
                    align_self: AlignSelf::Stretch,
                    ..default()
                },
            ));
            p.spawn((
                PlotArea {
                    chart: id,
                    image: image.clone(),
                },
                ImageNode::new(image),
                RelativeCursorPosition::default(),
                ChartHover::default(),
                ChartFrame::default(),
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    align_self: AlignSelf::Stretch,
                    ..default()
                },
            ))
            .with_children(|area| {
                area.spawn((
                    Crosshair(id),
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(0),
                        bottom: px(0),
                        width: px(border::HAIRLINE),
                        display: Display::None,
                        ..default()
                    },
                    Fill(ColorRole::Text3),
                    Pickable::IGNORE,
                ));
                for s in &chart.series {
                    area.spawn((
                        HoverDot(s.id),
                        Node {
                            position_type: PositionType::Absolute,
                            width: px(7),
                            height: px(7),
                            border: UiRect::all(px(1.5)),
                            border_radius: BorderRadius::MAX,
                            display: Display::None,
                            ..default()
                        },
                        Fill(COLORS[s.color]),
                        Stroke(ColorRole::Surface),
                        Pickable::IGNORE,
                    ));
                }
                area.spawn((
                    HoverTip(id),
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(SPACE[2]),
                        padding: UiRect::axes(px(SPACE[3]), px(SPACE[2])),
                        border: UiRect::all(px(border::HAIRLINE)),
                        border_radius: rounded(radius::SM),
                        display: Display::None,
                        ..column(SPACE[1])
                    },
                    Fill(ColorRole::Surface),
                    Stroke(ColorRole::Line),
                    GlobalZIndex(5),
                    Pickable::IGNORE,
                ));
            });
        });
        c.spawn((
            XRow(id),
            Node {
                margin: UiRect::left(px(GUTTER_W)),
                ..row(0.0)
            },
        ))
        .with_children(|x| {
            x.spawn((
                XLabel::Start(id),
                text(TypeStyle::TINY, ColorRole::Text2, ""),
            ));
            x.spawn(flex_spacer());
            x.spawn((
                XLabel::End(id),
                text(TypeStyle::TINY, ColorRole::Text2, "now"),
            ));
        });
    });
}

/// A series' legend row: its colour (⚠ in `danger` once its field no
/// longer exists), its label and its live value, and a remove button.
fn spawn_legend_row(c: &mut ChildSpawnerCommands, t: &Theme, s: &Series) {
    let id = s.id;
    let label = s.label();
    c.spawn((
        LegendRow(id),
        Hovered::default(),
        a11y(Role::Row, label.clone()),
        Node {
            min_height: px(20),
            align_items: AlignItems::Center,
            ..row(SPACE[3])
        },
    ))
    .with_children(|r| {
        r.spawn((
            LegendPart::Swatch(id),
            Node {
                width: px(12),
                height: px(3),
                flex_shrink: 0.0,
                border_radius: rounded(1.5),
                ..default()
            },
            Fill(COLORS[s.color]),
        ));
        r.spawn((
            LegendPart::Warn(id),
            Node {
                display: Display::None,
                ..default()
            },
            children![icon(IconId::Warning, 12.0, ColorRole::Danger)],
        ));
        r.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_child((
            LegendPart::Label(id),
            text(TypeStyle::BODY, ColorRole::Text, label),
            TextLayout::no_wrap(),
        ));
        r.spawn((
            LegendPart::Value(id),
            text(TypeStyle::MONO_SMALL, ColorRole::Text, "—"),
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        ));
        // Over the value, which it hides while it shows.
        r.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                ..default()
            },
            children![(
                icon_button(
                    t,
                    IconId::Close,
                    IconButtonSize::Small,
                    Tooltip::new("Remove series"),
                ),
                LegendRemove(id),
                Visibility::Hidden,
            )],
        ));
    });
}

/// The picker (Add field…): a header (Back while it lists a topic's
/// fields, the topic or "Add field", Close), where the field goes once
/// there are charts, the filter, a status line and the rows (filled by
/// `sync_picker_rows`).
pub(super) fn spawn_picker(r: &mut ChildSpawnerCommands, t: &Theme, p: &Picker, charts: &[Chart]) {
    r.spawn(Node {
        padding: UiRect::new(px(SPACE[4]), px(SPACE[3]), px(SPACE[3]), px(0)),
        align_items: AlignItems::Center,
        ..row(SPACE[2])
    })
    .with_children(|h| {
        if p.topic.is_some() {
            h.spawn((
                icon_button(
                    t,
                    IconId::ChevronLeft,
                    IconButtonSize::Small,
                    Tooltip::new("Back to topics"),
                ),
                PickerButton::Back,
            ));
        }
        let (style, title) = match &p.topic {
            Some(topic) => (TypeStyle::MONO_SMALL, topic.clone()),
            None => (TypeStyle::HEADING, "Add field".to_string()),
        };
        h.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_child((text(style, ColorRole::Text, title), TextLayout::no_wrap()));
        h.spawn((
            icon_button(
                t,
                IconId::Close,
                IconButtonSize::Small,
                Tooltip::new("Close"),
            ),
            PickerButton::Close,
        ));
    });
    if !charts.is_empty() {
        let mut targets = vec![Target::New];
        let mut options = vec!["New chart".to_string()];
        for c in charts.iter().filter(|c| !c.is_full()) {
            targets.push(Target::Chart(c.id));
            options.push(c.title.clone());
        }
        let selected = targets.iter().position(|x| *x == p.target).unwrap_or(0);
        r.spawn(Node {
            padding: UiRect::new(px(SPACE[4]), px(SPACE[4]), px(SPACE[3]), px(0)),
            align_items: AlignItems::Center,
            ..row(SPACE[3])
        })
        .with_children(|row| {
            row.spawn(text(TypeStyle::SMALL, ColorRole::Text2, "Add to"));
            row.spawn((
                select(t, "Add to chart", options, selected, 180.0),
                PickerTarget(targets),
            ));
        });
    }
    let placeholder = if p.topic.is_some() {
        "Filter fields"
    } else {
        "Filter topics"
    };
    r.spawn(Node {
        padding: UiRect::new(px(SPACE[4]), px(SPACE[4]), px(SPACE[3]), px(0)),
        ..row(0.0)
    })
    .with_child((
        filter_field(t, placeholder, "Filter", p.filter.clone()),
        PickerFilter,
    ));
    r.spawn((
        PickerStatus,
        text(TypeStyle::SMALL, ColorRole::Text3, ""),
        Node {
            margin: UiRect::new(px(SPACE[5]), px(SPACE[4]), px(SPACE[3]), px(0)),
            display: Display::None,
            ..default()
        },
    ));
    r.spawn((
        ScrollArea,
        a11y(Role::ListBox, placeholder.trim_start_matches("Filter ")),
        Node {
            max_height: px(t.s.tree_row_h * 7.0),
            overflow: Overflow::scroll_y(),
            padding: UiRect::all(px(SPACE[3])),
            ..column(0.0)
        },
    ))
    .with_child((
        PickerRows,
        AccessibilityNode(accesskit::Node::new(Role::GenericContainer)),
        Node {
            flex_shrink: 0.0,
            ..column(0.0)
        },
    ));
}

/// A picker row: `label` in mono, clipped at the edge, and `detail` in
/// `text_2` at the right (a topic's type, a field's value).
pub(super) fn pick_row(theme: &Theme, row: PickRow, label: String, detail: String) -> impl Bundle {
    let mut visuals = Visuals::new(ColorRole::Clear, ColorRole::Text);
    visuals.active_ink = ColorRole::Text;
    let value = match &row {
        PickRow::Field(p) => PickValue(Some(p.clone())),
        PickRow::Topic(_) => PickValue(None),
    };
    (
        Node {
            height: px(theme.s.tree_row_h),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(SPACE[3])),
            column_gap: px(SPACE[3]),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Headless,
        Hovered::default(),
        focus::PressFocusless,
        visuals,
        Fill(visuals.fill),
        a11y(Role::ListBoxOption, label.clone()),
        row,
        children![
            (
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                children![(
                    text(TypeStyle::MONO_SMALL, ColorRole::Text, label),
                    TextLayout::no_wrap(),
                    Content,
                )],
            ),
            (
                text(TypeStyle::MONO_SMALL, ColorRole::Text2, detail),
                TextLayout::no_wrap(),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                Content,
                OwnInk,
                value,
            ),
        ],
    )
}

/// A crosshair readout line: a series' colour and its value (text never
/// takes the series colour).
pub(super) fn tip_line(color: Option<ColorRole>, label: String) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            ..row(SPACE[2])
        },
        children![
            (
                Node {
                    width: px(8),
                    height: px(3),
                    border_radius: rounded(1.5),
                    display: display(color.is_some()),
                    ..default()
                },
                Fill(color.unwrap_or(ColorRole::Clear)),
            ),
            (
                text(TypeStyle::TINY, ColorRole::Text, label),
                TextLayout::no_wrap()
            ),
        ],
    )
}

/// A y axis label at `top` (logical pixels from the plot's top).
pub(super) fn y_label(label: String, top: f32) -> impl Bundle {
    (
        text(TypeStyle::TINY, ColorRole::Text2, label),
        TextLayout::no_wrap(),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(top - 7.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}
