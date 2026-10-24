//! The Save as… and Export world as SDF… dialog (spec §7.1; DECISIONS.md
//! D-108): a centered card over everything with the file name (the
//! world's, to start), "Use current poses" (off for Save as…, which
//! writes the authored world; on for Export, the server's poses), Cancel
//! and Save. Enter saves, Esc or Cancel closes. Save as… marks the world
//! saved; Export leaves it as it was.

use accesskit::Role;
use bevy::ecs::spawn::SpawnWith;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{checkbox_self_update, observe, Activate};
use gz_bevy::world_save::SaveWorld;

use crate::assets::fonts::text;
use crate::theme::{
    border, elevation, radius, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE,
};
use crate::widgets::focus::TakesEscape;
use crate::widgets::layout::{column, flex_spacer, row};
use crate::widgets::text_field::TextFieldInput;
use crate::widgets::{a11y, button, rounded, text_field, toggle_row, ButtonVariant, TextSubmit};

/// The dialog's width.
const WIDTH: f32 = 400.0;

/// Which dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogMode {
    /// Save as…: the authored world unless "Use current poses"; marks the
    /// world saved.
    SaveAs,
    /// Export world as SDF…: the server's poses unless unticked; the world
    /// stays dirty.
    Export,
}

impl DialogMode {
    fn title(self) -> &'static str {
        match self {
            DialogMode::SaveAs => "Save as",
            DialogMode::Export => "Export world as SDF",
        }
    }
}

/// The open dialog, if any: its mode and the name it starts with.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SaveDialog(pub Option<(DialogMode, String)>);

impl SaveDialog {
    pub(crate) fn open(&mut self, mode: DialogMode, world: Option<&str>) {
        self.0 = Some((mode, super::world_menu::default_name(world)));
    }
}

/// The dialog's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DialogRoot;

/// Its name field's root.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct DialogName;

/// Its "Use current poses" toggle.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CurrentPoses;

/// Its buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogButton {
    Cancel,
    Save,
}

fn dialog(theme: &Theme, mode: DialogMode, name: &str) -> impl Bundle {
    let (shadow, tint) = elevation::MENU.shadow();
    let t = theme.clone();
    let name = name.to_string();
    (
        DialogRoot,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::new(px(SPACE[6]), px(SPACE[6]), Val::Vh(20.0), px(0)),
            align_items: AlignItems::Center,
            ..column(0.0)
        },
        GlobalZIndex(950),
        TakesEscape,
        children![(
            Node {
                width: px(WIDTH),
                max_width: Val::Percent(100.0),
                padding: UiRect::all(px(SPACE[7])),
                border: UiRect::all(px(border::HAIRLINE)),
                border_radius: rounded(radius::LG),
                ..column(SPACE[6])
            },
            Fill(ColorRole::Surface),
            Stroke(ColorRole::Line),
            shadow,
            tint,
            a11y(Role::Dialog, mode.title()),
            TabGroup::modal(),
            Children::spawn(SpawnWith(move |c: &mut ChildSpawner| {
                c.spawn(text(
                    TypeStyle::new(FontFamily::Ui, 15.0, 600),
                    ColorRole::Text,
                    mode.title(),
                ));
                c.spawn((text_field(&t, "File name", name), DialogName));
                c.spawn((
                    toggle_row(&t, "Use current poses", mode == DialogMode::Export),
                    CurrentPoses,
                    observe(checkbox_self_update),
                ));
                c.spawn(text(
                    TypeStyle::SMALL,
                    ColorRole::Text3,
                    "Off: each model where it was loaded or last placed, not where physics took it",
                ));
                c.spawn(row(SPACE[4])).with_children(|r| {
                    r.spawn(flex_spacer());
                    r.spawn((
                        button(&t, "Cancel", ButtonVariant::Normal),
                        DialogButton::Cancel,
                    ));
                    r.spawn((
                        button(&t, "Save", ButtonVariant::Normal),
                        DialogButton::Save,
                    ));
                });
            })),
        )],
    )
}

