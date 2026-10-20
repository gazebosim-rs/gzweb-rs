//! The widget gallery (`gzweb --ui gallery`, `?ui=gallery`): every widget,
//! and every state of the clickable ones, in one theme at a time, laid out
//! after board 06 (`design/ui/assets/boards/06-theme-tokens-light-vs-dark.png`).
//! The left column is the board's component strip and a states grid; the
//! right one the board's token list, then the remaining widgets. T (or the
//! button at the top right) switches light ↔ dark at runtime. No server
//! connection is involved.
//!
//! With [`GalleryCapture`] it saves a screenshot once the page has settled
//! and exits (`--ui gallery --screenshot out.png`).

use std::path::PathBuf;

use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use bevy::text::Underline;
use bevy::ui_widgets::Activate;
use gz_bevy::view::input::InputMap;

use crate::assets::brand::logo;
use crate::assets::fonts::{span, text};
use crate::assets::icons::{icon, IconId};
use crate::theme::visuals::{Active, ForceState, WidgetState};
use crate::theme::{
    border, radius, ColorRole, Fill, FocusRing, FontFamily, Ring, Stroke, Theme, TypeStyle, SPACE,
};
use crate::widgets::button::{text_button_parts, TextButton};
use crate::widgets::chip::dot;
use crate::widgets::focus::TextFocus;
use crate::widgets::layout::flex_spacer;
use crate::widgets::layout::{column, row, separator, Orientation};
use crate::widgets::menu::{menu, menu_group_label, MenuSide};
use crate::widgets::number_field::NumberInput;
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::splitter::SplitAxis;
use crate::widgets::text::set_text;
use crate::widgets::{
    activity_button, badge, button, filter_chip, icon_button, menu_checkbox, menu_item, menu_panel,
    menu_separator, number_field, primary_button, rounded, section_header, segmented_group, select,
    slider_row, splitter, status_chip, text_button, toggle_row, tool_button, tooltip_card,
    tree_row, AxisTag, BadgeKind, ButtonVariant, EntityKind, IconButtonSize, NumberField,
    SliderSpec, Splitter, StatusKind, Tooltip, TooltipSide, TreeRowSpec, Unit,
};
use crate::GzUiSystems;

/// The gallery's window size (the spec's screenshot size, §12).
pub const SIZE: (u32, u32) = (1440, 900);

/// Adds the gallery screen (and a 2D camera for it).
pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_gallery)
            .add_systems(
                Update,
                (toggle_theme_key, gallery_texts, focus_example).in_set(GzUiSystems::Behaviour),
            )
            .add_systems(Last, capture_gallery)
            .add_observer(on_theme_button);
    }
}

/// Save a screenshot here once the gallery has settled, then exit.
#[derive(Resource, Clone, Debug)]
pub struct GalleryCapture {
    pub path: PathBuf,
    /// Frames to wait (fonts, atlases, layout and the glyph cache).
    pub frames: u32,
}

impl GalleryCapture {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            frames: 30,
        }
    }
}

/// Text that names the current theme ("Light" / "Dark").
#[derive(Component, Clone, Copy, Debug)]
pub enum ThemeText {
    Name,
    Caption,
}

/// A token's hex value, shown next to its swatch.
#[derive(Component, Clone, Copy, Debug)]
pub struct TokenHex(pub ColorRole);

/// The button that switches theme.
#[derive(Component, Clone, Copy, Debug)]
pub struct ThemeToggle;

/// The number field the gallery focuses to show the focus look.
#[derive(Component, Clone, Copy, Debug)]
pub struct FocusExample;

/// `#RRGGBB` (or `#RRGGBBAA`) for a colour.
pub fn hex(color: Color) -> String {
    let c = color.to_srgba();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if c.alpha < 1.0 {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            b(c.red),
            b(c.green),
            b(c.blue),
            b(c.alpha)
        )
    } else {
        format!("#{:02X}{:02X}{:02X}", b(c.red), b(c.green), b(c.blue))
    }
}

