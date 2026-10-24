//! TextField: the input look of the NumberField (§5.8: 32 tall, radius 6,
//! 1px `line`, `field` fill, padding 0 8) holding free text, for the
//! connect card's address. Editing is Bevy's headless text input
//! (DECISIONS.md D-27). Enter fires [`TextSubmit`] on the field.

use accesskit::Role;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::{EditableText, LineBreak, TextEdit};
use bevy::ui_widgets::TextInput;
use bevy::window::SystemCursorIcon;

use super::{a11y, rounded};
use crate::assets::fonts::text;
use crate::assets::fonts::Type;
use crate::theme::{
    border, radius, CaretRoles, ColorRole, Fill, FontFamily, Ink, Ring, Stroke, Theme, TypeStyle,
    SPACE,
};
use bevy::ecs::spawn::SpawnWith;

/// Field text: mono 12/400 (addresses and paths).
pub(crate) const FIELD_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 400);

/// The editable text of a [`text_field`].
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TextFieldInput;

/// Enter was pressed in a text field: `entity` is the field's root.
#[derive(EntityEvent, Clone, Debug, PartialEq, Eq)]
pub struct TextSubmit {
    pub entity: Entity,
    pub value: String,
}

/// A field's placeholder: shown while its text is empty (Bevy 0.20's text
/// input has none).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct FieldPlaceholder;

/// A one-line text field holding `value`; `name` is its accessible name.
pub fn text_field(theme: &Theme, name: impl Into<String>, value: impl Into<String>) -> impl Bundle {
    field(theme.s.input_h, FIELD_TYPE, name.into(), value.into(), None)
}

/// A panel's filter field (§3.3: inputs are 30 tall in panel filter bars):
/// Geist 13, with `placeholder` in `text_3` while it's empty.
pub fn filter_field(
    theme: &Theme,
    name: impl Into<String>,
    placeholder: impl Into<String>,
    value: impl Into<String>,
) -> impl Bundle {
    field(
        theme.s.input_h_compact,
        TypeStyle::BODY,
        name.into(),
        value.into(),
        Some(placeholder.into()),
    )
}

/// A command line (§3.3: the Console's footer input, mono 12): as tall as
/// a panel's filter, with `placeholder` in `text_3` while it's empty.
pub(crate) fn command_field(
    theme: &Theme,
    name: impl Into<String>,
    placeholder: impl Into<String>,
) -> impl Bundle {
    field(
        theme.s.input_h_compact,
        FIELD_TYPE,
        name.into(),
        String::new(),
        Some(placeholder.into()),
    )
}

fn field(
    height: f32,
    style: TypeStyle,
    name: String,
    value: String,
    placeholder: Option<String>,
) -> impl Bundle {
    let empty = value.is_empty();
    (
        Node {
            height: px(height),
            flex_grow: 1.0,
            min_width: px(0),
            padding: UiRect::horizontal(px(SPACE[4])),
            border: UiRect::all(px(border::HAIRLINE)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            border_radius: rounded(radius::SM),
            ..default()
        },
        Fill(ColorRole::Field),
        Stroke(ColorRole::Line),
        Ring(ColorRole::Clear),
        Outline::new(px(border::FOCUS_HALO), px(0), Color::NONE),
        Children::spawn(SpawnWith(move |f: &mut ChildSpawner| {
            if let Some(placeholder) = placeholder {
                f.spawn((
                    FieldPlaceholder,
                    text(style, ColorRole::Text3, placeholder),
                    TextLayout::no_wrap(),
                    Pickable::IGNORE,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(SPACE[4]),
                        display: if empty { Display::Flex } else { Display::None },
                        ..default()
                    },
                ));
            }
            f.spawn((
                TextFieldInput,
                TextInput,
                EditableText::new(value),
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                TextFont::from_font_size(style.size),
                Type(style),
                Ink(ColorRole::Text),
                CaretRoles,
                TabIndex(0),
                a11y(Role::TextInput, name),
                bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::Text),
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    ..default()
                },
            ));
        })),
    )
}

/// Replaces a field's text, the caret at its end.
pub(crate) fn set_field_text(editable: &mut EditableText, s: &str) {
    if editable.value() != s {
        editable.editor.set_text(s);
        editable.queue_edit(TextEdit::TextEnd(false));
    }
}

/// A field's text, edited.
type EditedField = (With<TextFieldInput>, Changed<EditableText>);

/// A placeholder shows while its field is empty.
pub(crate) fn placeholder_system(
    inputs: Query<(&EditableText, &ChildOf), EditedField>,
    children: Query<&Children>,
    mut placeholders: Query<&mut Node, With<FieldPlaceholder>>,
) {
    for (text, parent) in &inputs {
        let empty = text.value().to_string().is_empty();
        for &c in children.get(parent.parent()).into_iter().flatten() {
            if let Ok(mut node) = placeholders.get_mut(c) {
                super::layout::set_shown(&mut node, empty);
            }
        }
    }
}

/// Enter submits, once: on the input, not again as the key bubbles up.
pub(crate) fn on_text_field_key(
    ev: On<FocusedInput<KeyboardInput>>,
    inputs: Query<(&EditableText, &ChildOf), With<TextFieldInput>>,
    mut commands: Commands,
) {
    if ev.input.state != ButtonState::Pressed
        || ev.input.logical_key != Key::Enter
        || ev.event_target() != ev.focused_entity
    {
        return;
    }
    if let Ok((text, parent)) = inputs.get(ev.focused_entity) {
        commands.trigger(TextSubmit {
            entity: parent.parent(),
            value: text.value().to_string(),
        });
    }
}

/// Focus look: `accent` border and the `accent_soft` halo, as the number
/// field has. Runs when focus moves or a field is new.
pub(crate) fn text_field_focus_system(
    focus: Res<bevy::input_focus::InputFocus>,
    inputs: Query<(Entity, &ChildOf), With<TextFieldInput>>,
    added: Query<(), Added<TextFieldInput>>,
    mut fields: Query<(&mut Stroke, &mut Ring)>,
) {
    if !focus.is_changed() && added.is_empty() {
        return;
    }
    for (input, parent) in &inputs {
        let focused = focus.get() == Some(input);
        if let Ok((mut stroke, mut ring)) = fields.get_mut(parent.parent()) {
            stroke.set_if_neq(Stroke(if focused {
                ColorRole::Accent
            } else {
                ColorRole::Line
            }));
            ring.set_if_neq(Ring(if focused {
                ColorRole::AccentSoft
            } else {
                ColorRole::Clear
            }));
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_text_field_key).add_systems(
        Update,
        (text_field_focus_system, placeholder_system).in_set(crate::GzUiSystems::Visuals),
    );
}