/// The dialog opens and closes with [`SaveDialog`]; open, its name field
/// takes the keys.
pub(crate) fn sync_dialog(
    state: Res<SaveDialog>,
    theme: Res<Theme>,
    roots: Query<Entity, With<DialogRoot>>,
    mut commands: Commands,
) {
    if !state.is_changed() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    if let Some((mode, name)) = &state.0 {
        commands.spawn(dialog(&theme, *mode, name));
    }
}

/// The name field takes the keys once it exists.
pub(crate) fn focus_name(
    fields: Query<(Entity, &ChildOf), Added<TextFieldInput>>,
    names: Query<(), With<DialogName>>,
    mut focus: ResMut<InputFocus>,
) {
    for (e, parent) in &fields {
        if names.contains(parent.parent()) {
            focus.set(e, FocusCause::Navigated);
        }
    }
}

/// What the dialog's controls hold, to save with.
fn chosen(
    names: &Query<(), With<DialogName>>,
    inputs: &Query<(&EditableText, &ChildOf), With<TextFieldInput>>,
    poses: &Query<Has<Checked>, With<CurrentPoses>>,
) -> (String, bool) {
    let name = inputs
        .iter()
        .find(|(_, p)| names.contains(p.parent()))
        .map(|(t, _)| t.value().to_string())
        .unwrap_or_default();
    let current = poses.iter().next().unwrap_or(false);
    (name, current)
}

/// Saves with what the dialog holds (a name without `.sdf` gets it) and
/// closes it; an empty name does nothing.
fn save(
    state: &mut SaveDialog,
    name: String,
    current_poses: bool,
    saves: &mut MessageWriter<SaveWorld>,
) {
    let Some((mode, _)) = state.0 else {
        return;
    };
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let name = if name.ends_with(".sdf") {
        name.to_string()
    } else {
        format!("{name}.sdf")
    };
    saves.write(SaveWorld {
        name,
        current_poses,
        mark_saved: mode == DialogMode::SaveAs,
    });
    state.0 = None;
}

/// Cancel closes; Save saves.
pub(crate) fn on_dialog_button(
    ev: On<Activate>,
    buttons: Query<&DialogButton>,
    names: Query<(), With<DialogName>>,
    inputs: Query<(&EditableText, &ChildOf), With<TextFieldInput>>,
    poses: Query<Has<Checked>, With<CurrentPoses>>,
    mut state: ResMut<SaveDialog>,
    mut saves: MessageWriter<SaveWorld>,
) {
    match buttons.get(ev.entity) {
        Ok(DialogButton::Cancel) => state.0 = None,
        Ok(DialogButton::Save) => {
            let (name, current) = chosen(&names, &inputs, &poses);
            save(&mut state, name, current, &mut saves);
        }
        Err(_) => {}
    }
}

/// Enter in the name field saves.
pub(crate) fn on_name_submit(
    ev: On<TextSubmit>,
    names: Query<(), With<DialogName>>,
    poses: Query<Has<Checked>, With<CurrentPoses>>,
    mut state: ResMut<SaveDialog>,
    mut saves: MessageWriter<SaveWorld>,
) {
    if names.contains(ev.entity) {
        let current = poses.iter().next().unwrap_or(false);
        save(&mut state, ev.value.clone(), current, &mut saves);
    }
}

/// Esc in the dialog closes it.
pub(crate) fn on_dialog_key(
    ev: On<FocusedInput<KeyboardInput>>,
    roots: Query<(), With<DialogRoot>>,
    parents: Query<&ChildOf>,
    mut state: ResMut<SaveDialog>,
) {
    if ev.input.state != ButtonState::Pressed || ev.input.key_code != KeyCode::Escape {
        return;
    }
    let inside = parents
        .iter_ancestors(ev.focused_entity)
        .any(|a| roots.contains(a));
    if inside && state.0.is_some() {
        state.0 = None;
    }
}
