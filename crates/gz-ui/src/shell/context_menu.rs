//! The entity context menu (spec §7.9; DECISIONS.md D-109): a right click
//! on a model in a view, or on a row of the Scene tree, opens it at the
//! pointer for that entity (selected first unless it already is). Focus F
//! · Follow ⇧F · Track ⇧T · Move camera to · Rename F2 · Hide / Show ·
//! Copy ⌘C · Paste ⌘V · Duplicate ⌘D · Delete · Align › · Show › · Apply
//! force… · Edit model · Inspect.
//!
//! The camera items act in the view it opened in (from the tree: every
//! view, or the first for Follow and Track); Move camera to frames it in
//! every view. Copy, Paste, Duplicate and Delete act on the selection's
//! top-level models ([`EditModels`]), Align on two or more of them
//! ([`AlignModels`]): those need a server that takes edits. Show › turns
//! the overlays on or off in the view (they draw for every entity). The
//! server can't rename a model, and Apply force… waits, so those say
//! why. Edit model edits the model in place (§6.6, D-112).

use bevy::input_focus::tab_navigation::NavAction;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, MenuAction, MenuEvent};
use gz_bevy::camera::MainCamera;
use gz_bevy::connection::GzStatus;
use gz_bevy::edit::{edits_allowed, movable, HasPlane};
use gz_bevy::overlays::{Overlay, OverlayCommand, OverlaySet, Overlays};
use gz_bevy::spawn::{GzEntity, GzWorldRoot};
use gz_bevy::undo::UndoStack;
use gz_bevy::undo::{Change, Record};
use gz_bevy::view::align::{AlignEdge, AlignModels};
use gz_bevy::view::editing::{EditCommand, EditingModel};
use gz_bevy::view::input::InputMap;
use gz_bevy::view::model_edits::{EditModels, ModelClipboard, ModelEdit, ModelsEdited};
use gz_bevy::view::{CameraMode, EntityContext, SelectMode, Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use crate::assets::icons::IconId;
use crate::shell::layout::{ShellLayout, Side};
use crate::shell::status_bar::readouts::StatusNote;
use crate::theme::Theme;
use crate::widgets::menu::{
    menu, menu_checkbox, menu_group_label, menu_item, menu_separator, submenu, MenuItemProps,
    MenuPopupOf, MenuSide,
};
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::Tooltip;

/// The open context menu: its subject, the view it opened in, and the
/// [`Menu`](crate::widgets::menu::Menu) inside it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContextMenu {
    pub(crate) subject: Entity,
    pub(crate) view: Option<Entity>,
    menu: Entity,
    shown: bool,
}

/// The context menu's items.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextItem {
    Focus,
    Follow,
    Track,
    MoveTo,
    Rename,
    Hide,
    Edit(ModelEdit),
    Align(usize, AlignEdge),
    Show(Overlay),
    ApplyForce,
    EditModel,
    Inspect,
}

/// The overlays Show › turns on and off (§7.9).
const SHOWN: [(Overlay, &str); 5] = [
    (Overlay::Collisions, "Collisions"),
    (Overlay::Inertia, "Inertia"),
    (Overlay::CenterOfMass, "Center of mass"),
    (Overlay::Joints, "Joints"),
    (Overlay::Frames, "Frames"),
];

/// What the menu needs to know about the world as it opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MenuState {
    /// The subject is a top-level model.
    pub(crate) model: bool,
    /// The subject is hidden.
    pub(crate) hidden: bool,
    /// The server takes edits.
    pub(crate) edits: bool,
    /// There's something to paste.
    pub(crate) copied: bool,
    /// How many of the selection's top-level models can move.
    pub(crate) movers: usize,
    /// The overlays on where it opened.
    pub(crate) overlays: Overlays,
    /// The subject is the model being edited.
    pub(crate) edited: bool,
}

/// Why an item can't run now, if it can't.
fn unavailable(item: ContextItem, s: &MenuState) -> Option<&'static str> {
    const NO_EDITS: &str = "This server doesn't take edits";
    match item {
        ContextItem::Rename => Some("The server can't rename a model"),
        ContextItem::ApplyForce => Some("Not yet"),
        ContextItem::EditModel if s.edited => Some("Editing it now"),
        ContextItem::Edit(ModelEdit::Copy) if !s.model => Some("Only a model copies"),
        ContextItem::Edit(ModelEdit::Paste) if !s.copied => Some("Nothing copied"),
        ContextItem::Edit(ModelEdit::Duplicate | ModelEdit::Delete) if !s.model => {
            Some("Only a model, for now")
        }
        ContextItem::Edit(e) if e.edits() && !s.edits => Some(NO_EDITS),
        ContextItem::Align(..) if s.movers < 2 => Some("Select two or more models"),
        ContextItem::Align(..) if !s.edits => Some(NO_EDITS),
        _ => None,
    }
}