/// The board's token list: role, name, what it's for.
pub const TOKENS: [(ColorRole, &str, &str); 20] = [
    (ColorRole::Bg, "bg", "app ground, activity bar"),
    (
        ColorRole::Surface,
        "surface",
        "bars, sidebar, inspector, menus",
    ),
    (ColorRole::Field, "field", "inputs"),
    (ColorRole::Hover, "hover", "hovered buttons and rows"),
    (ColorRole::Line, "line", "borders, dividers"),
    (ColorRole::Text, "text", "primary text"),
    (ColorRole::Text2, "text-2", "secondary text, icons"),
    (ColorRole::Text3, "text-3", "hints, units, placeholders"),
    (ColorRole::Accent, "accent", "selection, focus, on states"),
    (
        ColorRole::AccentText,
        "accent-text",
        "accent on text and icons",
    ),
    (ColorRole::AccentSoft, "accent-soft", "active button fill"),
    (ColorRole::AccentRing, "accent-ring", "active button ring"),
    (ColorRole::Primary, "primary", "play / pause button"),
    (ColorRole::Warn, "warn", "unsaved dot, warnings"),
    (ColorRole::WarnSoft, "warn-soft", "warning chip fill"),
    (ColorRole::Danger, "danger", "delete, errors"),
    (ColorRole::Ok, "ok", "connected, RTF healthy"),
    (ColorRole::AxisX, "axis-x / y / z", "gizmo and pose labels"),
    (
        ColorRole::ViewportSky,
        "viewport-sky",
        "default scene background",
    ),
    (
        ColorRole::ViewportGrid,
        "viewport-grid",
        "ground grid lines",
    ),
];

const fn state(hovered: bool, pressed: bool, active: bool, disabled: bool) -> WidgetState {
    WidgetState {
        hovered,
        pressed,
        active,
        disabled,
    }
}

/// The states shown in the states grid (plus a Focus column).
pub const STATES: [(&str, WidgetState); 5] = [
    ("Default", state(false, false, false, false)),
    ("Hover", state(true, false, false, false)),
    ("Pressed", state(true, true, false, false)),
    ("Active", state(false, false, true, false)),
    ("Disabled", state(false, false, false, true)),
];

const COLUMN_W: f32 = 674.0;
const NAME_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 14.0, 500);
const DESC_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 14.0, 400);
const HEX_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 400);
const TITLE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 22.0, 600);

fn card_node(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..node
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
    )
}

/// A column of the page, `width` wide.
fn page_column(width: f32, gap: f32) -> Node {
    Node {
        width: px(width),
        flex_shrink: 0.0,
        ..column(gap)
    }
}

/// A row of samples that keeps its size.
fn sample_row(gap: f32) -> Node {
    Node {
        flex_shrink: 0.0,
        ..row(gap)
    }
}

/// Spawns the gallery (and a UI camera).
pub(crate) fn spawn_gallery(mut commands: Commands, theme: Res<Theme>, map: Option<Res<InputMap>>) {
    let map = map.map_or_else(InputMap::builtin, |m| m.clone());
    commands.spawn((Camera2d, IsDefaultUiCamera));
    commands
        .spawn((
            GalleryRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::axes(px(SPACE[10]), px(SPACE[9])),
                column_gap: px(SPACE[10]),
                overflow: Overflow::clip(),
                ..default()
            },
            Fill(ColorRole::Bg),
        ))
        .with_children(|p| {
            p.spawn(page_column(COLUMN_W, SPACE[8]))
                .with_children(|c| left_column(c, &theme, &map));
            p.spawn(page_column(COLUMN_W + 20.0, SPACE[8]))
                .with_children(|c| right_column(c, &theme, &map));
        });
}

/// The gallery's root node.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GalleryRoot;

