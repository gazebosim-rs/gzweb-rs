//! Tooltip (§5.15): after 500 ms of hover, or at once when another
//! tooltip closed less than 300 ms ago. Radius 7, padding 8 10, Geist 12:
//! a 600-weight title in `tooltip_text` and an optional second line in
//! `tooltip_text_2`, its shortcut in Geist Mono (Geist lacks key caps such
//! as ⏎ and ⌫; DECISIONS.md D-26). Placed below its trigger (above in the
//! status bar, left of the rail, right of the activity bar) with an 8 px
//! gap and an 8 px arrow pointing at the trigger. It never leaves the
//! window: it flips to the other side when its own doesn't fit, and is
//! shifted to stay [`GAP`] inside the window's edges (DECISIONS.md D-49).

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::{
    ComputedNode, ComputedUiRenderTargetInfo, OverrideClip, Pressed, UiGlobalTransform, Val2,
};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};

use super::rounded;
use crate::assets::fonts::{span, text};
use crate::theme::{
    border, elevation, ColorRole, Fill, FontFamily, Stroke, Theme, TypeStyle, SPACE,
};
use crate::GzUiSystems;

/// Hover time before a tooltip shows, in seconds (`tooltip_delay_ms`).
pub const DELAY_SECS: f64 = 0.5;
/// A tooltip that closed this recently lets the next one show at once.
pub const CHAIN_SECS: f64 = 0.3;
/// Gap between trigger and tooltip, and the arrow's size.
pub const GAP: f32 = 8.0;
/// Corner radius (§5.15).
pub const RADIUS: f32 = 7.0;

/// Tooltip title type: Geist 12/600.
pub const TITLE_TYPE: TypeStyle = TypeStyle::new(FontFamily::Ui, 12.0, 600);
/// Tooltip second line: Geist 12/400 (`small`).
pub const DETAIL_TYPE: TypeStyle = TypeStyle::SMALL;
/// A shortcut on the second line: Geist Mono 12/400 (`mono_small`).
pub const KEYS_TYPE: TypeStyle = TypeStyle::MONO_SMALL;

/// Where a tooltip goes relative to its trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipSide {
    #[default]
    Below,
    /// Status bar controls.
    Above,
    /// Viewport rail buttons.
    Left,
    /// Activity bar buttons, at the window's left edge.
    Right,
}

impl TooltipSide {
    fn popover(self) -> Popover {
        let (side, align) = match self {
            TooltipSide::Below => (PopoverSide::Bottom, PopoverAlign::Center),
            TooltipSide::Above => (PopoverSide::Top, PopoverAlign::Center),
            TooltipSide::Left => (PopoverSide::Left, PopoverAlign::Center),
            TooltipSide::Right => (PopoverSide::Right, PopoverAlign::Center),
        };
        Popover {
            positions: vec![
                PopoverPlacement {
                    side,
                    align,
                    gap: GAP,
                },
                PopoverPlacement {
                    side: side.mirror(),
                    align,
                    gap: GAP,
                },
            ],
            window_margin: GAP,
        }
    }
}

/// A control's tooltip: its name (also its accessible name), shortcut and
/// an optional second line.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Tooltip {
    pub title: String,
    pub shortcut: Option<String>,
    pub detail: Option<String>,
    pub side: TooltipSide,
}

impl Tooltip {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..default()
        }
    }

    /// The key that does the same (shown on the second line).
    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// The shortcut from the input map, if the binding has keys.
    pub fn shortcut_opt(mut self, keys: Option<String>) -> Self {
        self.shortcut = keys;
        self
    }

    pub fn detail(mut self, line: impl Into<String>) -> Self {
        self.detail = Some(line.into());
        self
    }

    pub fn side(mut self, side: TooltipSide) -> Self {
        self.side = side;
        self
    }

    /// The accessible name (spec §10: the tooltip text).
    pub fn accessible_name(&self) -> String {
        self.title.clone()
    }

    /// The second line: detail and shortcut, joined with " · ".
    pub fn second_line(&self) -> Option<String> {
        match (&self.detail, &self.shortcut) {
            (Some(d), Some(s)) => Some(format!("{d} · {s}")),
            (Some(d), None) => Some(d.clone()),
            (None, Some(s)) => Some(s.clone()),
            (None, None) => None,
        }
    }
}

