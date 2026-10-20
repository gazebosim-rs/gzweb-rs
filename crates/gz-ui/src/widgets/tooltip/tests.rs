//! Tooltips: hover timing and chaining, text, cards that follow the
//! theme, and staying inside the window.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;

use super::{TooltipArrow, TooltipCard, TooltipStep, TooltipTimer, CHAIN_SECS, DELAY_SECS};
use crate::assets::fonts::Type;
use crate::assets::icons::IconId;
use crate::test_support::{frames, ui_app};
use crate::theme::{ColorRole, Fill, Stroke, Theme, ThemeMode};
use crate::widgets::*;

#[test]
fn tooltip_waits_500ms_then_chains_within_300ms() {
    let a = Entity::from_raw_u32(1).unwrap();
    let b = Entity::from_raw_u32(2).unwrap();
    let mut t = TooltipTimer::default();
    assert_eq!(t.step(Some(a), false, 0.0), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, 0.49), TooltipStep::default());
    assert_eq!(
        t.step(Some(a), false, DELAY_SECS),
        TooltipStep {
            hide: None,
            show: Some(a)
        }
    );
    // Shown once.
    assert_eq!(t.step(Some(a), false, 0.6), TooltipStep::default());
    // Moving to b within 300 ms: b shows at once.
    assert_eq!(
        t.step(Some(b), false, 0.7),
        TooltipStep {
            hide: Some(a),
            show: Some(b)
        }
    );
    // Leaving, then coming back after more than 300 ms: the full delay.
    assert_eq!(
        t.step(None, false, 0.8),
        TooltipStep {
            hide: Some(b),
            show: None
        }
    );
    let back = 0.8 + CHAIN_SECS + 0.01;
    assert_eq!(t.step(Some(a), false, back), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, back + 0.4), TooltipStep::default());
    assert_eq!(t.step(Some(a), false, back + DELAY_SECS).show, Some(a));
    // A press hides it and keeps it hidden until the pointer leaves.
    assert_eq!(t.step(Some(a), true, 3.0).hide, Some(a));
    assert_eq!(t.step(Some(a), false, 4.0), TooltipStep::default());
    t.step(None, false, 4.1);
    t.step(Some(a), false, 4.2);
    assert_eq!(t.step(Some(a), false, 4.2 + DELAY_SECS).show, Some(a));
}

#[test]
fn tooltip_text() {
    let t = Tooltip::new("Navigate").shortcut("Esc");
    assert_eq!(t.accessible_name(), "Navigate");
    assert_eq!(t.second_line().as_deref(), Some("Esc"));
    let d = Tooltip::new("Workspace: Build")
        .detail("Click to switch")
        .shortcut_opt(Some("Alt+1–4".into()))
        .side(TooltipSide::Above);
    assert_eq!(
        d.second_line().as_deref(),
        Some("Click to switch · Alt+1–4")
    );
    assert_eq!(d.side, TooltipSide::Above);
    assert_eq!(
        Tooltip::new("x").detail("y").second_line().as_deref(),
        Some("y")
    );
    assert_eq!(Tooltip::new("x").second_line(), None);
}

#[test]
fn tooltip_cards_show_on_hover_and_follow_the_theme() {
    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    let root = app
        .world_mut()
        .spawn(Node {
            padding: UiRect::all(px(100)),
            ..default()
        })
        .id();
    let below = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("Settings").shortcut("S"),
            ),
            ChildOf(root),
        ))
        .id();
    let left = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::CamPan,
                IconButtonSize::Rail,
                Tooltip::new("Pan").side(TooltipSide::Left),
            ),
            ChildOf(root),
        ))
        .id();
    frames(&mut app, 2);
    let card_of = |app: &mut App, trigger: Entity| {
        let w = app.world_mut();
        w.query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
            .iter(w)
            .find(|(_, p)| p.parent() == trigger)
            .map(|(e, _)| e)
    };
    // Hovered and held past the delay: a card under the trigger.
    app.world_mut().entity_mut(below).insert(Hovered(true));
    let timer = |app: &mut App| *app.world().resource::<TooltipTimer>();
    frames(&mut app, 1);
    assert!(card_of(&mut app, below).is_none());
    let mut tm = timer(&mut app);
    tm.target = tm.target.map(|(e, _)| (e, -1.0));
    app.insert_resource(tm);
    frames(&mut app, 3);
    let card = card_of(&mut app, below).expect("a tooltip");
    let w = app.world();
    assert_eq!(w.get::<Fill>(card).unwrap().0, ColorRole::Tooltip);
    // Light: no visible border.
    assert_eq!(w.get::<Stroke>(card).unwrap().0, ColorRole::Tooltip);
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    // Below the trigger: the arrow sits on top.
    assert_eq!(w.get::<Node>(arrow).unwrap().top, px(-4));
    // The shortcut is a Geist Mono span on the second line (D-26).
    let lines: Vec<Entity> = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .filter(|c| w.get::<Text>(*c).is_some())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(w.get::<Text>(lines[1]).unwrap().0, "");
    let keys = w.get::<Children>(lines[1]).unwrap()[0];
    assert_eq!(w.get::<TextSpan>(keys).unwrap().0, "S");
    assert_eq!(
        w.get::<Type>(keys).unwrap().0.font,
        crate::theme::FontFamily::Mono
    );
    // Dark: a 1px line.
    app.world_mut().resource_mut::<Theme>().toggle();
    frames(&mut app, 1);
    assert_eq!(app.world().get::<Stroke>(card).unwrap().0, ColorRole::Line);
    // Pressing hides it.
    app.world_mut().entity_mut(below).insert(Pressed);
    frames(&mut app, 2);
    assert!(card_of(&mut app, below).is_none());

    // Moving to the rail button: shows at once, to the left.
    app.world_mut()
        .entity_mut(below)
        .insert(Hovered(false))
        .remove::<Pressed>();
    app.world_mut().entity_mut(left).insert(Hovered(true));
    frames(&mut app, 3);
    let card = card_of(&mut app, left).expect("chained tooltip");
    let w = app.world();
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    assert_eq!(w.get::<Node>(arrow).unwrap().right, px(-4));
}

