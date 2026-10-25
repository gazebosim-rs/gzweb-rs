//! The Workspaces panel's body and footer (spec §3.3, §8; the board's
//! panel): the cards, the current layout's summary and its buttons, the
//! name row of Save as new…, and the footer's note and Import / export
//! layouts…. Built whenever the shell spawns them (DECISIONS.md D-58,
//! D-98); the cards and the summary follow in place
//! ([`systems`](super::systems)).

use std::str::FromStr;

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{observe, Button as Headless, ScrollArea};
use gz_bevy::view::input::InputMap;

use super::{WorkspacesBody, WorkspacesFooter};
use crate::assets::fonts::text;
use crate::assets::IconId;
use crate::panels::PanelRegistry;
use crate::shell::viewport::views::LayoutPreset;
use crate::shell::workspaces::{Persisted, StoreAt, WorkspaceLayout, WorkspaceStore};
use crate::theme::visuals::{Active, Visuals};
use crate::theme::{border, radius, ColorRole, Fill, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::button::ICON_BUTTON_RADIUS;
use crate::widgets::focus::TakesEscape;
use crate::widgets::layout::{column, flex_spacer, property_row, row};
use crate::widgets::menu::{
    menu_item, on_menu_event, popup, Menu, MenuItemProps, MenuSide, MenuTrigger,
};
use crate::widgets::section::section_header;
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::{
    a11y, button, clickable, icon_button, rounded, text_field, ButtonVariant, IconButtonSize,
    Tooltip,
};

/// A card's thumbnail (§3.3: 72×44).
pub(crate) const THUMB: (f32, f32) = (72.0, 44.0);

/// The thumbnail's side panels' widths.
const THUMB_SIDE: f32 = 14.0;

/// The gap between the thumbnail's blocks.
const THUMB_GAP: f32 = 2.0;

/// The cards' parent.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Cards;

/// A workspace's card: its index.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Card(pub(crate) usize);

/// A user's workspace's Delete, on its card: its index.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CardDelete(pub(crate) usize);

/// A summary row's value.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SummaryValue {
    Sidebar,
    Inspector,
    Viewports,
    Floating,
    Theme,
}

impl SummaryValue {
    const ALL: [SummaryValue; 5] = [
        SummaryValue::Sidebar,
        SummaryValue::Inspector,
        SummaryValue::Viewports,
        SummaryValue::Floating,
        SummaryValue::Theme,
    ];

    fn label(self) -> &'static str {
        match self {
            SummaryValue::Sidebar => "Sidebar",
            SummaryValue::Inspector => "Inspector",
            SummaryValue::Viewports => "Viewports",
            SummaryValue::Floating => "Floating panels",
            SummaryValue::Theme => "Theme",
        }
    }
}

/// The current layout section's header (its count text: "Build ·
/// modified").
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CurrentHeader;

/// The current layout's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutButton {
    /// "Save to Build".
    Save,
    /// "Save as new…".
    SaveAsNew,
    Reset,
}

/// Save as new…'s row: the name field, Cancel and Save.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct NameRow;

/// The name field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct NameField;

/// The name row's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NameButton {
    Cancel,
    Save,
}

/// The footer menu's items.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IoItem {
    Export,
    Import,
}

/// Why Import is unavailable.
pub(crate) const NO_IMPORT: &str = "Not yet: gzweb has no file picker";