/// The tooltip card itself. `live` cards float (absolute, with an arrow);
/// the gallery shows a static one in flow.
pub fn tooltip_card(theme: &Theme, tip: &Tooltip, live: bool) -> impl Bundle {
    let title = tip.title.clone();
    let detail = tip.detail.clone();
    let keys = tip.shortcut.clone();
    let side = tip.side;
    (
        TooltipCard,
        Node {
            position_type: if live {
                PositionType::Absolute
            } else {
                PositionType::Relative
            },
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: UiRect::axes(px(SPACE[5]), px(SPACE[4])),
            row_gap: px(SPACE[1]),
            border: UiRect::all(px(border::HAIRLINE)),
            border_radius: rounded(RADIUS),
            ..default()
        },
        Fill(ColorRole::Tooltip),
        Stroke(card_stroke(theme)),
        elevation::TOOLTIP.shadow(),
        Pickable::IGNORE,
        Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
            if live {
                p.spawn((
                    arrow_node(side),
                    TooltipArrow,
                    Fill(ColorRole::Tooltip),
                    Pickable::IGNORE,
                ));
            }
            // One line each: a floating card's text would otherwise wrap
            // to its trigger's width (the card is the trigger's child).
            p.spawn((
                text(TITLE_TYPE, ColorRole::TooltipText, title),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ));
            // The second line: "detail · keys", the keys a mono span.
            if detail.is_some() || keys.is_some() {
                let lead = match (&detail, &keys) {
                    (Some(d), Some(_)) => format!("{d} · "),
                    (Some(d), None) => d.clone(),
                    (None, _) => String::new(),
                };
                let mut line = p.spawn((
                    text(DETAIL_TYPE, ColorRole::TooltipText2, lead),
                    TextLayout::no_wrap(),
                    Pickable::IGNORE,
                ));
                if let Some(keys) = keys {
                    line.with_child(span(KEYS_TYPE, ColorRole::TooltipText2, keys));
                }
            }
        })),
    )
}

/// Light: no border (the fill is dark). Dark: 1px `line` (§4.2).
fn card_stroke(theme: &Theme) -> ColorRole {
    if theme.is_dark {
        ColorRole::Line
    } else {
        ColorRole::Tooltip
    }
}

/// Marks a tooltip card.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TooltipCard;

/// The 8 px arrow on a live tooltip, pointing at the trigger.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct TooltipArrow;

/// What [`TooltipTimer::step`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TooltipStep {
    pub hide: Option<Entity>,
    pub show: Option<Entity>,
}

/// The hover timing state machine (pure; `tooltip_system` drives it).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct TooltipTimer {
    /// The trigger under the pointer and since when.
    pub target: Option<(Entity, f64)>,
    /// The trigger whose tooltip is showing.
    pub shown: Option<Entity>,
    /// When the last tooltip closed.
    pub closed_at: Option<f64>,
    /// A press on the target hides its tooltip until the pointer leaves.
    pub suppressed: bool,
}

impl TooltipTimer {
    /// Advances to `now` with `hovered` (a pressed trigger counts as
    /// pressed).
    pub fn step(&mut self, hovered: Option<Entity>, pressed: bool, now: f64) -> TooltipStep {
        let mut out = TooltipStep::default();
        if self.target.map(|t| t.0) != hovered {
            if let Some(shown) = self.shown.take() {
                out.hide = Some(shown);
                self.closed_at = Some(now);
            }
            self.target = hovered.map(|h| (h, now));
            self.suppressed = false;
        }
        if pressed && self.target.is_some() {
            self.suppressed = true;
            if let Some(shown) = self.shown.take() {
                out.hide = Some(shown);
                self.closed_at = Some(now);
            }
        }
        if let (Some((target, since)), None, false) = (self.target, self.shown, self.suppressed) {
            let chained = self.closed_at.is_some_and(|c| since - c <= CHAIN_SECS);
            let delay = if chained { 0.0 } else { DELAY_SECS };
            if now - since >= delay {
                self.shown = Some(target);
                out.show = Some(target);
            }
        }
        out
    }
}

