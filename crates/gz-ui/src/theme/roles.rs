//! Colour roles (spec §4.1): widgets store a role ([`Fill`], [`Stroke`],
//! [`Ink`], [`Ring`], [`Shadow`]), never a resolved `Color`, and
//! `apply_theme_system` resolves them when the theme or a role changes;
//! [`Opacity`] scales them. The focus-visible ring is a role too.

use bevy::ecs::query::QueryData;
use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::text::TextCursorStyle;

use super::{border, ColorRole, Theme};

/// Background colour role → `BackgroundColor`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(BackgroundColor)]
pub struct Fill(pub ColorRole);

/// Border colour role → `BorderColor` (all sides).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(BorderColor)]
pub struct Stroke(pub ColorRole);

/// Content colour role → `TextColor` on text, the tint on icons (`ImageNode`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink(pub ColorRole);

/// Outline colour role → `Outline` (focus rings, input halos).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring(pub ColorRole);

/// Shadow colour role → every `BoxShadow` layer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadow(pub ColorRole);

/// Multiplies the alpha of every colour resolved on this entity (disabled
/// content at 40%, hidden tree rows at 50%).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Opacity(pub f32);

/// Selection and caret colours for a text input, from its [`Ink`].
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(TextCursorStyle)]
pub struct CaretRoles;

type RoleRefs = (
    Option<Ref<'static, Fill>>,
    Option<Ref<'static, Stroke>>,
    Option<Ref<'static, Ink>>,
    Option<Ref<'static, Ring>>,
    Option<Ref<'static, Shadow>>,
    Option<Ref<'static, Opacity>>,
    Has<CaretRoles>,
);

type Targets = (
    Option<&'static mut BackgroundColor>,
    Option<&'static mut BorderColor>,
    Option<&'static mut TextColor>,
    Option<&'static mut ImageNode>,
    Option<&'static mut Outline>,
    Option<&'static mut BoxShadow>,
    Option<&'static mut TextCursorStyle>,
);

type RoleChanged = Or<(
    Changed<Fill>,
    Changed<Stroke>,
    Changed<Ink>,
    Changed<Ring>,
    Changed<Shadow>,
    Changed<Opacity>,
)>;

type WithRole = Or<(
    With<Fill>,
    With<Stroke>,
    With<Ink>,
    With<Ring>,
    With<Shadow>,
)>;

/// Re-resolves role components: all of them when the [`Theme`] changes,
/// otherwise only entities whose role (or [`Opacity`]) changed, found by
/// change detection rather than a scan.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_theme_system(
    theme: Res<Theme>,
    mut q: ParamSet<(
        Query<(RoleRefs, Targets), WithRole>,
        Query<(RoleRefs, Targets), (WithRole, RoleChanged)>,
    )>,
) {
    if theme.is_changed() {
        for (roles, targets) in &mut q.p0() {
            resolve_roles(&theme, roles, targets);
        }
    } else {
        for (roles, targets) in &mut q.p1() {
            resolve_roles(&theme, roles, targets);
        }
    }
}

/// Writes one entity's colours from its roles.
fn resolve_roles(
    theme: &Theme,
    (fill, stroke, ink, ring, shadow, opacity, caret): <RoleRefs as QueryData>::Item<'_, '_>,
    targets: <Targets as QueryData>::Item<'_, '_>,
) {
    let alpha = opacity.as_deref().map_or(1.0, |o| o.0);
    let resolve = |role: ColorRole| {
        let c = theme.color(role);
        c.with_alpha(c.alpha() * alpha)
    };
    let (bg, border, text, image, outline, box_shadow, cursor) = targets;
    if let (Some(fill), Some(mut bg)) = (fill, bg) {
        bg.set_if_neq(BackgroundColor(resolve(fill.0)));
    }
    if let (Some(stroke), Some(mut border)) = (stroke, border) {
        border.set_if_neq(BorderColor::all(resolve(stroke.0)));
    }
    if let Some(ink) = ink {
        let c = resolve(ink.0);
        if let Some(mut text) = text {
            text.set_if_neq(TextColor(c));
        }
        if let Some(mut image) = image {
            if image.color != c {
                image.color = c;
            }
        }
        if let (true, Some(mut cursor)) = (caret, cursor) {
            cursor.color = c;
            cursor.selection_color = theme.color(ColorRole::AccentSoft);
            cursor.unfocused_selection_color = theme.color(ColorRole::Hover);
        }
    }
    if let (Some(ring), Some(mut outline)) = (ring, outline) {
        let c = resolve(ring.0);
        if outline.color != c {
            outline.color = c;
        }
    }
    if let (Some(shadow), Some(mut box_shadow)) = (shadow, box_shadow) {
        let c = resolve(shadow.0);
        for layer in &mut box_shadow.0 {
            layer.color = c;
        }
    }
}

/// A widget that shows the focus-visible ring (spec §5: 2px `accent`,
/// offset 2) while it has keyboard focus.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FocusRing;

/// Draws or clears the [`FocusRing`]s.
pub(crate) fn focus_ring_system(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    mut q: Query<(Entity, Option<&mut Ring>), With<FocusRing>>,
    mut commands: Commands,
) {
    if !focus.is_changed() && !visible.is_changed() {
        return;
    }
    for (entity, ring) in &mut q {
        let on = visible.0 && focus.get() == Some(entity);
        let role = if on {
            ColorRole::Accent
        } else {
            ColorRole::Clear
        };
        match ring {
            Some(mut ring) => {
                ring.set_if_neq(Ring(role));
            }
            None => {
                commands.entity(entity).insert((
                    Ring(role),
                    Outline::new(px(border::FOCUS_RING), px(2), Color::NONE),
                ));
            }
        }
    }
}