/// Builds the ready body: the cards (filled by `sync_cards`), the current
/// layout section, and the name row (shown by `sync_naming`).
pub(super) fn build_body(ev: On<Add<WorkspacesBody>>, theme: Res<Theme>, mut commands: Commands) {
    let t = theme.clone();
    commands.entity(ev.entity).with_children(|b| {
        b.spawn((
            ScrollArea,
            ScrollPosition::default(),
            TabIndex(0),
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                min_height: px(0),
                overflow: Overflow::scroll_y(),
                ..column(0.0)
            },
        ))
        .with_children(|c| {
            c.spawn((
                Cards,
                a11y(Role::List, "Workspaces"),
                Node {
                    padding: UiRect::all(px(SPACE[4])),
                    flex_shrink: 0.0,
                    ..column(SPACE[3])
                },
            ));
            c.spawn(Node {
                border: UiRect::top(px(border::HAIRLINE)),
                flex_shrink: 0.0,
                ..column(0.0)
            })
            .insert(Stroke(ColorRole::LineSoft))
            .with_children(|s| {
                s.spawn((section_header(&t, "Current layout", None), CurrentHeader));
                s.spawn(Node {
                    padding: UiRect::axes(px(SPACE[7]), px(SPACE[5])),
                    flex_shrink: 0.0,
                    ..column(SPACE[4])
                })
                .with_children(|rows| {
                    for v in SummaryValue::ALL {
                        rows.spawn(property_row(
                            v.label(),
                            (text(TypeStyle::MONO_SMALL, ColorRole::Text2, ""), v),
                        ));
                    }
                    rows.spawn(Node {
                        margin: UiRect::top(px(SPACE[2])),
                        ..row(SPACE[3])
                    })
                    .with_children(|r| {
                        for (b, label) in [
                            (LayoutButton::Save, "Save"),
                            (LayoutButton::SaveAsNew, "Save as new…"),
                            (LayoutButton::Reset, "Reset"),
                        ] {
                            equal_button(r, &t, label).insert(b);
                        }
                    });
                    rows.spawn((
                        NameRow,
                        // Its Esc closes it, not the shortcuts' too.
                        TakesEscape,
                        Node {
                            display: Display::None,
                            ..column(SPACE[3])
                        },
                    ))
                    .with_children(|n| {
                        n.spawn((text_field(&t, "Workspace name", ""), NameField));
                        n.spawn(row(SPACE[3])).with_children(|r| {
                            r.spawn(flex_spacer());
                            r.spawn((
                                button(&t, "Cancel", ButtonVariant::Normal),
                                NameButton::Cancel,
                            ));
                            r.spawn((button(&t, "Save", ButtonVariant::Normal), NameButton::Save));
                        });
                    });
                });
            });
        });
    });
}

/// A button sized to its label, sharing what's left of its row.
fn equal_button<'a>(
    row: &'a mut ChildSpawnerCommands,
    theme: &Theme,
    label: &str,
) -> EntityCommands<'a> {
    let mut b = row.spawn(button(theme, label, ButtonVariant::Normal));
    b.insert(Node {
        flex_grow: 1.0,
        height: px(theme.s.button_h),
        padding: UiRect::horizontal(px(SPACE[3])),
        border: UiRect::all(px(border::HAIRLINE)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: rounded(radius::SM),
        ..default()
    });
    b
}

/// What a card shows of a workspace.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CardLook {
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) keys: Option<String>,
    pub(crate) thumb: Thumb,
    pub(crate) current: bool,
    /// One of the user's: it has Delete.
    pub(crate) custom: bool,
}

/// A layout's thumbnail: the sidebar, the views' preset (and its first
/// share), the inspector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Thumb {
    pub(crate) sidebar: bool,
    pub(crate) preset: LayoutPreset,
    pub(crate) share: f32,
    pub(crate) inspector: bool,
}

impl Thumb {
    pub(crate) fn of(w: &WorkspaceLayout) -> Self {
        Self {
            sidebar: w.sidebar_open && w.primary.is_some(),
            preset: LayoutPreset::from_str(&w.preset).unwrap_or(LayoutPreset::Single),
            // In twentieths, so a divider's drag doesn't redraw the card
            // each frame.
            share: ((w.shares[0] * 20.0).round() / 20.0).clamp(0.2, 0.8),
            inspector: w.inspector_open,
        }
    }
}

/// What a layout shows, in words: "scene · inspector · 2 views",
/// "viewport only".
pub(crate) fn detail_of(w: &WorkspaceLayout, panels: &PanelRegistry) -> String {
    let title = |name: &Option<String>| {
        name.as_deref().map(|n| {
            panels
                .iter()
                .find(|d| d.id.name() == n)
                .map_or(n.to_string(), |d| d.title.to_lowercase())
        })
    };
    let mut parts: Vec<String> = Vec::new();
    if w.sidebar_open {
        parts.extend(title(&w.primary));
        parts.extend(title(&w.secondary));
    }
    if w.inspector_open {
        parts.push("inspector".into());
    }
    let views = LayoutPreset::from_str(&w.preset).map_or(1, |p| p.count());
    if views > 1 {
        parts.push(format!("{views} views"));
    }
    if !w.popouts.is_empty() {
        parts.push(format!("{} floating", w.popouts.len()));
    }
    if parts.is_empty() {
        "viewport only".into()
    } else {
        parts.join(" · ")
    }
}

/// A block of a thumbnail, growing by `grow`.
fn block(grow: f32) -> impl Bundle {
    (
        Node {
            flex_grow: grow,
            flex_basis: px(0),
            border_radius: rounded(2.0),
            ..default()
        },
        Fill(ColorRole::Line),
    )
}