/// The parts of an item: label, icon and binding.
fn parts(item: ContextItem, s: &MenuState) -> (String, Option<IconId>, Option<&'static str>) {
    let (label, icon, binding) = match item {
        ContextItem::Focus => ("Focus", Some(IconId::Focus), Some("view.focus")),
        ContextItem::Follow => ("Follow", Some(IconId::Follow), Some("view.follow")),
        ContextItem::Track => ("Track", Some(IconId::Track), Some("view.track")),
        ContextItem::MoveTo => ("Move camera to", Some(IconId::MoveTo), None),
        ContextItem::Rename => ("Rename", Some(IconId::Rename), Some("edit.rename")),
        ContextItem::Hide if s.hidden => ("Show", Some(IconId::Eye), None),
        ContextItem::Hide => ("Hide", Some(IconId::EyeOff), None),
        ContextItem::Edit(ModelEdit::Copy) => ("Copy", Some(IconId::Copy), Some("edit.copy")),
        ContextItem::Edit(ModelEdit::Paste) => ("Paste", Some(IconId::Copy), Some("edit.paste")),
        ContextItem::Edit(ModelEdit::Duplicate) => {
            ("Duplicate", Some(IconId::Plus), Some("edit.duplicate"))
        }
        ContextItem::Edit(ModelEdit::Delete) => {
            ("Delete", Some(IconId::Trash), Some("edit.delete"))
        }
        ContextItem::Align(_, edge) => (edge.label(), None, None),
        ContextItem::Show(o) => (
            SHOWN.iter().find(|(s, _)| *s == o).map_or("", |(_, l)| l),
            None,
            None,
        ),
        ContextItem::ApplyForce => ("Apply force…", Some(IconId::Translate), None),
        ContextItem::EditModel => ("Edit model", Some(IconId::Settings), None),
        ContextItem::Inspect => ("Inspect", Some(IconId::Search), Some("ui.inspector")),
    };
    (label.to_string(), icon, binding)
}

/// An item, disabled with its reason when it can't run.
fn item(p: &mut ChildSpawner, theme: &Theme, map: &InputMap, which: ContextItem, s: &MenuState) {
    let (label, icon, binding) = parts(which, s);
    let mut props =
        MenuItemProps::new(&label).shortcut_opt(binding.and_then(|b| shortcut_label(map, b)));
    props.icon = icon;
    let mut e = match which {
        ContextItem::Show(o) => p.spawn((menu_checkbox(theme, props, s.overlays.is_on(o)), which)),
        _ => p.spawn((menu_item(theme, props), which)),
    };
    if let Some(why) = unavailable(which, s) {
        e.insert((InteractionDisabled, Tooltip::new(label).detail(why)));
    }
}

/// The menu's content (§7.9's order).
fn items(
    theme: Theme,
    map: InputMap,
    s: MenuState,
) -> impl FnOnce(&mut ChildSpawner) + Send + Sync {
    move |p| {
        use ContextItem::*;
        for i in [Focus, Follow, Track, MoveTo] {
            item(p, &theme, &map, i, &s);
        }
        p.spawn(menu_separator());
        for i in [
            Rename,
            Hide,
            Edit(ModelEdit::Copy),
            Edit(ModelEdit::Paste),
            Edit(ModelEdit::Duplicate),
            Edit(ModelEdit::Delete),
        ] {
            item(p, &theme, &map, i, &s);
        }
        p.spawn(menu_separator());
        let (t, m, st) = (theme.clone(), map.clone(), s.clone());
        p.spawn(submenu(&theme, "Align", Some(IconId::Align), move |a| {
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                a.spawn(menu_group_label(name));
                for edge in AlignEdge::ALL {
                    item(a, &t, &m, Align(axis, edge), &st);
                }
            }
        }));
        let (t, m, st) = (theme.clone(), map.clone(), s.clone());
        p.spawn(submenu(&theme, "Show", Some(IconId::Overlays), move |a| {
            for (o, _) in SHOWN {
                item(a, &t, &m, Show(o), &st);
            }
        }));
        item(p, &theme, &map, ApplyForce, &s);
        if s.model {
            item(p, &theme, &map, EditModel, &s);
        }
        p.spawn(menu_separator());
        item(p, &theme, &map, Inspect, &s);
    }
}

/// The world as the menu needs it.
#[allow(clippy::type_complexity)]
pub(crate) type Subjects<'w, 's> = Query<
    'w,
    's,
    (
        &'static GzEntity,
        &'static ChildOf,
        Option<&'static Visibility>,
        Has<HasPlane>,
    ),
>;