fn header(c: &mut ChildSpawnerCommands) {
    c.spawn(Node {
        height: px(32),
        align_items: AlignItems::Center,
        column_gap: px(SPACE[6]),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|h| {
        h.spawn((ThemeText::Name, text(TITLE_TYPE, ColorRole::Text, "")));
        h.spawn(flex_spacer());
        h.spawn((
            ThemeText::Caption,
            text(TypeStyle::CAPS, ColorRole::Text2, ""),
        ));
    });
}

fn left_column(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    header(c);
    top_bar(c, theme, map);
    c.spawn(Node {
        column_gap: px(SPACE[8]),
        align_items: AlignItems::FlexStart,
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|r| {
        activity_card(r, theme, map);
        rail_card(r, theme, map);
        static_menu(r, theme, map);
        inspector_card(r, theme);
    });
    c.spawn(sample_row(SPACE[8])).with_children(|r| {
        r.spawn(status_chip(
            theme,
            StatusKind::Warning,
            "Mesh for crate_01 not found",
            None,
        ));
        r.spawn(tooltip_card(
            theme,
            &Tooltip::new("Workspace: Build")
                .detail("Click to switch")
                .shortcut("Alt+1–4"),
            false,
        ));
        r.spawn((
            text(TypeStyle::ROW, ColorRole::AccentText, "All components 27 ›"),
            Underline,
        ));
    });
    states_grid(c, theme);
}

fn top_bar(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    c.spawn(card_node(Node {
        height: px(theme.s.top_bar_h + 2.0 * border::HAIRLINE + SPACE[4]),
        padding: UiRect {
            left: px(SPACE[5]),
            right: px(SPACE[5]),
            ..default()
        },
        column_gap: px(SPACE[4]),
        align_items: AlignItems::Center,
        ..default()
    }))
    .with_children(|b| {
        b.spawn(logo(theme.s.logo));
        b.spawn(text_button(
            theme,
            TextButton::new("warehouse_demo")
                .dot(ColorRole::Warn)
                .chevron()
                .large(),
        ));
        b.spawn(separator(Orientation::Vertical, 18.0, SPACE[2]));
        b.spawn(segmented_group("Tools", true, false))
            .with_children(|g| {
                g.spawn(tool_button(
                    theme,
                    IconId::Navigate,
                    Tooltip::new("Navigate").binding(map, "tool.navigate"),
                    true,
                ));
                g.spawn(tool_button(
                    theme,
                    IconId::Plus,
                    Tooltip::new("Translate").binding(map, "tool.translate"),
                    false,
                ));
                g.spawn(tool_button(
                    theme,
                    IconId::Rotate,
                    Tooltip::new("Rotate").binding(map, "tool.rotate"),
                    false,
                ));
            });
        b.spawn(flex_spacer());
        b.spawn(primary_button(
            theme,
            IconId::Pause,
            Tooltip::new("Pause").binding(map, "sim.play_pause"),
        ));
        b.spawn(text(TypeStyle::CLOCK, ColorRole::Text, "00:02:14.380"));
        b.spawn(sample_row(SPACE[2] + 1.0)).with_children(|r| {
            r.spawn(dot(theme.s.status_dot, ColorRole::Ok));
            r.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text2, "RTF 98%"));
        });
    });
}

fn activity_card(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    r.spawn(card_node(Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(SPACE[3])),
        row_gap: px(SPACE[2]),
        ..default()
    }))
    .with_children(|a| {
        a.spawn((
            activity_button(
                theme,
                IconId::ActivityScene,
                Tooltip::new("Scene").binding(map, "panel.scene"),
                None,
            ),
            Active,
        ));
        a.spawn((
            activity_button(
                theme,
                IconId::ActivityAssets,
                Tooltip::new("Assets").binding(map, "panel.assets"),
                None,
            ),
            ForceState(STATES[1].1),
        ));
        a.spawn(activity_button(
            theme,
            IconId::ActivityPlot,
            Tooltip::new("Plot").binding(map, "panel.plot"),
            Some((BadgeKind::Count, 2)),
        ));
        a.spawn(activity_button(
            theme,
            IconId::ActivityConsole,
            Tooltip::new("Console").binding(map, "panel.console"),
            Some((BadgeKind::Warning, 2)),
        ));
    });
}

fn rail_card(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let tip = |id: &str, name: &str| Tooltip::new(name).binding(map, id).side(TooltipSide::Left);
    // The rail floats over a view: show it on a patch of `bg`.
    r.spawn((
        Node {
            padding: UiRect::all(px(SPACE[3])),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..default()
        },
        Fill(ColorRole::Hover),
    ))
    .with_children(|v| {
        v.spawn((
            Node {
                width: px(theme.s.rail_w),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(0), px(SPACE[2] - 1.0)),
                row_gap: px(SPACE[1]),
                border: UiRect::all(px(border::HAIRLINE)),
                border_radius: rounded(11.0),
                ..default()
            },
            Fill(ColorRole::Hud),
            Stroke(ColorRole::Line),
        ))
        .with_children(|rail| {
            rail.spawn((
                icon_button(
                    theme,
                    IconId::CamOrbit,
                    IconButtonSize::Rail,
                    tip("", "Orbit"),
                ),
                Active,
            ));
            rail.spawn(icon_button(
                theme,
                IconId::CamPan,
                IconButtonSize::Rail,
                tip("", "Pan"),
            ));
            rail.spawn(separator(Orientation::Horizontal, 22.0, 3.0));
            rail.spawn((
                icon_button(
                    theme,
                    IconId::CameraMode,
                    IconButtonSize::Rail,
                    tip("view.follow", "Camera: Follow amr_01"),
                ),
                Active,
            ))
            .with_children(|b| {
                let d = theme.s.status_dot;
                b.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(4),
                        right: px(4),
                        width: px(d),
                        height: px(d),
                        border_radius: rounded(d / 2.0),
                        ..default()
                    },
                    Fill(ColorRole::Accent),
                ));
            });
            rail.spawn(icon_button(
                theme,
                IconId::Overlays,
                IconButtonSize::Rail,
                tip("", "Overlays"),
            ));
        });
    });
}