/// The views' blocks in the preset's arrangement.
fn views_blocks(p: &mut ChildSpawner, thumb: Thumb) {
    let gap = px(THUMB_GAP);
    let line = |dir: FlexDirection| Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        flex_direction: dir,
        column_gap: gap,
        row_gap: gap,
        ..default()
    };
    match thumb.preset {
        LayoutPreset::Single => {
            p.spawn(block(1.0));
        }
        LayoutPreset::TwoColumns => {
            p.spawn(block(thumb.share));
            p.spawn(block(1.0 - thumb.share));
        }
        LayoutPreset::TwoRows => {
            p.spawn(line(FlexDirection::Column)).with_children(|c| {
                c.spawn(block(1.0));
                c.spawn(block(1.0));
            });
        }
        LayoutPreset::OnePlusTwo => {
            p.spawn(block(thumb.share));
            p.spawn(Node {
                flex_grow: 1.0 - thumb.share,
                ..line(FlexDirection::Column)
            })
            .with_children(|c| {
                c.spawn(block(1.0));
                c.spawn(block(1.0));
            });
        }
        LayoutPreset::Quad => {
            p.spawn(line(FlexDirection::Column)).with_children(|c| {
                for _ in 0..2 {
                    c.spawn(line(FlexDirection::Row)).with_children(|r| {
                        r.spawn(block(1.0));
                        r.spawn(block(1.0));
                    });
                }
            });
        }
    }
}

/// A thumbnail: the sidebar, the views, the inspector, as blocks.
fn thumbnail(thumb: Thumb) -> impl Bundle {
    (
        Node {
            width: px(THUMB.0),
            height: px(THUMB.1),
            flex_shrink: 0.0,
            align_items: AlignItems::Stretch,
            ..row(THUMB_GAP)
        },
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if thumb.sidebar {
                p.spawn((
                    Node {
                        width: px(THUMB_SIDE),
                        flex_shrink: 0.0,
                        border_radius: rounded(2.0),
                        ..default()
                    },
                    Fill(ColorRole::Line),
                ));
            }
            p.spawn(Node {
                flex_grow: 1.0,
                column_gap: px(THUMB_GAP),
                ..default()
            })
            .with_children(|v| views_blocks(v, thumb));
            if thumb.inspector {
                p.spawn((
                    Node {
                        width: px(THUMB_SIDE - 2.0),
                        flex_shrink: 0.0,
                        border_radius: rounded(2.0),
                        ..default()
                    },
                    Fill(ColorRole::Line),
                ));
            }
        })),
    )
}

/// The Delete button's side, and the room the card leaves it.
fn delete_room(theme: &Theme) -> f32 {
    theme.s.icon_btn_sm + SPACE[4]
}

/// A workspace's card: the thumbnail, the name over what it shows, then
/// its keys (one of the user's leaves room for Delete, beside it in the
/// list item). The current one in `accent_soft` with an `accent` border.
pub(crate) fn card(theme: &Theme, index: usize, look: CardLook) -> impl Bundle {
    let visuals = Visuals {
        active_ink: ColorRole::Text,
        active_stroke: Some(ColorRole::Accent),
        ..Visuals::new(ColorRole::Surface, ColorRole::Text)
            .stroked(ColorRole::Line)
            .keep_ink()
    };
    let mut label = format!("{}: {}", look.name, look.detail);
    if look.current {
        label.push_str(", current");
    }
    let right = if look.custom {
        SPACE[4] + delete_room(theme)
    } else {
        SPACE[4]
    };
    (
        Card(index),
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            padding: UiRect::new(px(SPACE[4]), px(right), px(SPACE[4]), px(SPACE[4])),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(radius::LG),
            align_items: AlignItems::Center,
            ..row(SPACE[5])
        },
        Headless,
        clickable(visuals, Role::Button, label),
        Children::spawn(SpawnWith(move |c: &mut ChildSpawner| {
            c.spawn(thumbnail(look.thumb));
            c.spawn(Node {
                flex_grow: 1.0,
                min_width: px(0),
                ..column(SPACE[1])
            })
            .with_children(|n| {
                n.spawn(text(TypeStyle::BODY, ColorRole::Text, look.name.clone()));
                n.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text3, look.detail));
            });
            if let Some(keys) = look.keys {
                c.spawn(text(TypeStyle::MONO_SMALL, ColorRole::Text3, keys));
            }
        })),
    )
}

