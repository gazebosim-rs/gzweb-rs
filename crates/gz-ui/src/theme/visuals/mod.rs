//! State visuals for clickable widgets (spec §4.3, §5 state table): one
//! system computes fill and content roles from the pointer state (Bevy
//! 0.20's `Hovered` and `Pressed`, which replace the old `Interaction`;
//! DECISIONS.md D-22) and the state markers [`Active`], `Checked` and
//! `InteractionDisabled`. No widget sets colours itself.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled, Pressed};

use crate::theme::{ColorRole, Fill, Ink, Opacity, Stroke};

/// The on / current state (spec §5 "Active / on"): an open panel's
/// activity button, the active tool, a selected row.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Active;

/// A child whose [`Ink`] follows its widget's state (icons, labels). The
/// nearest ancestor with [`Visuals`] drives it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Content;

/// [`Content`] that keeps its own colour and only takes the widget's
/// opacity: a menu item's shortcut (`text_3`) and leading icon (`text_2`)
/// and a checkbox item's box dim with the item when it's disabled.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OwnInk;

/// The roles a clickable widget uses in each state. [`Visuals::new`] fills
/// in the common table: hover → `hover` / `text`, pressed → `pressed` /
/// `text`, active → `accent_soft` / `accent_text`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visuals {
    pub fill: ColorRole,
    pub ink: ColorRole,
    pub hover_fill: ColorRole,
    pub hover_ink: ColorRole,
    pub pressed_fill: ColorRole,
    pub active_fill: ColorRole,
    pub active_ink: ColorRole,
    /// Border in the default state, if the widget has one.
    pub stroke: Option<ColorRole>,
    /// Border while active (activity buttons: inset `accent_ring`).
    pub active_stroke: Option<ColorRole>,
    /// `Checked` (radio, checkbox) shows the active look. Off for menu
    /// checkbox items, whose box shows the state.
    pub checked_is_active: bool,
}

impl Visuals {
    /// The common state table with this default fill and content colour.
    pub const fn new(fill: ColorRole, ink: ColorRole) -> Self {
        Self {
            fill,
            ink,
            hover_fill: ColorRole::Hover,
            hover_ink: ColorRole::Text,
            pressed_fill: ColorRole::Pressed,
            active_fill: ColorRole::AccentSoft,
            active_ink: ColorRole::AccentText,
            stroke: None,
            active_stroke: None,
            checked_is_active: true,
        }
    }

    /// Same, with a border.
    pub const fn stroked(mut self, stroke: ColorRole) -> Self {
        self.stroke = Some(stroke);
        self.active_stroke = Some(stroke);
        self
    }

    /// Keeps the content colour on hover (bordered and destructive buttons).
    pub const fn keep_ink(mut self) -> Self {
        self.hover_ink = self.ink;
        self
    }

    /// The resolved roles for `state`. Active wins over disabled: a
    /// disabled widget that is on still shows it (spec §10: state isn't
    /// hidden), with its content at [`DISABLED_OPACITY`].
    pub fn look(&self, state: WidgetState) -> Look {
        let (fill, ink, stroke) = if state.active {
            (self.active_fill, self.active_ink, self.active_stroke)
        } else if state.disabled {
            (self.fill, self.ink, self.stroke)
        } else if state.pressed {
            (self.pressed_fill, self.hover_ink, self.stroke)
        } else if state.hovered {
            (self.hover_fill, self.hover_ink, self.stroke)
        } else {
            (self.fill, self.ink, self.stroke)
        };
        Look {
            fill,
            ink,
            stroke,
            opacity: if state.disabled {
                DISABLED_OPACITY
            } else {
                1.0
            },
        }
    }
}

/// Content opacity of a disabled widget (spec §5).
pub const DISABLED_OPACITY: f32 = 0.4;

/// What a widget's state is this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetState {
    pub hovered: bool,
    pub pressed: bool,
    pub active: bool,
    pub disabled: bool,
}

/// Shows a widget in a fixed state regardless of the pointer (the gallery
/// uses it to show hover and pressed side by side).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForceState(pub WidgetState);

/// Shows the hover look while the widget has keyboard focus (menu items).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct HoverOnFocus;

/// The roles a widget shows in a [`WidgetState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: ColorRole,
    pub ink: ColorRole,
    pub stroke: Option<ColorRole>,
    /// Applies to the content only.
    pub opacity: f32,
}

type StateQuery = (
    Entity,
    &'static Visuals,
    Option<&'static Hovered>,
    Has<Pressed>,
    Has<Active>,
    Has<Checked>,
    Has<InteractionDisabled>,
    Option<&'static ForceState>,
    Has<HoverOnFocus>,
);