fn static_menu(r: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let keys = |id: &str| shortcut_label(map, id);
    r.spawn(menu_panel(theme))
        .insert(Node {
            width: px(222),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(SPACE[3])),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::LG),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|m| {
            m.spawn((
                menu_item(theme, "Perspective", None, keys("view.projection")),
                Active,
            ));
            m.spawn((
                menu_item(theme, "Follow amr_01", None, keys("view.follow")),
                ForceState(STATES[1].1),
            ));
            m.spawn(menu_checkbox(theme, "Grid", true, keys("overlay.grid")));
            m.spawn(menu_checkbox(
                theme,
                "Collisions",
                false,
                keys("overlay.collisions"),
            ));
        });
}

fn inspector_card(r: &mut ChildSpawnerCommands, theme: &Theme) {
    r.spawn(card_node(Node {
        width: px(236),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(SPACE[7])),
        row_gap: px(SPACE[2]),
        ..default()
    }))
    .with_children(|i| {
        i.spawn(sample_row(SPACE[3])).with_children(|f| {
            f.spawn(number_field(
                theme,
                AxisTag::X,
                NumberField::new(2.4),
                false,
            ));
            f.spawn((
                number_field(theme, AxisTag::Y, NumberField::new(-0.75), false),
                FocusExample,
            ));
        });
        i.spawn(toggle_row(theme, "Gravity", true));
        i.spawn(toggle_row(theme, "Static", false));
        i.spawn(Node {
            margin: UiRect::top(px(SPACE[2])),
            column_gap: px(SPACE[3]),
            ..default()
        })
        .with_children(|b| {
            b.spawn(button(theme, "Duplicate", ButtonVariant::Normal));
            b.spawn(button(theme, "Delete", ButtonVariant::Destructive));
        });
    });
}

/// Draws a widget's focus ring permanently (the states grid's Focus column).
fn show_focus(commands: &mut Commands, e: Entity) {
    commands.entity(e).remove::<FocusRing>().insert((
        Ring(ColorRole::Accent),
        Outline::new(px(border::FOCUS_RING), px(2), Color::NONE),
    ));
}

/// One sample widget for a states-grid row.
#[derive(Clone, Copy, Debug)]
enum Sample {
    Icon32,
    Icon28,
    Rail,
    Text,
    Bordered,
    Destructive,
    Chip,
    Item,
    Activity,
}

