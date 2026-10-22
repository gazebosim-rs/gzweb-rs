//! NumberField (§5.8): 32 tall, radius 6, 1px `line`, `field` fill,
//! padding 0 8, gap 6, mono 12/500. A leading tag (X/Y/Z/R/P/Yw in the
//! axis text colours, weight 600, or a short label in `text_2`), the value,
//! and an optional unit in `text_3`. Focus: `accent` border and a 3 px
//! `accent_soft` halo.
//!
//! Editing is Bevy's headless text input (selection, clipping, clipboard,
//! IME; DECISIONS.md D-27) behind this wrapper. Enter or blur evaluates the
//! text with [`expr::eval`] and commits; Esc reverts. Text the user didn't
//! edit commits nothing: it only catches up with the current value (spec
//! §13: blur without change resumes streaming). Dragging the tag scrubs:
//! one `step` per pixel, ×10 with Shift, ÷10 with Alt, from the modifiers
//! held at each move. Commits are [`NumberCommit`] events on the field.

use accesskit::Role;
use bevy::a11y::AccessibilityNode;
use bevy::ecs::spawn::SpawnWith;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusLost, FocusedInput, InputFocus};
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::prelude::*;
use bevy::text::{EditableText, LineBreak, TextEdit};
use bevy::ui_widgets::{SelectAllOnFocus, TextInput};
use bevy::window::SystemCursorIcon;

pub(crate) mod expr;

use super::{a11y, rounded};
use crate::assets::fonts::{text, Type};
use crate::theme::{
    border, radius, CaretRoles, ColorRole, Fill, FontFamily, Ink, Ring, Stroke, Theme, TypeStyle,
    SPACE,
};
use crate::GzUiSystems;
use expr::ExprError;
pub use expr::Unit;

/// Value type: mono 12/500.
pub(crate) const VALUE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 500);
/// Tag type: mono 12/600 (Geist Mono's heaviest file is 500; D-25).
pub(crate) const TAG_TYPE: TypeStyle = TypeStyle::new(FontFamily::Mono, 12.0, 600);

/// A field's leading tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AxisTag {
    X,
    Y,
    Z,
    Roll,
    Pitch,
    Yaw,
    /// A short label ("mass").
    Label(String),
}

impl AxisTag {
    pub fn tag(&self) -> &str {
        match self {
            AxisTag::X => "X",
            AxisTag::Y => "Y",
            AxisTag::Z => "Z",
            AxisTag::Roll => "R",
            AxisTag::Pitch => "P",
            AxisTag::Yaw => "Yw",
            AxisTag::Label(l) => l,
        }
    }

    /// The accessible name: the axis or angle spelled out, or the label.
    pub fn name(&self) -> &str {
        match self {
            AxisTag::Roll => "Roll",
            AxisTag::Pitch => "Pitch",
            AxisTag::Yaw => "Yaw",
            other => other.tag(),
        }
    }

    /// R→x, P→y, Yw→z colours; labels in `text_2`.
    pub fn role(&self) -> ColorRole {
        match self {
            AxisTag::X | AxisTag::Roll => ColorRole::AxisXText,
            AxisTag::Y | AxisTag::Pitch => ColorRole::AxisYText,
            AxisTag::Z | AxisTag::Yaw => ColorRole::AxisZText,
            AxisTag::Label(_) => ColorRole::Text2,
        }
    }
}

/// A number field's value and rules (on the field's root node).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct NumberField {
    pub value: f64,
    /// Scrub step per pixel.
    pub step: f64,
    /// Decimals shown.
    pub precision: usize,
    pub unit: Unit,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The accessible name, if not the tag's ("Position X").
    pub name: Option<String>,
}

impl NumberField {
    pub fn new(value: f64) -> Self {
        Self {
            value,
            step: 0.01,
            precision: 3,
            unit: Unit::None,
            min: None,
            max: None,
            name: None,
        }
    }

    pub fn unit(mut self, unit: Unit) -> Self {
        self.unit = unit;
        self
    }

    pub fn precision(mut self, precision: usize, step: f64) -> Self {
        self.precision = precision;
        self.step = step;
        self
    }

