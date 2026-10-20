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
use bevy::text::{EditableText, LineBreak};
use bevy::ui_widgets::TextInput;
use bevy::window::SystemCursorIcon;

use super::{a11y, rounded};
use crate::assets::fonts::Type;
use crate::theme::{
    border, radius, CaretRoles, ColorRole, Fill, FontFamily, Ink, Ring, Stroke, Theme, TypeStyle,
    SPACE,
};

/// Field text: mono 12/400 (addresses and paths).
pub const FIELD_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 400);

/// The editable text of a [`text_field`].
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TextFieldInput;

/// Enter was pressed in a text field: `entity` is the field's root.
#[derive(EntityEvent, Clone, Debug, PartialEq, Eq)]
pub struct TextSubmit {
    pub entity: Entity,
    pub value: String,
}

/// A one-line text field holding `value`; `name` is its accessible name.
pub fn text_field(theme: &Theme, name: impl Into<String>, value: impl Into<String>) -> impl Bundle {
    (
        Node {
            height: px(theme.s.input_h),
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
        children![(
            TextFieldInput,
            TextInput,
            EditableText::new(value.into()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            TextFont::from_font_size(FIELD_TYPE.size),
            Type(FIELD_TYPE),
            Ink(ColorRole::Text),
            CaretRoles,
            TabIndex(0),
            a11y(Role::TextInput, name.into()),
            bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::Text),
            Node {
                flex_grow: 1.0,
                min_width: px(0),
                ..default()
            },
        )],
    )
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
        text_field_focus_system.in_set(crate::GzUiSystems::Visuals),
    );
}