impl Sample {
    const ALL: [(Sample, &'static str); 9] = [
        (Sample::Icon32, "IconButton 32"),
        (Sample::Icon28, "IconButton 28"),
        (Sample::Rail, "Rail 32"),
        (Sample::Text, "TextButton"),
        (Sample::Bordered, "Button"),
        (Sample::Destructive, "Destructive"),
        (Sample::Chip, "Filter chip"),
        (Sample::Item, "Menu item"),
        (Sample::Activity, "Activity"),
    ];

    fn spawn(self, p: &mut ChildSpawnerCommands, t: &Theme) -> Entity {
        match self {
            Sample::Icon32 => p.spawn(icon_button(
                t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("Settings"),
            )),
            Sample::Icon28 => p.spawn(icon_button(
                t,
                IconId::Filter,
                IconButtonSize::Small,
                Tooltip::new("Filter"),
            )),
            Sample::Rail => p.spawn(icon_button(
                t,
                IconId::CamPan,
                IconButtonSize::Rail,
                Tooltip::new("Pan"),
            )),
            Sample::Text => p.spawn(text_button(t, TextButton::new("World").chevron())),
            Sample::Bordered => p.spawn(button(t, "Duplicate", ButtonVariant::Normal)),
            Sample::Destructive => p.spawn(button(t, "Delete", ButtonVariant::Destructive)),
            Sample::Chip => p.spawn(filter_chip("Models", false)),
            Sample::Item => {
                let mut e = p.spawn(menu_item(t, "Grid", None, Some("G".into())));
                e.insert(Node {
                    width: px(84),
                    height: px(t.s.menu_item_h),
                    padding: UiRect::horizontal(px(SPACE[5])),
                    column_gap: px(SPACE[5]),
                    align_items: AlignItems::Center,
                    border_radius: rounded(radius::SM),
                    ..default()
                });
                e
            }
            Sample::Activity => p.spawn(activity_button(
                t,
                IconId::ActivityTopics,
                Tooltip::new("Topics"),
                None,
            )),
        }
        .id()
    }
}

fn states_grid(c: &mut ChildSpawnerCommands, theme: &Theme) {
    const LABEL_W: f32 = 104.0;
    const CELL_W: f32 = 92.0;
    let caps = |t: &str, w: f32| {
        (
            text(TypeStyle::CAPS, ColorRole::Text3, t.to_string()),
            Node {
                width: px(w),
                ..default()
            },
        )
    };
    c.spawn(card_node(Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(SPACE[7]), px(SPACE[5])),
        row_gap: px(SPACE[1]),
        ..default()
    }))
    .with_children(|g| {
        g.spawn(sample_row(0.0)).with_children(|h| {
            h.spawn(caps("State", LABEL_W));
            for (name, _) in STATES {
                h.spawn(caps(name, CELL_W));
            }
            h.spawn(caps("Focus", CELL_W));
        });
        // Room for the Focus column's ring (offset 2, width 2) above and
        // below, so neighbouring rings don't touch.
        let ring = 2.0 + border::FOCUS_RING;
        for (sample, label) in Sample::ALL {
            let h = match sample {
                Sample::Activity => theme.s.activity_btn,
                _ => theme.s.icon_btn,
            } + 2.0 * ring
                + SPACE[1];
            g.spawn(Node {
                height: px(h),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    text(TypeStyle::SMALL, ColorRole::Text2, label),
                    Node {
                        width: px(LABEL_W),
                        ..default()
                    },
                ));
                for (_, s) in STATES {
                    r.spawn(Node {
                        width: px(CELL_W),
                        ..default()
                    })
                    .with_children(|cell| {
                        let e = sample.spawn(cell, theme);
                        cell.commands().entity(e).insert(ForceState(s));
                    });
                }
                r.spawn(Node {
                    width: px(CELL_W),
                    padding: UiRect::left(px(SPACE[1])),
                    ..default()
                })
                .with_children(|cell| {
                    let e = sample.spawn(cell, theme);
                    let mut commands = cell.commands();
                    commands.entity(e).insert(ForceState(STATES[0].1));
                    show_focus(&mut commands, e);
                });
            });
        }
    });
}

fn right_column(c: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    c.spawn(Node {
        height: px(32),
        align_items: AlignItems::Center,
        column_gap: px(SPACE[6]),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|h| {
        h.spawn(text(TypeStyle::CAPS, ColorRole::Text2, "Color tokens"));
        h.spawn(flex_spacer());
        h.spawn(text(TypeStyle::SMALL, ColorRole::Text3, "T switches theme"));
        h.spawn((
            text_button(theme, TextButton::new("Dark theme")),
            ThemeToggle,
        ));
    });
    token_list(c);
    c.spawn(Node {
        flex_grow: 1.0,
        min_height: px(0),
        ..default()
    })
    .with_children(|bottom| {
        let tree = bottom
            .spawn(card_node(Node {
                width: px(320),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            }))
            .with_children(|t| scene_panel(t, theme, map))
            .id();
        bottom.spawn(splitter(
            theme,
            Splitter {
                axis: SplitAxis::Columns,
                pane: tree,
                sign: 1.0,
                min: 240.0,
                max: 420.0,
            },
            "Resize the scene panel",
        ));
        bottom
            .spawn(Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                flex_direction: FlexDirection::Column,
                row_gap: px(SPACE[4]),
                padding: UiRect::left(px(SPACE[3])),
                ..default()
            })
            .with_children(|w| more_widgets(w, theme, map));
    });
}

