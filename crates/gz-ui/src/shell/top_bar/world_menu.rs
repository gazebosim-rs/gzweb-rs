//! The World menu (spec §7.1; DECISIONS.md D-108), from the world button:
//! a header with the world's file name and its state ("unsaved changes ·
//! last saved 12:01", or "saved"), then New world, Open world…, Open
//! recent, Save, Save as…, Revert to saved, Undo and Redo (named for what
//! they'd do), Import…, Export world as SDF…, Export screenshot, and
//! Connect to server… (the current address as its hint).
//!
//! The websocket server can't load a world, so New, Open, Open recent,
//! Revert and Import stay unavailable, saying why. Save writes the
//! authored world through [`SaveWorld`]; Save as… and Export ask for a
//! name first ([`dialog`](super::save_dialog)); a screenshot of the window
//! saves as a PNG. A save says how it went in the status bar.

use accesskit::Role;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::text::LineBreak;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::GzStatus;
use gz_bevy::save::save_bytes;
use gz_bevy::spawn::GzIndex;
use gz_bevy::undo::{UndoCommand, UndoStack};
use gz_bevy::view::input::InputMap;
use gz_bevy::world_save::{SaveWorld, WorldSaved};

use super::save_dialog::{DialogMode, SaveDialog};
use super::world_button::world_button;
use crate::assets::fonts::text;
use crate::assets::icons::IconId;
use crate::shell::connection::{world_name, ConnectionView};
use crate::shell::status_bar::readouts::StatusNote;
use crate::shell::{CurrentPhase, Phase, UiCommand};
use crate::theme::{ColorRole, FontFamily, Ink, Theme, TypeStyle, SPACE};
use crate::widgets::layout::column;
use crate::widgets::menu::{menu, menu_item, menu_separator, MenuItemProps, MenuSide};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::{a11y, Tooltip};

/// The World menu's width (§5.14: `world_menu_w`).
pub(crate) const WORLD_MENU_W: f32 = 280.0;

/// What the World menu, ⌘S and the command palette ask of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldCommand {
    /// ⌘S: the authored world, as `<world>.sdf`.
    Save,
    /// ⇧⌘S: asks for a name (and current poses or authored ones).
    SaveAs,
    /// Asks for a name; the server's poses; the world stays dirty.
    ExportSdf,
    /// The window as a PNG.
    ExportScreenshot,
    /// The connect card, over the world.
    Connect,
}

/// The World menu's items.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorldItem {
    New,
    Open,
    Recent,
    Save,
    SaveAs,
    Revert,
    Undo,
    Redo,
    Import,
    ExportSdf,
    ExportScreenshot,
    Connect,
}

/// Why the server can't do what an item asks, for those it never can.
const NO_LOADING: &str = "The websocket server can't load a world";

impl WorldItem {
    /// Its label, icon, binding and, for one the server never allows, why.
    fn parts(
        self,
    ) -> (
        &'static str,
        Option<IconId>,
        Option<&'static str>,
        Option<&'static str>,
    ) {
        match self {
            WorldItem::New => ("New world", None, Some("file.new"), Some(NO_LOADING)),
            WorldItem::Open => (
                "Open world…",
                Some(IconId::Folder),
                Some("file.open"),
                Some(NO_LOADING),
            ),
            WorldItem::Recent => ("Open recent", Some(IconId::Recent), None, Some(NO_LOADING)),
            WorldItem::Save => ("Save", Some(IconId::Save), Some("file.save"), None),
            WorldItem::SaveAs => ("Save as…", None, Some("file.save_as"), None),
            WorldItem::Revert => (
                "Revert to saved",
                Some(IconId::Revert),
                None,
                Some(NO_LOADING),
            ),
            WorldItem::Undo => ("Undo", Some(IconId::Undo), Some("edit.undo"), None),
            WorldItem::Redo => ("Redo", Some(IconId::Redo), Some("edit.redo"), None),
            WorldItem::Import => (
                "Import SDF / URDF / mesh…",
                Some(IconId::Import),
                Some("file.import"),
                Some("Not yet: inserting goes through the Assets panel"),
            ),
            WorldItem::ExportSdf => ("Export world as SDF…", Some(IconId::Export), None, None),
            WorldItem::ExportScreenshot => ("Export screenshot", None, None, None),
            WorldItem::Connect => ("Connect to server…", Some(IconId::Server), None, None),
        }
    }