/// An [`EntityContext`] opens the menu at the pointer (one at a time).
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_context_menu(
    mut asks: MessageReader<EntityContext>,
    theme: Res<Theme>,
    map: Option<Res<InputMap>>,
    windows: Query<&Window>,
    ui_scale: Res<UiScale>,
    status: Option<Res<GzStatus>>,
    selection: Res<Selection>,
    clipboard: Option<Res<ModelClipboard>>,
    overlays: Option<Res<Overlays>>,
    sets: Query<&OverlaySet>,
    subjects: Subjects,
    roots: Query<Entity, With<GzWorldRoot>>,
    open: Query<Entity, With<ContextMenu>>,
    editing: Option<Res<EditingModel>>,
    mut commands: Commands,
) {
    let Some(ask) = asks.read().last() else {
        return;
    };
    let (Some(map), Ok(window)) = (map, windows.single()) else {
        return;
    };
    // At the pointer (without one, `--menu context`, a quarter down the
    // middle: the menu is tall), in UI pixels: the window's over the UI
    // scale.
    let at = window
        .cursor_position()
        .unwrap_or(Vec2::new(window.width() / 2.0, window.height() / 4.0))
        / ui_scale.0.max(f32::EPSILON);
    for e in &open {
        commands.entity(e).despawn();
    }
    let root = roots.single().ok();
    let top_model = |e: Entity| {
        subjects
            .get(e)
            .is_ok_and(|(g, p, ..)| g.kind == NodeKind::Model && Some(p.parent()) == root)
    };
    let state = MenuState {
        model: top_model(ask.entity),
        hidden: subjects
            .get(ask.entity)
            .is_ok_and(|(_, _, v, _)| v == Some(&Visibility::Hidden)),
        edits: edits_allowed(status.as_deref()),
        copied: clipboard.is_some_and(|c| !c.is_empty()),
        movers: selection
            .entities
            .iter()
            .filter(|e| {
                subjects
                    .get(**e)
                    .is_ok_and(|(g, p, _, plane)| root.is_some_and(|r| movable(g, p, r, plane)))
            })
            .count(),
        overlays: ask
            .view
            .and_then(|v| sets.get(v).ok().map(|s| s.0))
            .or(overlays.as_deref().copied())
            .unwrap_or_default(),
        edited: editing.and_then(|e| e.model()) == Some(ask.entity),
    };
    let mut menu_root = Entity::PLACEHOLDER;
    let holder = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(at.x),
                top: px(at.y),
                width: px(0),
                height: px(0),
                ..default()
            },
            GlobalZIndex(900),
        ))
        .with_children(|c| {
            menu_root = c
                .spawn(menu(
                    &theme,
                    MenuSide::Below,
                    Node::default(),
                    items(theme.clone(), map.clone(), state),
                ))
                .id();
        })
        .id();
    commands.entity(holder).insert(ContextMenu {
        subject: ask.entity,
        view: ask.view,
        menu: menu_root,
        shown: false,
    });
    commands.trigger(MenuEvent {
        source: menu_root,
        action: MenuAction::Open(NavAction::First),
    });
}

/// A closed context menu goes (and takes the keys with it).
pub(crate) fn close_context_menu(
    mut menus: Query<(Entity, &mut ContextMenu)>,
    popups: Query<(&ChildOf, &Visibility), With<MenuPopupOf>>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    for (e, mut m) in &mut menus {
        let visible = popups
            .iter()
            .any(|(p, v)| p.parent() == m.menu && *v == Visibility::Visible);
        if visible {
            m.shown = true;
        } else if m.shown {
            let inside = focus
                .get()
                .is_some_and(|f| f == e || parents.iter_ancestors(f).any(|a| a == e));
            if inside {
                focus.clear();
            }
            commands.entity(e).despawn();
        }
    }
}

/// A right press on a Scene tree row asks for its entity's menu, selecting
/// it unless it already is.
pub(crate) fn tree_context(
    entity: Entity,
    selection: &mut ResMut<Selection>,
    out: &mut MessageWriter<EntityContext>,
) {
    if !selection.entities.contains(&entity) {
        gz_bevy::view::select(&mut selection.reborrow(), SelectMode::Replace, &[entity]);
    }
    out.write(EntityContext { entity, view: None });
}