    /// The accessible name, when the tag alone is ambiguous.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// `v` within the range.
    pub fn clamp(&self, v: f64) -> f64 {
        let v = self.min.map_or(v, |m| v.max(m));
        self.max.map_or(v, |m| v.min(m))
    }

    /// The value as the field shows it.
    pub fn format(&self, v: f64) -> String {
        let s = format!("{v:.*}", self.precision);
        // No "-0.000".
        if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
            s[1..].to_string()
        } else {
            s
        }
    }

    /// What typing `text` and committing gives: the clamped value, or why
    /// not.
    pub fn parse(&self, text: &str) -> Result<f64, ExprError> {
        expr::eval(text, self.unit).map(|v| self.clamp(v))
    }

    /// The value a scrub move of `dx` pixels from `from` gives, with the
    /// modifiers held during that move.
    pub fn scrub(&self, from: f64, dx: f32, shift: bool, alt: bool) -> f64 {
        let k = match (shift, alt) {
            (true, false) => 10.0,
            (false, true) => 0.1,
            _ => 1.0,
        };
        self.clamp(from + f64::from(dx) * self.step * k)
    }
}

/// A field's value was committed (Enter, blur or a scrub). `is_final` is
/// false during a scrub drag.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct NumberCommit {
    pub entity: Entity,
    pub value: f64,
    pub is_final: bool,
}

/// The editable text of a field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct NumberInput;

/// The text the field last showed. Text that still equals it wasn't edited,
/// so Enter or blur commits nothing.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShownText(pub String);

/// The scrub handle (the tag).
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct NumberTag;

/// A scrub in progress: its value so far, and whether it has committed
/// one. Each move adds to it with the modifiers held then, so pressing
/// Shift mid-drag doesn't jump.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scrub {
    pub value: f64,
    pub moved: bool,
}

/// Set while the text doesn't evaluate (the border turns `danger`).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Invalid(pub ExprError);

/// A number field: `tag`, the value, and the unit suffix if any. Pass
/// `compact` for 30 px panel filter bars.
pub fn number_field(theme: &Theme, tag: AxisTag, field: NumberField, compact: bool) -> impl Bundle {
    let h = if compact {
        theme.s.input_h_compact
    } else {
        theme.s.input_h
    };
    let shown = field.format(field.value);
    let unit = field.unit.suffix();
    let mut access = a11y(
        Role::SpinButton,
        field.name.clone().unwrap_or_else(|| tag.name().to_string()),
    );
    access.set_numeric_value(field.value);
    (
        Node {
            height: px(h),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            padding: UiRect::horizontal(px(SPACE[4])),
            column_gap: px(SPACE[3]),
            border: UiRect::all(px(border::HAIRLINE)),
            align_items: AlignItems::Center,
            border_radius: rounded(radius::SM),
            ..default()
        },
        Fill(ColorRole::Field),
        Stroke(ColorRole::Line),
        Ring(ColorRole::Clear),
        Outline::new(px(border::FOCUS_HALO), px(0), Color::NONE),
        field,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            p.spawn((
                NumberTag,
                text(TAG_TYPE, tag.role(), tag.tag().to_string()),
                bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::EwResize),
            ));
            p.spawn((
                NumberInput,
                TextInput,
                EditableText::new(shown.clone()),
                ShownText(shown),
                SelectAllOnFocus,
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                TextFont::from_font_size(VALUE_TYPE.size),
                Type(VALUE_TYPE),
                Ink(ColorRole::Text),
                CaretRoles,
                TabIndex(0),
                access,
                bevy::picking::cursor::EntityCursor::System(SystemCursorIcon::Text),
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    ..default()
                },
            ));
            if let Some(unit) = unit {
                p.spawn(text(VALUE_TYPE, ColorRole::Text3, unit));
            }
        })),
    )
}

fn set_text(editable: &mut EditableText, shown: &mut ShownText, s: &str) {
    if editable.value() != s {
        editable.editor.set_text(s);
        editable.queue_edit(TextEdit::TextEnd(false));
    }
    if shown.0 != s {
        shown.0 = s.to_string();
    }
}

type InputQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut EditableText,
        &'static mut ShownText,
        &'static ChildOf,
    ),
    With<NumberInput>,
>;

/// Evaluates an input's text into its field: commits a valid value and
/// reformats, or marks the field [`Invalid`]. `revert_on_error` (blur)
/// restores the old value instead. Unedited text commits nothing; it shows
/// the current value again (it may have changed while the field had focus).
fn commit(
    input: Entity,
    revert_on_error: bool,
    inputs: &mut InputQuery,
    fields: &mut Query<&mut NumberField>,
    commands: &mut Commands,
) {
    let Ok((mut editable, mut shown, parent)) = inputs.get_mut(input) else {
        return;
    };
    let root = parent.parent();
    let Ok(mut field) = fields.get_mut(root) else {
        return;
    };
    let text = editable.value().to_string();
    if text == shown.0 {
        commands.entity(root).remove::<Invalid>();
        let current = field.format(field.value);
        set_text(&mut editable, &mut shown, &current);
        return;
    }
    match field.parse(&text) {
        Ok(v) => {
            commands.entity(root).remove::<Invalid>();
            let changed = v != field.value;
            field.value = v;
            let formatted = field.format(v);
            set_text(&mut editable, &mut shown, &formatted);
            if changed {
                commands.trigger(NumberCommit {
                    entity: root,
                    value: v,
                    is_final: true,
                });
            }
        }
        Err(_) if revert_on_error => {
            commands.entity(root).remove::<Invalid>();
            let formatted = field.format(field.value);
            set_text(&mut editable, &mut shown, &formatted);
        }
        Err(e) => {
            commands.entity(root).insert(Invalid(e));
        }
    }
}

/// Enter commits; Esc reverts (Bevy's text input then drops focus). Runs
/// once, on the input itself, not again as the key bubbles up.
pub(crate) fn on_number_key(
    ev: On<FocusedInput<KeyboardInput>>,
    mut inputs: InputQuery,
    mut fields: Query<&mut NumberField>,
    mut commands: Commands,
) {
    if ev.input.state != ButtonState::Pressed
        || ev.event_target() != ev.focused_entity
        || !inputs.contains(ev.focused_entity)
    {
        return;
    }
    match ev.input.logical_key {
        Key::Enter => commit(
            ev.focused_entity,
            false,
            &mut inputs,
            &mut fields,
            &mut commands,
        ),
        Key::Escape => {
            if let Ok((mut editable, mut shown, parent)) = inputs.get_mut(ev.focused_entity) {
                if let Ok(field) = fields.get(parent.parent()) {
                    let formatted = field.format(field.value);
                    set_text(&mut editable, &mut shown, &formatted);
                    commands.entity(parent.parent()).remove::<Invalid>();
                }
            }
        }
        _ => {}
    }
}

/// Blur commits an edit (or reverts text that doesn't evaluate).
pub(crate) fn on_number_blur(
    ev: On<FocusLost>,
    mut inputs: InputQuery,
    mut fields: Query<&mut NumberField>,
    mut commands: Commands,
) {
    if ev.event_target() == ev.entity {
        commit(ev.entity, true, &mut inputs, &mut fields, &mut commands);
    }
}

/// Scrub start: remember the value.
pub(crate) fn on_scrub_start(
    ev: On<PointerDragStart>,
    tags: Query<&ChildOf, With<NumberTag>>,
    fields: Query<&NumberField>,
    mut commands: Commands,
) {
    if let Ok(parent) = tags.get(ev.entity) {
        if let Ok(field) = fields.get(parent.parent()) {
            commands.entity(parent.parent()).insert(Scrub {
                value: field.value,
                moved: false,
            });
        }
    }
}

/// Scrubbing: each horizontal move adds `step` per pixel, scaled by the
/// modifiers held now.
pub(crate) fn on_scrub(
    ev: On<PointerDrag>,
    keys: Res<ButtonInput<KeyCode>>,
    tags: Query<&ChildOf, With<NumberTag>>,
    mut fields: Query<(&mut NumberField, &mut Scrub)>,
    mut commands: Commands,
) {
    let Ok(parent) = tags.get(ev.entity) else {
        return;
    };
    let root = parent.parent();
    let Ok((mut field, mut scrub)) = fields.get_mut(root) else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let v = field.scrub(scrub.value, ev.delta.x, shift, alt);
    scrub.value = v;
    if v != field.value {
        field.value = v;
        scrub.moved = true;
        commands.trigger(NumberCommit {
            entity: root,
            value: v,
            is_final: false,
        });
    }
}