/// Shows and hides tooltip cards from hover. The triggers are searched
/// only when one's hover changed; otherwise the timer's target stands.
#[allow(clippy::type_complexity)]
pub(crate) fn tooltip_system(
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    mut timer: ResMut<TooltipTimer>,
    triggers: Query<(Entity, &Tooltip, &Hovered, Has<Pressed>)>,
    hover_changed: Query<(), (With<Tooltip>, Changed<Hovered>)>,
    cards: Query<(Entity, &ChildOf), With<TooltipCard>>,
    mut commands: Commands,
) {
    let hovered = if hover_changed.is_empty() {
        timer
            .target
            .and_then(|(e, _)| triggers.get(e).ok())
            .filter(|(_, _, h, _)| h.get())
    } else {
        triggers.iter().find(|(_, _, h, _)| h.get())
    };
    let pressed = hovered.is_some_and(|h| h.3);
    let step = timer.step(hovered.map(|h| h.0), pressed, time.elapsed_secs_f64());
    if let Some(trigger) = step.hide {
        for (card, parent) in &cards {
            if parent.parent() == trigger {
                commands.entity(card).despawn();
            }
        }
    }
    if let Some(trigger) = step.show {
        if let Ok((_, tip, _, _)) = triggers.get(trigger) {
            show_tooltip(&mut commands, &theme, trigger, tip);
        }
    }
}

/// Spawns `tip`'s floating card on `trigger`.
pub fn show_tooltip(commands: &mut Commands, theme: &Theme, trigger: Entity, tip: &Tooltip) {
    commands.entity(trigger).with_child((
        tooltip_card(theme, tip, true),
        tip.side.popover(),
        GlobalZIndex(1000),
        OverrideClip,
    ));
}

fn arrow_node(side: TooltipSide) -> (Node, UiTransform) {
    let half = -GAP / 2.0;
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: px(GAP),
        height: px(GAP),
        ..default()
    };
    match side {
        TooltipSide::Below => {
            node.top = px(half);
            node.left = Val::Percent(50.0);
            node.margin.left = px(half);
        }
        TooltipSide::Above => {
            node.bottom = px(half);
            node.left = Val::Percent(50.0);
            node.margin.left = px(half);
        }
        TooltipSide::Left => {
            node.right = px(half);
            node.top = Val::Percent(50.0);
            node.margin.top = px(half);
        }
        TooltipSide::Right => {
            node.left = px(half);
            node.top = Val::Percent(50.0);
            node.margin.top = px(half);
        }
    }
    (
        node,
        UiTransform {
            rotation: Rot2::degrees(45.0),
            ..default()
        },
    )
}

/// A node's rect in logical pixels.
fn logical_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let k = node.inverse_scale_factor();
    Rect::from_center_size(transform.translation * k, node.size() * k)
}

/// How far `rect` must move to lie inside `bounds` (to its top-left when
/// it's larger).
pub fn shift_inside(rect: Rect, bounds: Rect) -> Vec2 {
    let axis = |min: f32, max: f32, lo: f32, hi: f32| {
        if min < lo || max - min > hi - lo {
            lo - min
        } else if max > hi {
            hi - max
        } else {
            0.0
        }
    };
    Vec2::new(
        axis(rect.min.x, rect.max.x, bounds.min.x, bounds.max.x),
        axis(rect.min.y, rect.max.y, bounds.min.y, bounds.max.y),
    )
}