fn token_list(c: &mut ChildSpawnerCommands) {
    c.spawn(card_node(Node {
        flex_direction: FlexDirection::Column,
        overflow: Overflow::clip(),
        ..default()
    }))
    .with_children(|list| {
        for (i, (role, name, what)) in TOKENS.into_iter().enumerate() {
            list.spawn((
                Node {
                    height: px(27),
                    padding: UiRect::horizontal(px(SPACE[7])),
                    column_gap: px(SPACE[6]),
                    align_items: AlignItems::Center,
                    border: UiRect::top(px(if i == 0 { 0.0 } else { border::HAIRLINE })),
                    ..default()
                },
                Stroke(ColorRole::Line),
            ))
            .with_children(|r| {
                r.spawn((
                    Node {
                        width: px(20),
                        height: px(20),
                        border: UiRect::all(px(border::HAIRLINE)),
                        border_radius: rounded(radius::SM - 1.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Fill(role),
                    Stroke(ColorRole::Line),
                ));
                r.spawn(text(NAME_TYPE, ColorRole::Text, name))
                    .with_children(|t| {
                        t.spawn(span(DESC_TYPE, ColorRole::Text2, format!(" · {what}")));
                    });
                r.spawn(flex_spacer());
                r.spawn((TokenHex(role), text(HEX_TYPE, ColorRole::Text2, "")));
            });
        }
    });
}

fn scene_panel(t: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    t.spawn(section_header(theme, "Scene", Some("14 entities".into())))
        .with_children(|h| {
            h.spawn(icon_button(
                theme,
                IconId::ExpandAll,
                IconButtonSize::Small,
                Tooltip::new("Expand all").binding(map, "tree.expand_all"),
            ));
            h.spawn(icon_button(
                theme,
                IconId::CollapseAll,
                IconButtonSize::Small,
                Tooltip::new("Collapse all").binding(map, "tree.collapse_all"),
            ));
        });
    t.spawn(Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(SPACE[4])),
        ..default()
    })
    .with_children(|rows| {
        let spec = |depth, kind, name: &str, expanded, hidden| TreeRowSpec {
            depth,
            kind,
            name: name.into(),
            expanded,
            hidden,
        };
        rows.spawn(tree_row(
            theme,
            spec(0, EntityKind::World, "warehouse_demo", Some(true), false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(1, EntityKind::Light, "sun", None, false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(1, EntityKind::Light, "ceiling_light", None, true),
        ));
        rows.spawn((
            tree_row(
                theme,
                spec(1, EntityKind::Model, "amr_01", Some(true), false),
            ),
            Active,
        ));
        rows.spawn(tree_row(
            theme,
            spec(2, EntityKind::Link, "base_link", Some(false), false),
        ));
        rows.spawn(tree_row(
            theme,
            spec(2, EntityKind::Joint, "wheel_left_joint", None, false),
        ));
    });
}

fn more_widgets(w: &mut ChildSpawnerCommands, theme: &Theme, map: &InputMap) {
    let mut linear = SliderSpec::new("Linear", 0.0, 2.0, 0.6);
    linear.step = 0.05;
    w.spawn(slider_row(theme, linear));
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(select(
            theme,
            "Camera mode",
            vec!["Free".into(), "Follow amr_01".into(), "Track amr_01".into()],
            0,
            128.0,
        ));
        // A popover (§5.16): the Snap settings.
        let t = theme.clone();
        r.spawn(menu(
            theme,
            MenuSide::Below,
            text_button_parts(theme, TextButton::new("Snap 0.25").mono().chevron()),
            move |p: &mut ChildSpawner| {
                p.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(SPACE[3]),
                        width: px(220),
                        padding: UiRect::all(px(SPACE[2])),
                        ..default()
                    },
                    children![
                        text(TypeStyle::CAPS, ColorRole::Text3, "Snap"),
                        number_field(
                            &t,
                            AxisTag::Label("move".into()),
                            NumberField::new(0.25).unit(Unit::Length),
                            false,
                        ),
                        number_field(
                            &t,
                            AxisTag::Label("turn".into()),
                            NumberField::new(15.0).unit(Unit::Angle).precision(1, 0.5),
                            false,
                        ),
                    ],
                ));
            },
        ));
        // A menu with icons, a group label, a separator and a checkbox.
        let t = theme.clone();
        let keys = |id: &str| shortcut_label(map, id);
        let (focus, home, follow) = (keys("view.focus"), keys("view.home"), keys("view.follow"));
        r.spawn(menu(
            theme,
            MenuSide::Below,
            text_button_parts(
                theme,
                TextButton::new("Camera")
                    .leading(IconId::CameraMode)
                    .chevron(),
            ),
            move |p: &mut ChildSpawner| {
                p.spawn(menu_group_label("View"));
                p.spawn(menu_item(&t, "Focus", Some(IconId::Focus), focus));
                p.spawn(menu_item(&t, "Home", Some(IconId::Home), home));
                p.spawn(menu_separator());
                p.spawn((
                    menu_item(&t, "Follow", Some(IconId::Follow), follow),
                    Active,
                ));
                p.spawn(menu_checkbox(&t, "Link cameras", false, None));
            },
        ));
    });
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(filter_chip("All", true));
        r.spawn(filter_chip("Models", false));
        r.spawn(filter_chip("Lights", false));
        r.spawn(flex_spacer());
        r.spawn(badge(theme, BadgeKind::Count, 3));
        r.spawn(badge(theme, BadgeKind::Warning, 12));
        r.spawn(badge(theme, BadgeKind::Error, 1));
    });
    w.spawn(sample_row(SPACE[3])).with_children(|r| {
        r.spawn(status_chip(
            theme,
            StatusKind::Error,
            "Lost connection",
            None,
        ));
        r.spawn(status_chip(
            theme,
            StatusKind::Info,
            "Saved",
            Some("1".into()),
        ));
        r.spawn(icon(IconId::Warning, 15.0, ColorRole::WarnText));
    });
}