    /// The groups, separators between them (§7.1).
    const GROUPS: [&'static [WorldItem]; 5] = [
        &[WorldItem::New, WorldItem::Open, WorldItem::Recent],
        &[WorldItem::Save, WorldItem::SaveAs, WorldItem::Revert],
        &[WorldItem::Undo, WorldItem::Redo],
        &[
            WorldItem::Import,
            WorldItem::ExportSdf,
            WorldItem::ExportScreenshot,
        ],
        &[WorldItem::Connect],
    ];
}

/// The header's two lines.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorldHeader {
    /// The file name, "warehouse_demo.sdf".
    Name,
    /// "unsaved changes · last saved 12:01" (`warn_text`) or "saved"
    /// (`text_3`).
    State,
}

/// A World menu item whose label keeps to one line (the menu is a fixed
/// 280 wide, §5.14).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct OneLine;

/// The items' labels don't wrap.
pub(crate) fn one_line_labels(
    items: Query<&Children, Added<OneLine>>,
    mut layouts: Query<&mut TextLayout>,
) {
    for children in &items {
        for &c in children {
            if let Ok(mut l) = layouts.get_mut(c) {
                l.linebreak = LineBreak::NoWrap;
            }
        }
    }
}

/// When the world was last saved this session, on the local clock.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LastSaved(pub Option<String>);

/// Another server or world wasn't saved this session.
pub(crate) fn forget_last_saved(
    status: Option<Res<GzStatus>>,
    mut last: ResMut<LastSaved>,
    mut seen: Local<Option<(String, String)>>,
) {
    let now = status
        .as_deref()
        .and_then(|s| Some((s.url.clone(), s.world.clone()?)));
    if now.is_some() && *seen != now {
        if seen.is_some() && last.0.is_some() {
            last.0 = None;
        }
        *seen = now;
    }
}

/// The longest Undo or Redo label (with its edit's name), so it and its
/// key fit the menu's fixed width: longer ones end in "…".
const EDIT_LABEL_MAX: usize = 26;