/// Keeps every tooltip card [`GAP`] inside the window: after Bevy's
/// popover has picked the side (flipping if needed), a card still past an
/// edge (centred on a trigger near it) is shifted in, the same way the
/// popover moves it, so the frame being drawn already shows it inside.
#[allow(clippy::type_complexity)]
pub(crate) fn keep_tooltips_inside(
    mut cards: Query<
        (
            Entity,
            &ComputedNode,
            &ComputedUiRenderTargetInfo,
            &mut UiTransform,
            &mut UiGlobalTransform,
        ),
        With<TooltipCard>,
    >,
    children: Query<&Children>,
    mut transforms: Query<&mut UiGlobalTransform, Without<TooltipCard>>,
) {
    for (card, node, target, mut transform, mut global) in &mut cards {
        let window = Rect::from_corners(Vec2::ZERO, target.logical_size()).inflate(-GAP);
        let shift = shift_inside(logical_rect(node, &global), window);
        if shift == Vec2::ZERO {
            continue;
        }
        let (Val::Px(x), Val::Px(y)) = (transform.translation.x, transform.translation.y) else {
            continue;
        };
        transform.translation = Val2::px(x + shift.x, y + shift.y);
        let physical = shift * target.scale_factor();
        let mut affine = global.affine();
        affine.translation += physical;
        *global = affine.into();
        let mut stack: Vec<Entity> = children.get(card).into_iter().flatten().copied().collect();
        while let Some(e) = stack.pop() {
            if let Ok(mut t) = transforms.get_mut(e) {
                let mut affine = t.affine();
                affine.translation += physical;
                *t = affine.into();
            }
            stack.extend(children.get(e).into_iter().flatten());
        }
    }
}

/// Puts each arrow on the card's edge that faces the trigger, after the
/// popover placed (and maybe flipped) the card and it was kept inside the
/// window, and slides it along that edge to point at the trigger's centre
/// (clear of the rounded corners).
pub(crate) fn orient_tooltip_arrows(
    cards: Query<(&ComputedNode, &UiGlobalTransform, &ChildOf, &Children), With<TooltipCard>>,
    triggers: Query<(&ComputedNode, &UiGlobalTransform), Without<TooltipCard>>,
    mut arrows: Query<&mut Node, With<TooltipArrow>>,
) {
    for (card_node, card_transform, parent, children) in &cards {
        let Ok((trigger_node, trigger_transform)) = triggers.get(parent.parent()) else {
            continue;
        };
        let card = logical_rect(card_node, card_transform);
        let trigger = logical_rect(trigger_node, trigger_transform);
        let half = px(-GAP / 2.0);
        // Offsets are from inside the card's 1px border.
        let along = |centre: f32, min: f32, len: f32| {
            let lo = RADIUS;
            let hi = (len - 2.0 * border::HAIRLINE - RADIUS - GAP).max(lo);
            px((centre - min - border::HAIRLINE - GAP / 2.0).clamp(lo, hi))
        };
        for child in children.iter() {
            let Ok(mut node) = arrows.get_mut(child) else {
                continue;
            };
            let mut want = node.clone();
            want.top = Val::Auto;
            want.bottom = Val::Auto;
            want.left = Val::Auto;
            want.right = Val::Auto;
            want.margin = UiRect::ZERO;
            if card.min.y >= trigger.max.y - 0.5 || card.max.y <= trigger.min.y + 0.5 {
                // Below or above: the arrow on the top or bottom edge.
                if card.min.y >= trigger.max.y - 0.5 {
                    want.top = half;
                } else {
                    want.bottom = half;
                }
                want.left = along(trigger.center().x, card.min.x, card.width());
            } else {
                // Beside it: the arrow on the left or right edge.
                if card.min.x >= trigger.center().x {
                    want.left = half;
                } else {
                    want.right = half;
                }
                want.top = along(trigger.center().y, card.min.y, card.height());
            }
            if *node != want {
                *node = want;
            }
        }
    }
}

/// Re-picks the card border role when the theme flips (§4.2).
pub(crate) fn tooltip_border_system(
    theme: Res<Theme>,
    mut cards: Query<&mut Stroke, With<TooltipCard>>,
) {
    if !theme.is_changed() {
        return;
    }
    for mut stroke in &mut cards {
        stroke.set_if_neq(Stroke(card_stroke(&theme)));
    }
}

pub(crate) fn build(app: &mut App) {
    app.init_resource::<TooltipTimer>()
        .add_systems(
            Update,
            (tooltip_system, tooltip_border_system).in_set(GzUiSystems::Behaviour),
        )
        .add_systems(
            PostUpdate,
            (
                keep_tooltips_inside
                    .after(bevy::ui::UiSystems::Layout)
                    .before(bevy::ui::UiSystems::PostLayout),
                orient_tooltip_arrows.after(bevy::ui::UiSystems::PostLayout),
            ),
        );
}
