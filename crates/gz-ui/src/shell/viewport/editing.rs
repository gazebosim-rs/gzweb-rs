//! The EDITING pill (spec §6.6; DECISIONS.md D-112): while a model is
//! edited it floats top-centre of the active view, `hud` fill, 36 tall,
//! radius 9: an `EDITING` tag, the model's name, "· 3 links · 2 joints ·
//! sim paused", and Done (`primary`), which writes the parts added and
//! leaves editing (D-114). While there are parts to write, Discard
//! (bordered) leaves without them. What writing says ([`EditNote`]) shows
//! in the status bar.

use accesskit::Role;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as Headless};
use gz_bevy::connection::GzStatus;
use gz_bevy::overlays::extras::SdfExtras;
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::editing::parts::{EditNote, EditParts};
use gz_bevy::view::editing::{EditCommand, EditingModel};
use gz_bevy::view::ViewId;
use gz_core::scene::NodeKind;

use super::views::ViewCameras;
use crate::assets::fonts::text;
use crate::shell::status_bar::readouts::StatusNote;
use crate::theme::visuals::{Content, Visuals};
use crate::theme::{border, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE};
use crate::widgets::layout::{row, set_shown};
use crate::widgets::text::set_text;
use crate::widgets::{a11y, clickable, rounded};

/// The pill's height and radius (§6.6).
pub(crate) const PILL_H: f32 = 36.0;
const PILL_RADIUS: f32 = 9.0;

/// The tag's type: mono 10/600.
const TAG_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 10.0, 600);

/// The row that centres the pill across its view.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EditingBar;

/// The model's name.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EditingName;

/// "· 3 links · 2 joints · sim paused".
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EditingMeta;

/// The pill's Done.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EditingDone;

/// The pill's Discard, shown while there are parts to write.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EditingDiscard;