/// `label`, cut to [`EDIT_LABEL_MAX`] characters with an ellipsis.
pub(crate) fn fit_label(label: &str) -> String {
    if label.chars().count() <= EDIT_LABEL_MAX {
        return label.to_string();
    }
    let cut: String = label.chars().take(EDIT_LABEL_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The world button, as the World menu's trigger.
pub(crate) fn world_menu(theme: &Theme, map: &InputMap) -> impl Bundle {
    let t = theme.clone();
    let map = map.clone();
    menu(theme, MenuSide::Below, world_button(theme), move |p| {
        p.spawn(Node {
            width: px(WORLD_MENU_W - 2.0 * SPACE[3]),
            ..column(0.0)
        })
        .with_children(|m| {
            m.spawn((
                Node {
                    padding: UiRect::axes(px(SPACE[5]), px(SPACE[3])),
                    ..column(SPACE[1])
                },
                a11y(Role::Group, "World"),
            ))
            .with_children(|h| {
                h.spawn((
                    text(
                        TypeStyle::new(FontFamily::Ui, 13.0, 600),
                        ColorRole::Text,
                        "No world",
                    ),
                    WorldHeader::Name,
                ));
                h.spawn((
                    text(TypeStyle::MONO_SMALL, ColorRole::Text3, ""),
                    WorldHeader::State,
                ));
            });
            for group in WorldItem::GROUPS {
                m.spawn(menu_separator());
                for &item in group {
                    let (label, icon, binding, why) = item.parts();
                    let mut props = MenuItemProps::new(label)
                        .shortcut_opt(binding.and_then(|b| shortcut_label(&map, b)));
                    props.icon = icon;
                    if item == WorldItem::Connect {
                        // Its hint: the current address.
                        props.shortcut = Some(String::new());
                    }
                    let mut e = m.spawn((menu_item(&t, props), item, OneLine));
                    if let Some(why) = why {
                        e.insert((InteractionDisabled, Tooltip::new(label).detail(why)));
                    }
                }
            }
        });
    })
}

/// The header and the items follow the world: its name, whether it's
/// dirty and when it was saved; Save, Save as… and the exports need a
/// live world; Undo and Redo name what they'd do; Connect shows the
/// current address.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn world_menu_states(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    undo: Option<Res<UndoStack>>,
    last: Res<LastSaved>,
    mut headers: Query<(&WorldHeader, &mut Text, &mut Ink)>,
    items: Query<(Entity, &WorldItem, &Children, Has<InteractionDisabled>)>,
    mut texts: Query<&mut Text, Without<WorldHeader>>,
    added: Query<(), Added<WorldItem>>,
    mut commands: Commands,
) {
    let changed = phase.is_changed()
        || status.as_ref().is_some_and(Res::is_changed)
        || index.as_ref().is_some_and(Res::is_changed)
        || undo.as_ref().is_some_and(Res::is_changed)
        || last.is_changed()
        || !added.is_empty();
    if !changed {
        return;
    }
    let live = phase.0 == Phase::Live;
    let world = world_name(status.as_deref(), index.as_deref());
    let dirty = undo.as_deref().is_some_and(UndoStack::is_dirty);
    for (which, mut t, mut ink) in &mut headers {
        let (want, role) = match which {
            WorldHeader::Name => (
                world
                    .as_ref()
                    .map_or_else(|| "No world".into(), |w| format!("{w}.sdf")),
                ColorRole::Text,
            ),
            WorldHeader::State if world.is_none() => (String::new(), ColorRole::Text3),
            WorldHeader::State if dirty => (
                match &last.0 {
                    Some(at) => format!("unsaved changes · last saved {at}"),
                    None => "unsaved changes".into(),
                },
                ColorRole::WarnText,
            ),
            WorldHeader::State => ("saved".into(), ColorRole::Text3),
        };
        set_text(&mut t, &want);
        ink.set_if_neq(Ink(role));
    }
    let undo = undo.as_deref();
    for (e, item, children, disabled) in &items {
        let (label, ..) = item.parts();
        let (enabled, label) = match item {
            WorldItem::Save
            | WorldItem::SaveAs
            | WorldItem::ExportSdf
            | WorldItem::ExportScreenshot => (live, label.to_string()),
            WorldItem::Undo => match undo.and_then(UndoStack::undo_label) {
                Some(l) => (true, fit_label(&format!("Undo {l}"))),
                None => (false, label.to_string()),
            },
            WorldItem::Redo => match undo.and_then(UndoStack::redo_label) {
                Some(l) => (true, fit_label(&format!("Redo {l}"))),
                None => (false, label.to_string()),
            },
            WorldItem::Connect => (true, label.to_string()),
            _ => continue,
        };
        if enabled == disabled {
            if enabled {
                commands.entity(e).remove::<InteractionDisabled>();
            } else {
                commands.entity(e).insert(InteractionDisabled);
            }
        }
        // The label is the item's first text; Connect's hint its second.
        let parts: Vec<Entity> = children.iter().filter(|c| texts.contains(*c)).collect();
        if let Some(mut t) = parts.first().and_then(|c| texts.get_mut(*c).ok()) {
            set_text(&mut t, &label);
        }
        if *item == WorldItem::Connect {
            let hint = status.as_deref().map_or("", |s| {
                s.url
                    .trim_start_matches("ws://")
                    .trim_start_matches("wss://")
            });
            if let Some(mut t) = parts.get(1).and_then(|c| texts.get_mut(*c).ok()) {
                set_text(&mut t, hint);
            }
        }
    }
}

/// A World menu item asks for what it says.
pub(crate) fn on_world_item(
    ev: On<Activate>,
    items: Query<&WorldItem, Without<InteractionDisabled>>,
    mut ui: MessageWriter<UiCommand>,
    mut undo: MessageWriter<UndoCommand>,
) {
    let Ok(item) = items.get(ev.entity) else {
        return;
    };
    let cmd = match item {
        WorldItem::Save => WorldCommand::Save,
        WorldItem::SaveAs => WorldCommand::SaveAs,
        WorldItem::ExportSdf => WorldCommand::ExportSdf,
        WorldItem::ExportScreenshot => WorldCommand::ExportScreenshot,
        WorldItem::Connect => WorldCommand::Connect,
        WorldItem::Undo => {
            undo.write(UndoCommand::Undo);
            return;
        }
        WorldItem::Redo => {
            undo.write(UndoCommand::Redo);
            return;
        }
        _ => return,
    };
    ui.write(UiCommand::World(cmd));
}

/// The file a world saves as by default: `<world>.sdf`.
pub(crate) fn default_name(world: Option<&str>) -> String {
    format!("{}.sdf", world.unwrap_or("world"))
}

/// Runs the [`WorldCommand`]s: Save asks for the authored world as
/// `<world>.sdf`; Save as… and Export open the name dialog; a screenshot
/// is taken; Connect shows the connect card over the world. None but
/// Connect does anything without a live world.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_world_commands(
    mut commands_in: MessageReader<UiCommand>,
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    index: Option<Res<GzIndex>>,
    mut saves: MessageWriter<SaveWorld>,
    mut dialog: ResMut<SaveDialog>,
    mut view: ResMut<ConnectionView>,
    mut commands: Commands,
) {
    let live = phase.0 == Phase::Live;
    let world = world_name(status.as_deref(), index.as_deref());
    for cmd in commands_in.read() {
        let UiCommand::World(cmd) = cmd else {
            continue;
        };
        match cmd {
            WorldCommand::Connect => view.switching = true,
            _ if !live => {}
            WorldCommand::Save => {
                saves.write(SaveWorld {
                    name: default_name(world.as_deref()),
                    current_poses: false,
                    mark_saved: true,
                });
            }
            WorldCommand::SaveAs => dialog.open(DialogMode::SaveAs, world.as_deref()),
            WorldCommand::ExportSdf => dialog.open(DialogMode::Export, world.as_deref()),
            WorldCommand::ExportScreenshot => {
                let name = format!(
                    "{}-{}.png",
                    world.as_deref().unwrap_or("world"),
                    gz_bevy::messages::clock_now().replace(':', "")
                );
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_screenshot(name));
            }
        }
    }
}

