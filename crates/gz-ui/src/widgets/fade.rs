//! Fading a subtree (Bevy UI has no group opacity): each entity with a
//! colour role gets its own [`Opacity`] times the fade's, and keeps its
//! own in [`PreFade`] (a disabled widget's 40%) until the fade ends or is
//! interrupted. Run after the widgets' own looks, so the fade wins.

use bevy::prelude::*;

use crate::theme::{Fill, Ink, Opacity, Shadow, Stroke};

/// An entity's own opacity before a fade, restored when it ends.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreFade(pub f32);

/// Anything with a colour role (what a fade's opacity applies to).
pub(crate) type WithRole = Or<(With<Fill>, With<Stroke>, With<Ink>, With<Shadow>)>;

/// What [`fade_tree`] reads and writes.
pub(crate) type FadeQueries<'w, 's> = (
    Query<'w, 's, &'static Children>,
    Query<'w, 's, (), WithRole>,
    Query<'w, 's, (Option<&'static mut Opacity>, Option<&'static PreFade>)>,
);

/// `root` and everything under it at `alpha` of their own opacity
/// (`None`: their own again).
pub(crate) fn fade_tree(
    commands: &mut Commands,
    (children, roles, opacities): &mut FadeQueries,
    root: Entity,
    alpha: Option<f32>,
) {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if roles.contains(e) {
            if let Ok((opacity, pre)) = opacities.get_mut(e) {
                fade_one(commands, e, opacity, pre, alpha);
            }
        }
        if let Ok(c) = children.get(e) {
            stack.extend(c.iter());
        }
    }
}

/// One entity's opacity at `alpha` of its own (`None`: its own again).
fn fade_one(
    commands: &mut Commands,
    e: Entity,
    opacity: Option<Mut<Opacity>>,
    pre: Option<&PreFade>,
    alpha: Option<f32>,
) {
    let base = pre.map_or_else(|| opacity.as_deref().map_or(1.0, |o| o.0), |p| p.0);
    let want = Opacity(alpha.map_or(base, |a| base * a));
    match opacity {
        Some(mut o) => {
            o.set_if_neq(want);
        }
        None => {
            commands.entity(e).insert(want);
        }
    }
    match (alpha, pre) {
        (Some(_), None) => {
            commands.entity(e).insert(PreFade(base));
        }
        (None, Some(_)) => {
            commands.entity(e).remove::<PreFade>();
        }
        _ => {}
    }
}