/// What changed a widget's look since the last frame.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
pub(crate) struct LookChanges<'w, 's> {
    changed: Query<
        'w,
        's,
        Entity,
        (
            With<Visuals>,
            Or<(
                Changed<Visuals>,
                Changed<Hovered>,
                Changed<ForceState>,
                Changed<Children>,
                Added<Pressed>,
                Added<Active>,
                Added<Checked>,
                Added<InteractionDisabled>,
            )>,
        ),
    >,
    released: RemovedComponents<'w, 's, Pressed>,
    inactive: RemovedComponents<'w, 's, Active>,
    unchecked: RemovedComponents<'w, 's, Checked>,
    enabled: RemovedComponents<'w, 's, InteractionDisabled>,
    unforced: RemovedComponents<'w, 's, ForceState>,
    new_content: Query<'w, 's, Entity, Added<Content>>,
    parents: Query<'w, 's, &'static ChildOf>,
    widgets: Query<'w, 's, (), With<Visuals>>,
    focus: Res<'w, InputFocus>,
}

impl LookChanges<'_, '_> {
    /// The widgets to restyle: new or changed state, new content, and the
    /// widgets focus left and entered ([`HoverOnFocus`]).
    fn collect(&mut self, out: &mut Vec<Entity>, last_focus: &mut Option<Entity>) {
        out.clear();
        out.extend(self.changed.iter());
        out.extend(self.released.read());
        out.extend(self.inactive.read());
        out.extend(self.unchecked.read());
        out.extend(self.enabled.read());
        out.extend(self.unforced.read());
        for content in &self.new_content {
            let owner = self
                .parents
                .iter_ancestors(content)
                .find(|e| self.widgets.contains(*e));
            out.extend(owner);
        }
        if self.focus.is_changed() {
            out.extend(last_focus.take());
            *last_focus = self.focus.get();
            out.extend(*last_focus);
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// What [`button_visuals_system`] writes: each widget's fill and stroke,
/// and its content's ink and opacity.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
pub(crate) struct LookTargets<'w, 's> {
    fills: Query<'w, 's, &'static mut Fill>,
    strokes: Query<'w, 's, &'static mut Stroke>,
    content: Query<
        'w,
        's,
        (
            Option<&'static mut Ink>,
            Option<&'static mut Opacity>,
            Has<Visuals>,
            Has<OwnInk>,
        ),
        With<Content>,
    >,
}

/// What [`button_visuals_system`] keeps between frames: the widgets to
/// visit, the content walk's stack, and the last focused widget.
#[derive(Default)]
pub(crate) struct VisualsScratch {
    dirty: Vec<Entity>,
    stack: Vec<Entity>,
    last_focus: Option<Entity>,
}

/// Writes each widget's [`Fill`] / [`Stroke`] and its [`Content`]'s
/// [`Ink`] and [`Opacity`] from its state. Only widgets whose state, or
/// content, changed this frame are visited, and only real changes are
/// written, so the theme pass stays incremental.
pub(crate) fn button_visuals_system(
    mut changes: LookChanges,
    widgets: Query<StateQuery>,
    children: Query<&Children>,
    mut looks: LookTargets,
    mut commands: Commands,
    mut scratch: Local<VisualsScratch>,
) {
    let VisualsScratch {
        dirty,
        stack,
        last_focus,
    } = &mut *scratch;
    let LookTargets {
        fills,
        strokes,
        content,
    } = &mut looks;
    changes.collect(dirty, last_focus);
    let focus = changes.focus.get();
    for &widget in &*dirty {
        let Ok((entity, visuals, hovered, pressed, active, checked, disabled, forced, focus_hover)) =
            widgets.get(widget)
        else {
            continue;
        };
        let state = forced.map_or(
            WidgetState {
                hovered: hovered.is_some_and(Hovered::get)
                    || (focus_hover && focus == Some(entity)),
                pressed,
                active: active || (checked && visuals.checked_is_active),
                disabled,
            },
            |f| f.0,
        );
        let look = visuals.look(state);
        match fills.get_mut(entity) {
            Ok(mut fill) => {
                fill.set_if_neq(Fill(look.fill));
            }
            Err(_) => {
                commands.entity(entity).insert(Fill(look.fill));
            }
        }
        if let Some(stroke) = look.stroke {
            match strokes.get_mut(entity) {
                Ok(mut s) => {
                    s.set_if_neq(Stroke(stroke));
                }
                Err(_) => {
                    commands.entity(entity).insert(Stroke(stroke));
                }
            }
        }
        // Content down to (not into) nested widgets.
        stack.clear();
        stack.extend(children.get(entity).into_iter().flatten());
        while let Some(child) = stack.pop() {
            let Ok((ink, opacity, nested, own_ink)) = content.get_mut(child) else {
                if !widgets.contains(child) {
                    stack.extend(children.get(child).into_iter().flatten());
                }
                continue;
            };
            if nested {
                continue;
            }
            match (ink, own_ink) {
                (_, true) => {}
                (Some(mut ink), false) => {
                    ink.set_if_neq(Ink(look.ink));
                }
                (None, false) => {
                    commands.entity(child).insert(Ink(look.ink));
                }
            }
            match opacity {
                Some(mut o) => {
                    o.set_if_neq(Opacity(look.opacity));
                }
                None => {
                    commands.entity(child).insert(Opacity(look.opacity));
                }
            }
            stack.extend(children.get(child).into_iter().flatten());
        }
    }
}

#[cfg(test)]
mod tests;