/// Saves a captured window as `name` (a PNG), saying so in the status bar.
fn save_screenshot(
    name: String,
) -> impl FnMut(On<ScreenshotCaptured>, Res<Time<Real>>, ResMut<StatusNote>) {
    move |ev, time, mut note| {
        let saved = png_bytes(&ev.image).and_then(|png| save_bytes(&name, "image/png", &png));
        match saved {
            Ok(at) => note.show(format!("Saved {}", file_name(&at)), time.elapsed_secs_f64()),
            Err(e) => warn!("Couldn't save the screenshot: {e}"),
        }
    }
}

/// A captured image as PNG bytes.
fn png_bytes(image: &Image) -> Result<Vec<u8>, String> {
    let rgba = image
        .clone()
        .try_into_dynamic()
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(rgba.as_raw())
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The last part of a path ("/home/u/w.sdf" reads "w.sdf").
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A finished save says so in the status bar ("Saved warehouse_demo.sdf"
/// for 3 s) and, when it saved the world, when; a failed one logs why
/// (the status bar's chip and the Console show it).
pub(crate) fn world_saved(
    mut saved: MessageReader<WorldSaved>,
    time: Res<Time<Real>>,
    mut note: ResMut<StatusNote>,
    mut last: ResMut<LastSaved>,
) {
    for s in saved.read() {
        match &s.result {
            Ok(at) => {
                note.show(format!("Saved {}", file_name(at)), time.elapsed_secs_f64());
                if s.marked {
                    last.0 = Some(gz_bevy::messages::clock_now()[..5].to_string());
                }
            }
            Err(e) => warn!("Couldn't save the world: {e}"),
        }
    }
}

#[cfg(test)]
mod tests;