/// T switches light ↔ dark (not while typing in a field).
pub(crate) fn toggle_theme_key(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextFocus>,
    mut theme: ResMut<Theme>,
) {
    if keys.just_pressed(KeyCode::KeyT) && !typing.0 {
        theme.toggle();
    }
}

/// The theme button switches light ↔ dark.
pub(crate) fn on_theme_button(
    activate: On<Activate>,
    toggles: Query<(), With<ThemeToggle>>,
    mut theme: ResMut<Theme>,
) {
    if toggles.contains(activate.entity) {
        theme.toggle();
    }
}

/// The theme name and caption, the toggle's label and the token hex values.
pub(crate) fn gallery_texts(
    theme: Res<Theme>,
    mut names: Query<(&ThemeText, &mut Text)>,
    mut hexes: Query<(&TokenHex, &mut Text), Without<ThemeText>>,
    toggles: Query<&Children, With<ThemeToggle>>,
    mut labels: Query<&mut Text, (Without<ThemeText>, Without<TokenHex>)>,
) {
    if !theme.is_changed() {
        return;
    }
    let dark = theme.is_dark;
    for (which, mut t) in &mut names {
        let want = match (which, dark) {
            (ThemeText::Name, false) => "Light",
            (ThemeText::Name, true) => "Dark",
            (ThemeText::Caption, false) => "DEFAULT · FOLLOWS OS",
            (ThemeText::Caption, true) => "SAME TOKENS · OWN VALUES",
        };
        set_text(&mut t, want);
    }
    for (token, mut t) in &mut hexes {
        set_text(&mut t, &hex(theme.color(token.0)));
    }
    for children in &toggles {
        for &c in children {
            if let Ok(mut t) = labels.get_mut(c) {
                set_text(&mut t, if dark { "Light theme" } else { "Dark theme" });
            }
        }
    }
}

/// In a capture, focuses the example field once to show the focus look
/// (and a caret). Not interactively: the focused field would take the T
/// key (and, selected on focus, be replaced by a "t").
pub(crate) fn focus_example(
    capture: Option<Res<GalleryCapture>>,
    examples: Query<&Children, With<FocusExample>>,
    inputs: Query<(), With<NumberInput>>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    mut done: Local<bool>,
) {
    if *done || capture.is_none() {
        return;
    }
    for children in &examples {
        if let Some(input) = children.iter().find(|c| inputs.contains(*c)) {
            focus.set(input, FocusCause::Navigated);
            visible.0 = true;
            *done = true;
        }
    }
}

/// Saves the screenshot after [`GalleryCapture::frames`] frames, once.
pub(crate) fn capture_gallery(capture: Option<ResMut<GalleryCapture>>, mut commands: Commands) {
    let Some(mut capture) = capture else {
        return;
    };
    if capture.frames == 0 {
        return;
    }
    capture.frames -= 1;
    if capture.frames == 0 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(gz_bevy::snapshot::save_screenshot(capture.path.clone()));
    }
}

#[cfg(test)]
mod tests;