/// Runs an item for the menu's subject.
#[allow(clippy::too_many_arguments)]
pub(crate) fn on_context_item(
    ev: On<Activate>,
    items: Query<&ContextItem, Without<InteractionDisabled>>,
    menus: Query<(Entity, &ContextMenu)>,
    parents: Query<&ChildOf>,
    cameras: Query<Entity, With<MainCamera>>,
    overlays: Option<Res<Overlays>>,
    sets: Query<&OverlaySet>,
    selection: Res<Selection>,
    mut visibility: Query<&mut Visibility, With<GzEntity>>,
    mut layout: ResMut<ShellLayout>,
    mut views: MessageWriter<ViewCommand>,
    mut overlay_out: MessageWriter<OverlayCommand>,
    mut edits: MessageWriter<EditModels>,
    mut aligns: MessageWriter<AlignModels>,
    mut records: MessageWriter<Record>,
    mut editing: MessageWriter<EditCommand>,
) {
    let Ok(item) = items.get(ev.entity) else {
        return;
    };
    let Some((_, menu)) = parents
        .iter_ancestors(ev.entity)
        .find_map(|a| menus.get(a).ok())
    else {
        return;
    };
    let subject = menu.subject;
    // The views a camera item acts in: the one it opened in, else all.
    let in_views: Vec<Entity> = match menu.view {
        Some(v) => vec![v],
        None => cameras.iter().collect(),
    };
    let mut to_views = |op: ViewOp, views_in: &[Entity]| {
        for &view in views_in {
            views.write(ViewCommand { view, op });
        }
    };
    match *item {
        ContextItem::Focus => to_views(ViewOp::Focus { at: None }, &in_views),
        ContextItem::MoveTo => {
            let all: Vec<Entity> = cameras.iter().collect();
            to_views(ViewOp::Focus { at: None }, &all);
        }
        ContextItem::Follow | ContextItem::Track => {
            let mode = if *item == ContextItem::Follow {
                CameraMode::Follow(subject)
            } else {
                CameraMode::Track(subject)
            };
            let first: Vec<Entity> = in_views.first().copied().into_iter().collect();
            to_views(ViewOp::SetMode(mode), &first);
        }
        ContextItem::Hide => {
            // The selection, which holds the subject.
            let hide = visibility
                .get(subject)
                .is_ok_and(|v| *v != Visibility::Hidden);
            let after = if hide {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            let mut changes = vec![];
            for &e in &selection.entities {
                if let Ok(mut v) = visibility.get_mut(e) {
                    let before = *v;
                    if before != after {
                        *v = after;
                        changes.push(Change::Visibility {
                            entity: e,
                            before,
                            after,
                        });
                    }
                }
            }
            if !changes.is_empty() {
                records.write(Record {
                    verb: if hide { "hide" } else { "show" },
                    named: true,
                    changes,
                    scrub: None,
                    open: false,
                });
            }
        }
        ContextItem::Edit(edit) => {
            edits.write(EditModels(edit));
        }
        ContextItem::Align(axis, edge) => {
            aligns.write(AlignModels { axis, edge });
        }
        ContextItem::Show(o) => {
            let on = menu
                .view
                .and_then(|v| sets.get(v).ok().map(|s| s.0))
                .or(overlays.as_deref().copied())
                .unwrap_or_default()
                .is_on(o);
            let cmd = OverlayCommand::set(o, !on);
            overlay_out.write(match menu.view {
                Some(v) => cmd.in_view(v),
                None => cmd,
            });
        }
        ContextItem::Inspect => layout.show(Side::Inspector, true),
        ContextItem::EditModel => {
            editing.write(EditCommand::Enter(subject));
        }
        ContextItem::Rename | ContextItem::ApplyForce => {}
    }
}

/// A finished copy, paste or duplicate says so in the status bar for 3 s;
/// a delete for 5 s, with an Undo (§7.1). A failed one says why (and
/// gz-bevy logs it).
pub(crate) fn models_edited(
    mut edited: MessageReader<ModelsEdited>,
    time: Res<Time<Real>>,
    undo: Option<Res<UndoStack>>,
    mut note: ResMut<StatusNote>,
) {
    let now = time.elapsed_secs_f64();
    for e in edited.read() {
        let text = match &e.result {
            Ok(text) => text,
            Err(why) => {
                note.show(why.clone(), now);
                continue;
            }
        };
        let undo = undo.as_deref();
        let undoes = undo
            .and_then(UndoStack::undo_serial)
            .zip(undo.and_then(UndoStack::undo_label));
        match (e.edit, undoes) {
            (ModelEdit::Delete, Some((n, l))) => {
                note.show_undo(text.clone(), (n, l.to_string()), now);
            }
            _ => note.show(text.clone(), now),
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_message::<EntityContext>()
        .add_message::<EditModels>()
        .add_message::<EditCommand>()
        .add_message::<ModelsEdited>()
        .add_message::<AlignModels>()
        .add_observer(on_context_item)
        .add_systems(
            Update,
            (open_context_menu, close_context_menu)
                .chain()
                .after(gz_bevy::view::apply_view_commands),
        )
        .add_systems(Update, models_edited.after(gz_bevy::undo::UndoSystems));
}

#[cfg(test)]
mod tests;