/// The pill, in a row across the view's top that lets the pointer through,
/// hidden until a model is edited.
pub(crate) fn editing_bar(theme: &Theme) -> impl Bundle {
    let inset = theme.s.viewport_inset;
    let visuals = {
        let mut v = Visuals::new(ColorRole::Primary, ColorRole::OnPrimary).keep_ink();
        v.hover_fill = ColorRole::Primary;
        v.pressed_fill = ColorRole::Text2;
        v
    };
    let bordered = Visuals::new(ColorRole::Surface2, ColorRole::Text)
        .stroked(ColorRole::Line)
        .keep_ink();
    let button_node = |border: f32| Node {
        height: px(PILL_H - 2.0 * SPACE[2]),
        padding: UiRect::horizontal(px(SPACE[5])),
        border: UiRect::all(px(border)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: rounded(PILL_RADIUS - SPACE[2] / 2.0),
        ..default()
    };
    (
        EditingBar,
        Name::new("editing pill"),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(inset),
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        Pickable::IGNORE,
        children![(
            Node {
                height: px(PILL_H),
                padding: UiRect::new(px(SPACE[4]), px(SPACE[2]), px(0), px(0)),
                border: UiRect::all(px(border::HAIRLINE)),
                border_radius: rounded(PILL_RADIUS),
                ..row(SPACE[4])
            },
            Fill(ColorRole::Hud),
            Stroke(ColorRole::Line),
            a11y(Role::Toolbar, "Editing"),
            children![
                (
                    Node {
                        padding: UiRect::axes(px(SPACE[2]), px(1)),
                        border_radius: rounded(4.0),
                        ..default()
                    },
                    Fill(ColorRole::AccentSoft),
                    Pickable::IGNORE,
                    children![(
                        text(TAG_TYPE, ColorRole::AccentText, "EDITING"),
                        Pickable::IGNORE
                    )],
                ),
                (
                    text(
                        TypeStyle::new(FontFamily::Ui, 13.0, 600),
                        ColorRole::Text,
                        ""
                    ),
                    EditingName,
                    Pickable::IGNORE,
                ),
                (
                    text(
                        TypeStyle::new(FontFamily::Ui, 12.0, 400),
                        ColorRole::Text2,
                        ""
                    ),
                    EditingMeta,
                    Pickable::IGNORE,
                ),
                (
                    EditingDiscard,
                    Node {
                        display: Display::None,
                        ..button_node(border::HAIRLINE)
                    },
                    Headless,
                    clickable(bordered, Role::Button, "Discard parts"),
                    children![(
                        text(
                            crate::widgets::button::BUTTON_TYPE,
                            ColorRole::Text,
                            "Discard"
                        ),
                        Content
                    )],
                ),
                (
                    EditingDone,
                    button_node(0.0),
                    Headless,
                    clickable(visuals, Role::Button, "Done editing"),
                    children![(
                        text(
                            crate::widgets::button::BUTTON_TYPE,
                            ColorRole::OnPrimary,
                            "Done"
                        ),
                        Content
                    )],
                ),
            ],
        )],
    )
}

/// "· 3 links · 2 joints", and "· sim paused" while it is.
pub(crate) fn meta_text(links: usize, joints: usize, paused: bool) -> String {
    let plural = |n: usize, one: &str| {
        if n == 1 {
            format!("{n} {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    let mut s = format!("· {} · {}", plural(links, "link"), plural(joints, "joint"));
    if paused {
        s.push_str(" · sim paused");
    }
    s
}

/// The pill shows on the active view while a model is edited, with its
/// name, its links and joints (the joints the world's SDFormat declares in
/// it), whether the sim is paused, and Discard while there are parts to
/// write.
#[allow(clippy::too_many_arguments)]
pub(crate) fn editing_pill_state(
    editing: Option<Res<EditingModel>>,
    parts: Option<Res<EditParts>>,
    routing: Res<ViewRouting>,
    status: Option<Res<GzStatus>>,
    extras: Option<Res<SdfExtras>>,
    entities: Query<&GzEntity>,
    children: Query<&Children>,
    cameras: ViewCameras,
    mut bars: Query<(Entity, &mut Node), With<EditingBar>>,
    mut discards: Query<&mut Node, (With<EditingDiscard>, Without<EditingBar>)>,
    mut names: Query<&mut Text, (With<EditingName>, Without<EditingMeta>)>,
    mut metas: Query<&mut Text, (With<EditingMeta>, Without<EditingName>)>,
) {
    let model = editing.as_deref().and_then(EditingModel::model);
    let active = routing.active.unwrap_or(ViewId(0));
    for (e, mut node) in &mut bars {
        set_shown(
            &mut node,
            model.is_some() && cameras.id_of(e) == Some(active),
        );
    }
    let holds = parts.as_deref().is_some_and(EditParts::holds);
    for mut node in &mut discards {
        set_shown(&mut node, holds);
    }
    let Some(model) = model else {
        return;
    };
    let Ok(gz) = entities.get(model) else {
        return;
    };
    let links = children
        .iter_descendants(model)
        .filter(|d| entities.get(*d).is_ok_and(|g| g.kind == NodeKind::Link))
        .count();
    let nested = format!("{}/", gz.path);
    let joints = extras
        .as_deref()
        .and_then(|x| x.extras.as_ref())
        .map_or(0, |x| {
            x.links
                .values()
                .flat_map(|l| &l.joints)
                .filter(|j| j.model == gz.path || j.model.starts_with(&nested))
                .count()
        });
    let paused = status
        .as_deref()
        .and_then(|s| s.sim.as_ref())
        .is_some_and(|s| s.paused);
    for mut t in &mut names {
        set_text(&mut t, &gz.name);
    }
    let meta = meta_text(links, joints, paused);
    for mut t in &mut metas {
        set_text(&mut t, &meta);
    }
}

/// Done writes the parts added and leaves editing; Discard leaves
/// without them.
pub(crate) fn on_done(
    ev: On<Activate>,
    dones: Query<(), With<EditingDone>>,
    discards: Query<(), With<EditingDiscard>>,
    mut out: MessageWriter<EditCommand>,
) {
    if dones.contains(ev.entity) {
        out.write(EditCommand::Done);
    } else if discards.contains(ev.entity) {
        out.write(EditCommand::Discard);
    }
}

/// What writing the model says (writing, written, or why not) shows in the
/// status bar for 3 s.
pub(crate) fn edit_notes(
    mut notes: MessageReader<EditNote>,
    time: Res<Time<Real>>,
    mut note: ResMut<StatusNote>,
) {
    for EditNote(text) in notes.read() {
        note.show(text.clone(), time.elapsed_secs_f64());
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn build(app: &mut App) {
    app.add_message::<EditCommand>()
        .init_resource::<EditingModel>()
        .init_resource::<EditParts>()
        .add_message::<gz_bevy::view::editing::parts::PartCommand>()
        .add_message::<gz_bevy::view::editing::parts::PartsDiscarded>()
        .add_message::<gz_bevy::view::editing::parts::WriteParts>()
        .add_message::<EditNote>()
        .init_resource::<gz_bevy::undo::Replacing>()
        .add_message::<gz_bevy::undo::UndoCommand>()
        .add_systems(
            Update,
            (editing_pill_state, edit_notes).in_set(crate::shell::ShellSystems::Readouts),
        )
        .add_observer(on_done);
}