/// The card's look for workspace `index`.
pub(crate) fn card_look(
    workspaces: &crate::shell::Workspaces,
    index: usize,
    now: Option<&WorkspaceLayout>,
    modified: bool,
    panels: &PanelRegistry,
    map: Option<&InputMap>,
) -> CardLook {
    let w = &workspaces.list()[index];
    let current = index == workspaces.current;
    let layout = if current {
        now.unwrap_or(w.shown())
    } else {
        w.shown()
    };
    let modified = if current { modified } else { w.left_modified() };
    let mut detail = detail_of(layout, panels);
    if modified {
        detail.push_str(" · modified");
    }
    CardLook {
        name: w.name.clone(),
        detail,
        keys: w
            .binding()
            .and_then(|b| map.and_then(|m| shortcut_label(m, b))),
        thumb: Thumb::of(layout),
        current,
        custom: w.builtin.is_none(),
    }
}

/// A card spawned: its list item (the cards' child) and its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spawned {
    pub(crate) item: Entity,
    pub(crate) card: Entity,
}

/// Spawns `look`'s card: a list item holding the card (marked current)
/// and, one of the user's, its Delete over the card's right end. The
/// caller places the item.
pub(crate) fn spawn_card(
    commands: &mut Commands,
    theme: &Theme,
    index: usize,
    look: CardLook,
) -> Spawned {
    let (current, custom, name) = (look.current, look.custom, look.name.clone());
    let item = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(0.0)
            },
            a11y(Role::ListItem, name.clone()),
        ))
        .id();
    let mut c = commands.spawn((card(theme, index, look), ChildOf(item)));
    if current {
        c.insert(Active);
    }
    let card = c.id();
    if custom {
        let side = theme.s.icon_btn_sm;
        commands
            .spawn((
                icon_button(
                    theme,
                    IconId::Trash,
                    IconButtonSize::Small,
                    Tooltip::new(format!("Delete {name}")),
                ),
                CardDelete(index),
                ChildOf(item),
            ))
            .insert(Node {
                width: px(side),
                height: px(side),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: rounded(ICON_BUTTON_RADIUS),
                // Over the card's right end, in the room it leaves.
                margin: UiRect::left(px(-delete_room(theme))),
                ..default()
            });
    }
    Spawned { item, card }
}

/// Where the layouts are kept, for the footer's note; a store that isn't
/// written this session (`read_only`) says so.
pub(crate) fn note_of(store: Option<&WorkspaceStore>, read_only: bool) -> String {
    let at = match store.map(|s| &s.at) {
        Some(StoreAt::File(path)) => path.display().to_string(),
        Some(StoreAt::Browser) => "this browser's storage".into(),
        _ => return "Layouts are kept for this session only".into(),
    };
    if read_only {
        format!("Layouts are kept for this session only: {at} is left as it is")
    } else {
        format!("Layouts are kept in {at}")
    }
}

/// Builds the footer: the note over Import / export layouts… (a menu:
/// Export layouts…, and Import, unavailable).
pub(super) fn build_footer(
    ev: On<Add<WorkspacesFooter>>,
    theme: Res<Theme>,
    store: Option<Res<WorkspaceStore>>,
    persisted: Res<Persisted>,
    mut commands: Commands,
) {
    let t = theme.clone();
    let note = note_of(store.as_deref(), persisted.read_only());
    commands.entity(ev.entity).with_children(|f| {
        f.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            ..column(SPACE[4])
        })
        .with_children(|c| {
            c.spawn(text(TypeStyle::SMALL, ColorRole::Text2, note));
            c.spawn((
                Menu,
                Node {
                    width: Val::Percent(100.0),
                    ..default()
                },
                observe(on_menu_event),
            ))
            .with_children(|m| {
                let mut trigger = m.spawn(button(
                    &t,
                    "Import / export layouts…",
                    ButtonVariant::Normal,
                ));
                trigger.insert((
                    Node {
                        flex_grow: 1.0,
                        height: px(t.s.button_h),
                        border: UiRect::all(px(border::HAIRLINE)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: rounded(radius::SM),
                        ..default()
                    },
                    MenuTrigger,
                    bevy::ui_widgets::MenuButton,
                ));
                let t = t.clone();
                m.spawn(popup(&t.clone(), MenuSide::Above, move |p| {
                    p.spawn((
                        menu_item(&t, MenuItemProps::new("Export layouts…")),
                        IoItem::Export,
                    ));
                    p.spawn((
                        menu_item(&t, MenuItemProps::new("Import layouts…")),
                        IoItem::Import,
                        InteractionDisabled,
                        Tooltip::new("Import layouts…").detail(NO_IMPORT),
                    ));
                }));
            });
        });
    });
}