/// Scrub end: a final commit of the scrub's value (the field's may have
/// been set from outside since the last move, by a value streaming in),
/// unless it never changed the value.
pub(crate) fn on_scrub_end(
    ev: On<PointerDragEnd>,
    tags: Query<&ChildOf, With<NumberTag>>,
    mut fields: Query<(&mut NumberField, &Scrub)>,
    mut commands: Commands,
) {
    if let Ok(parent) = tags.get(ev.entity) {
        let root = parent.parent();
        if let Ok((mut field, scrub)) = fields.get_mut(root) {
            if field.value != scrub.value {
                field.value = scrub.value;
            }
            commands.entity(root).remove::<Scrub>();
            if scrub.moved {
                commands.trigger(NumberCommit {
                    entity: root,
                    value: scrub.value,
                    is_final: true,
                });
            }
        }
    }
}

/// Shows a changed value in an unfocused field (scrubs, programmatic sets)
/// and gives it to AccessKit. A focused field's text stays as it is.
pub(crate) fn sync_number_text(
    focus: Res<InputFocus>,
    fields: Query<(&NumberField, &Children), Changed<NumberField>>,
    mut inputs: Query<
        (
            Entity,
            &mut EditableText,
            &mut ShownText,
            &mut AccessibilityNode,
        ),
        With<NumberInput>,
    >,
) {
    for (field, children) in &fields {
        for &c in children {
            if let Ok((e, mut editable, mut shown, mut access)) = inputs.get_mut(c) {
                if access.numeric_value() != Some(field.value) {
                    access.set_numeric_value(field.value);
                }
                if focus.get() != Some(e) {
                    set_text(&mut editable, &mut shown, &field.format(field.value));
                }
            }
        }
    }
}

/// Border and halo: `accent` + `accent_soft` while the input has focus,
/// `danger` while the text doesn't evaluate, else `line`. Runs for every
/// field when focus moves, else only for fields that are new or whose
/// [`Invalid`] state changed.
#[allow(clippy::type_complexity)]
pub(crate) fn number_focus_system(
    focus: Res<InputFocus>,
    mut fields: Query<(&Children, &mut Stroke, &mut Ring, Has<Invalid>), With<NumberField>>,
    changed: Query<Entity, Or<(Added<NumberField>, Added<Invalid>)>>,
    mut valid_again: RemovedComponents<Invalid>,
    inputs: Query<(), With<NumberInput>>,
    mut dirty: Local<Vec<Entity>>,
) {
    dirty.clear();
    dirty.extend(changed.iter().chain(valid_again.read()));
    let mut style =
        |(children, mut stroke, mut ring, invalid): (&Children, Mut<Stroke>, Mut<Ring>, bool)| {
            let focused = children
                .iter()
                .any(|c| inputs.contains(c) && focus.get() == Some(c));
            let border = match (invalid, focused) {
                (true, _) => ColorRole::Danger,
                (false, true) => ColorRole::Accent,
                (false, false) => ColorRole::Line,
            };
            stroke.set_if_neq(Stroke(border));
            ring.set_if_neq(Ring(if focused {
                ColorRole::AccentSoft
            } else {
                ColorRole::Clear
            }));
        };
    if focus.is_changed() {
        fields.iter_mut().for_each(&mut style);
    } else {
        for &e in &*dirty {
            if let Ok(item) = fields.get_mut(e) {
                style(item);
            }
        }
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_observer(on_number_key)
        .add_observer(on_number_blur)
        .add_observer(on_scrub_start)
        .add_observer(on_scrub)
        .add_observer(on_scrub_end)
        .add_systems(
            Update,
            (sync_number_text, number_focus_system).in_set(GzUiSystems::Behaviour),
        );
}

#[cfg(test)]
mod tests;