/// A live tooltip card's rect and its arrow's node, a few frames after
/// it was shown on `trigger`.
fn shown_tooltip(app: &mut App, trigger: Entity, tip: Tooltip) -> (Rect, Rect, Node) {
    use crate::widgets::tooltip::show_tooltip;
    let theme = app.world().resource::<Theme>().clone();
    let mut commands = app.world_mut().commands();
    show_tooltip(&mut commands, &theme, trigger, &tip);
    app.world_mut().flush();
    frames(app, 4);
    let w = app.world_mut();
    let rect = |w: &World, e: Entity| {
        let n = w.get::<ComputedNode>(e).unwrap();
        let t = w.get::<UiGlobalTransform>(e).unwrap();
        let k = n.inverse_scale_factor();
        Rect::from_center_size(t.translation * k, n.size() * k)
    };
    let card = w
        .query_filtered::<(Entity, &ChildOf), With<TooltipCard>>()
        .iter(w)
        .find(|(_, p)| p.parent() == trigger)
        .unwrap()
        .0;
    let arrow = w
        .get::<Children>(card)
        .unwrap()
        .iter()
        .find(|c| w.get::<TooltipArrow>(*c).is_some())
        .unwrap();
    (
        rect(w, card),
        rect(w, trigger),
        w.get::<Node>(arrow).unwrap().clone(),
    )
}

/// Tooltips never leave the window: activity buttons (at the left edge)
/// open theirs to the right; a centred tooltip on a trigger at the right
/// edge shifts in to an 8 px margin, its arrow still under the trigger.
#[test]
fn tooltips_stay_inside_the_window() {
    use crate::widgets::tooltip::{shift_inside, GAP};
    let bounds = Rect::new(8.0, 8.0, 1432.0, 892.0);
    assert_eq!(
        shift_inside(Rect::new(-20.0, 10.0, 80.0, 40.0), bounds),
        Vec2::new(28.0, 0.0)
    );
    assert_eq!(
        shift_inside(Rect::new(1400.0, 880.0, 1500.0, 900.0), bounds),
        Vec2::new(-68.0, -8.0)
    );
    assert_eq!(
        shift_inside(Rect::new(10.0, 10.0, 20.0, 20.0), bounds),
        Vec2::ZERO
    );

    let mut app = ui_app(ThemeMode::Light);
    let t = Theme::default();
    // An activity button at the window's left edge.
    let at = |app: &mut App, node: Node| app.world_mut().spawn(node).id();
    let left_slot = at(
        &mut app,
        Node {
            position_type: PositionType::Absolute,
            left: px(6),
            top: px(60),
            ..default()
        },
    );
    let activity = app
        .world_mut()
        .spawn((
            activity_button(
                &t,
                IconId::ActivityScene,
                Tooltip::new("Scene · long enough to clip"),
                None,
            ),
            ChildOf(left_slot),
        ))
        .id();
    // An icon button at the right edge with a tooltip below.
    let right_slot = at(
        &mut app,
        Node {
            position_type: PositionType::Absolute,
            right: px(30),
            top: px(4),
            ..default()
        },
    );
    let right = app
        .world_mut()
        .spawn((
            icon_button(
                &t,
                IconId::Settings,
                IconButtonSize::Regular,
                Tooltip::new("x"),
            ),
            ChildOf(right_slot),
        ))
        .id();
    frames(&mut app, 2);
    let tip = app.world().get::<Tooltip>(activity).unwrap().clone();
    assert_eq!(tip.side, TooltipSide::Right);
    let (card, button, arrow) = shown_tooltip(&mut app, activity, tip);
    // (The popover measures from inside the button's 1px border.)
    assert!(
        card.min.x >= button.max.x + GAP - 1.5,
        "{card:?} right of {button:?}"
    );
    // One line per row: the title doesn't wrap to the 44 px trigger.
    assert!(card.height() < 40.0 && card.width() > 150.0, "{card:?}");
    assert!(card.min.x >= GAP && card.min.y >= GAP && card.max.y <= 900.0 - GAP);
    // The arrow is on the card's left edge.
    assert_eq!(arrow.left, px(-GAP / 2.0));
    let long =
        Tooltip::new("Settings for the whole workbench").detail("Below, near the right edge");
    let (card, button, arrow) = shown_tooltip(&mut app, right, long);
    assert!(card.max.x <= 1440.0 - GAP + 0.5, "{card:?} inside");
    assert!(card.min.y >= button.max.y + GAP - 1.5, "{card:?} below");
    // Shifted left, the arrow still points at the trigger.
    let Val::Px(left) = arrow.left else {
        panic!("{:?}", arrow.left)
    };
    assert!((card.min.x + 1.0 + left + GAP / 2.0 - button.center().x).abs() < 0.5);
    assert_eq!(arrow.top, px(-GAP / 2.0));
}
