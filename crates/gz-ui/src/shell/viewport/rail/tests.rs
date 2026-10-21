//! The rail through the full UI: where it sits, its controls ↔ the view's
//! state both ways, its menus, the compact rail, Tab, and the keys.

use bevy::input::ButtonState;
use bevy::picking::backend::HitData;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::overlays::{Overlay, OverlayCommand, Overlays};
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::{ortho_height, CameraMode, DragAction, Selection, ViewCommand, ViewOp};
use gz_core::scene::NodeKind;

use super::menus::{CameraItem, LayoutPreset, OverlayItem, NOT_YET};
use super::*;
use crate::shell::regions::Region;
use crate::shell::viewport::hud::{ReleaseTarget, TargetChip};
use crate::shell::viewport::view::ViewNode;
use crate::shell::viewport::view_cube::ViewCube;
use crate::test_support::shell::*;
use crate::test_support::{descendants, frames};
use crate::theme::visuals::Active;
use crate::theme::Theme;

/// The view commands sent so far.
#[derive(Resource, Default)]
struct Ops(Vec<ViewOp>);

fn record(mut reader: MessageReader<ViewCommand>, mut ops: ResMut<Ops>) {
    ops.0.extend(reader.read().map(|c| c.op));
}

/// The full UI, `h` tall, live, recording view commands.
fn live(h: u32) -> App {
    let mut app = live_view(h);
    app.init_resource::<Ops>().add_systems(Last, record);
    app
}

fn ops(app: &mut App) -> Vec<ViewOp> {
    std::mem::take(&mut app.world_mut().resource_mut::<Ops>().0)
}

fn with<C: Component + PartialEq + Copy>(app: &mut App, c: C) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &C)>()
        .iter(world)
        .find(|(_, x)| **x == c)
        .map(|(e, _)| e)
        .expect("one")
}

fn rect_of<C: Component>(app: &mut App) -> (Vec2, Vec2) {
    let e = find::<C>(app);
    rect(app, e)
}

fn activate(app: &mut App, e: Entity) {
    app.world_mut().trigger(Activate { entity: e });
    frames(app, 2);
}

fn has<C: Component>(app: &App, e: Entity) -> bool {
    app.world().get::<C>(e).is_some()
}

fn tip(app: &App, e: Entity) -> Tooltip {
    app.world().get::<Tooltip>(e).unwrap().clone()
}

/// The first text under `e`.
fn label(app: &App, e: Entity) -> String {
    descendants(app.world(), e)
        .into_iter()
        .find_map(|c| app.world().get::<Text>(c).map(|t| t.0.clone()))
        .unwrap_or_default()
}

/// The radio button (not the flyout item) for `a`.
fn radio(app: &mut App, a: DragAction) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &DragChoice), Without<bevy::ui_widgets::MenuItem>>()
        .iter(world)
        .find(|(_, c)| c.0 == a)
        .unwrap()
        .0
}

#[test]
fn the_rail_sits_below_the_cube_at_the_views_top_right() {
    let mut app = live(900);
    let (v, vs) = rect_of::<ViewNode>(&mut app);
    let (r, rs) = rect_of::<Rail>(&mut app);
    let (c, cs) = rect_of::<ViewCube>(&mut app);
    let right = v.x + vs.x;
    // Cube 56 at top 10, right 2; rail 40 wide, 8 below it, inset 10.
    assert_eq!(cs, Vec2::splat(56.0));
    assert_eq!((c.y - v.y, right - (c.x + cs.x)), (10.0, 2.0));
    assert_eq!(rs.x, 40.0);
    assert_eq!((r.y - v.y, right - (r.x + rs.x)), (74.0, 10.0));
    // The cube is centred over the rail.
    assert_eq!(c.x + cs.x / 2.0, r.x + rs.x / 2.0);
    // Eight 32 px buttons, gaps 2, three separators (1 + 2 × 3), 4 a side
    // (padding 3, border 1): the buttons fill the 40 px.
    assert_eq!(rs.y, 8.0 * 32.0 + 10.0 * 2.0 + 3.0 * 7.0 + 2.0 * 4.0);
    let (b, bs) = rect_of::<CameraModeButton>(&mut app);
    assert_eq!((b.x - r.x, bs.x), (4.0, 32.0));
}

#[test]
fn orbit_pan_zoom_and_the_drag_action_follow_each_other() {
    let mut app = live(900);
    let orbit = radio(&mut app, DragAction::Orbit);
    let pan = radio(&mut app, DragAction::Pan);
    assert!(has::<Checked>(&app, orbit) && !has::<Checked>(&app, pan));
    click(&mut app, pan);
    frames(&mut app, 2);
    assert_eq!(view(&mut app).drag_action, DragAction::Pan);
    assert!(has::<Checked>(&app, pan) && !has::<Checked>(&app, orbit));
    // The other way: the view's state shows on the rail.
    send(&mut app, ViewOp::SetDragAction(DragAction::Zoom));
    let zoom = radio(&mut app, DragAction::Zoom);
    assert!(has::<Checked>(&app, zoom) && !has::<Checked>(&app, pan));
    // Names in the tooltips, which open left of the rail.
    let t = tip(&app, zoom);
    assert_eq!((t.title.as_str(), t.side), ("Zoom", TooltipSide::Left));
}

#[test]
fn projection_shows_and_toggles_the_projection() {
    let mut app = live(900);
    let button = with(&mut app, RailButton::Projection);
    assert_eq!(tip(&app, button).title, "Perspective");
    assert_eq!(tip(&app, button).shortcut.as_deref(), Some("5"));
    assert!(!has::<Active>(&app, button));
    activate(&mut app, button);
    assert_eq!(ops(&mut app), [ViewOp::ToggleProjection]);
    let world = app.world_mut();
    let p = world
        .query_filtered::<&Projection, With<MainCamera>>()
        .single(world)
        .unwrap();
    assert!(ortho_height(p).is_some());
    assert!(has::<Active>(&app, button));
    assert_eq!(tip(&app, button).title, "Orthographic");
    let icon = descendants(app.world(), button)
        .into_iter()
        .find_map(|c| app.world().get::<crate::assets::icons::Icon>(c))
        .unwrap()
        .id;
    assert_eq!(icon, IconId::ProjOrtho);
}

#[test]
fn focus_frames_and_a_double_click_goes_home() {
    let mut app = live(900);
    let focus = with(&mut app, RailButton::Focus);
    assert_eq!(tip(&app, focus).shortcut.as_deref(), Some("F"));
    activate(&mut app, focus);
    activate(&mut app, focus);
    activate(&mut app, focus);
    assert_eq!(
        ops(&mut app),
        [
            ViewOp::Focus { at: None },
            ViewOp::Home,
            ViewOp::Focus { at: None }
        ]
    );
}

/// A model named `amr_01`, selected or not.
fn amr(app: &mut App, selected: bool) -> Entity {
    let e = app
        .world_mut()
        .spawn((
            GzEntity {
                name: "amr_01".into(),
                id: 1,
                kind: NodeKind::Model,
                path: "amr_01".into(),
            },
            Transform::from_xyz(1.0, 0.0, 0.0),
        ))
        .id();
    if selected {
        app.world_mut().resource_mut::<Selection>().entities = vec![e];
    }
    frames(app, 2);
    e
}

#[test]
fn the_camera_menu_needs_a_selection_and_shows_the_mode() {
    let mut app = live(900);
    let follow = with(&mut app, CameraItem::Follow);
    let track = with(&mut app, CameraItem::Track);
    let free = with(&mut app, CameraItem::Free);
    let button = find::<CameraModeButton>(&mut app);
    // Nothing selected (M5 brings selection): disabled, saying how else.
    assert!(has::<InteractionDisabled>(&app, follow));
    assert_eq!(label(&app, follow), "Follow selection");
    assert_eq!(
        tip(&app, follow).detail.as_deref(),
        Some("Select a model first, or point at one in the view and press ⇧F")
    );
    assert!(has::<Active>(&app, free));
    activate(&mut app, follow);
    assert!(ops(&mut app).is_empty());
    // Selected: named after it.
    let e = amr(&mut app, true);
    assert!(!has::<InteractionDisabled>(&app, follow));
    assert_eq!(label(&app, follow), "Follow amr_01");
    assert_eq!(label(&app, track), "Track amr_01");
    assert_eq!(tip(&app, follow).detail, None);
    activate(&mut app, follow);
    assert_eq!(ops(&mut app), [ViewOp::SetMode(CameraMode::Follow(e))]);
    assert_eq!(view(&mut app).mode, CameraMode::Follow(e));
    assert!(has::<Active>(&app, follow) && !has::<Active>(&app, free));
    assert!(has::<Active>(&app, button));
    assert_eq!(tip(&app, button).detail.as_deref(), Some("Following"));
    let dot = find::<CameraModeDot>(&mut app);
    assert!(displayed(&app, dot));
    // Following again changes nothing; Track switches.
    activate(&mut app, follow);
    assert!(ops(&mut app).is_empty());
    activate(&mut app, track);
    assert_eq!(view(&mut app).mode, CameraMode::Track(e));
    assert_eq!(tip(&app, button).detail.as_deref(), Some("Tracking"));
    // Free frees it; the dot goes.
    activate(&mut app, free);
    assert_eq!(view(&mut app).mode, CameraMode::Free);
    assert!(!displayed(&app, dot) && !has::<Active>(&app, button));
    // Following with nothing selected any more still names the target and
    // stays enabled.
    send(&mut app, ViewOp::SetMode(CameraMode::Follow(e)));
    app.world_mut().resource_mut::<Selection>().entities.clear();
    frames(&mut app, 2);
    assert!(!has::<InteractionDisabled>(&app, follow));
    assert!(has::<InteractionDisabled>(&app, track));
    assert_eq!(label(&app, follow), "Follow amr_01");
    ops(&mut app);
    // The later items wait.
    for item in [CameraItem::FollowOffset, CameraItem::FovSpeed] {
        let i = with(&mut app, item);
        assert!(has::<InteractionDisabled>(&app, i));
        assert_eq!(tip(&app, i).detail.as_deref(), Some(NOT_YET));
        activate(&mut app, i);
    }
    assert!(ops(&mut app).is_empty());
}

#[test]
fn the_target_chip_names_the_target_and_releases_it() {
    let mut app = live(900);
    let chip = find::<TargetChip>(&mut app);
    assert!(!displayed(&app, chip));
    let e = amr(&mut app, false);
    send(&mut app, ViewOp::SetMode(CameraMode::Track(e)));
    assert!(displayed(&app, chip));
    assert!(all_text(&mut app).contains("amr_01"));
    // Left of the Camera mode button, 8 px from the rail, centred on it.
    let (c, cs) = rect(&app, chip);
    let (b, bs) = rect_of::<CameraModeButton>(&mut app);
    let (r, _) = rect_of::<Rail>(&mut app);
    assert_eq!(cs.y, 28.0);
    assert_eq!(r.x - (c.x + cs.x), 8.0);
    assert_eq!(c.y + cs.y / 2.0, b.y + bs.y / 2.0);
    let release = find::<ReleaseTarget>(&mut app);
    assert_eq!(tip(&app, release).title, "Stop tracking");
    assert_eq!(tip(&app, release).shortcut.as_deref(), Some("⇧T"));
    send(&mut app, ViewOp::SetMode(CameraMode::Follow(e)));
    assert_eq!(tip(&app, release).title, "Stop following");
    activate(&mut app, release);
    assert_eq!(view(&mut app).mode, CameraMode::Free);
    assert!(!displayed(&app, chip));
}

#[test]
fn the_overlays_menu_and_the_overlays_follow_each_other() {
    let mut app = live(900);
    let badge = find::<OverlaysBadge>(&mut app);
    assert!(!displayed(&app, badge));
    let item = |app: &mut App, o| with(app, OverlayItem(o));
    let collisions = item(&mut app, Overlay::Collisions);
    activate(&mut app, collisions);
    assert!(app.world().resource::<Overlays>().collisions);
    assert!(has::<Checked>(&app, collisions));
    assert!(displayed(&app, badge));
    assert_eq!(label(&app, badge), "1");
    // The keys' way (G, C, I, J send these): the menu follows.
    for o in [Overlay::Grid, Overlay::Joints] {
        app.world_mut().write_message(OverlayCommand::toggle(o));
    }
    frames(&mut app, 2);
    let grid = item(&mut app, Overlay::Grid);
    assert!(has::<Checked>(&app, grid));
    assert_eq!(label(&app, badge), "3");
    activate(&mut app, collisions);
    assert!(!app.world().resource::<Overlays>().collisions);
    assert!(!has::<Checked>(&app, collisions));
    assert_eq!(label(&app, badge), "2");
    // Every overlay has an item, in the spec's order.
    let popup = app.world().get::<ChildOf>(collisions).unwrap().parent();
    let items: Vec<Overlay> = app
        .world()
        .get::<Children>(popup)
        .unwrap()
        .iter()
        .filter_map(|c| app.world().get::<OverlayItem>(c).map(|i| i.0))
        .collect();
    assert_eq!(items, Overlay::ALL);
    let t = all_text(&mut app);
    for s in [
        "Grid",
        "Center of mass",
        "Lidar scans / sensor data",
        "Apply to all views",
    ] {
        assert!(t.contains(s), "{s}");
    }
}

#[test]
fn the_layout_popover_offers_single_and_shows_the_rest_disabled() {
    let mut app = live(900);
    let world = app.world_mut();
    let presets: Vec<(LayoutPreset, Tooltip)> = world
        .query::<(&LayoutPreset, &Tooltip)>()
        .iter(world)
        .map(|(p, t)| (*p, t.clone()))
        .collect();
    assert_eq!(presets.len(), 5);
    for (p, t) in presets {
        let want = if p == LayoutPreset::Single {
            "Current layout"
        } else {
            NOT_YET
        };
        assert_eq!(t.detail.as_deref(), Some(want), "{p:?}");
        assert_eq!(t.title, p.name());
        assert!(!p.panes().is_empty());
    }
    let t = all_text(&mut app);
    assert!(t.contains("Maximize") && t.contains("Close view") && t.contains("Link cameras"));
}

#[test]
fn a_short_view_folds_the_drag_buttons_into_one() {
    // At the 640 px minimum and UI scale 1.25 the view is 440 UI pixels
    // tall, under rail_compact_below_view_h (500).
    let mut app = live(640);
    app.world_mut().resource_mut::<Theme>().scale = 1.25;
    frames(&mut app, 4);
    let group = find::<DragGroup>(&mut app);
    let compact = find::<CompactDrag>(&mut app);
    assert!(!displayed(&app, group) && displayed(&app, compact));
    let (_, rs) = rect_of::<Rail>(&mut app);
    // (Rounded to physical pixels at 1.25.)
    assert!((rs.y - (6.0 * 32.0 + 8.0 * 2.0 + 3.0 * 7.0 + 2.0 * 4.0)).abs() <= 1.0);
    // The flyout picks; the button shows the current action.
    let trigger = find::<CompactDragButton>(&mut app);
    assert!(has::<Active>(&app, trigger));
    let world = app.world_mut();
    let pan = world
        .query_filtered::<(Entity, &DragChoice), With<bevy::ui_widgets::MenuItem>>()
        .iter(world)
        .find(|(_, c)| c.0 == DragAction::Pan)
        .unwrap()
        .0;
    activate(&mut app, pan);
    assert_eq!(view(&mut app).drag_action, DragAction::Pan);
    assert!(has::<Active>(&app, pan));
    assert_eq!(tip(&app, trigger).title, "Pan");
    // 900 tall: the full rail.
    let mut app = live(900);
    let group = find::<DragGroup>(&mut app);
    let compact = find::<CompactDrag>(&mut app);
    assert!(displayed(&app, group) && !displayed(&app, compact));
}

/// Tab from no focus until it comes round: the focused entities in order.
fn tab_walk(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    let mut seen = Vec::new();
    for _ in 0..300 {
        key(app, KeyCode::Tab, ButtonState::Pressed);
        app.update();
        key(app, KeyCode::Tab, ButtonState::Released);
        app.update();
        let e = focused(app).expect("Tab focuses something");
        if seen.first() == Some(&e) {
            break;
        }
        seen.push(e);
    }
    seen
}

fn region_of(world: &World, e: Entity) -> Region {
    std::iter::successors(Some(e), |x| world.get::<ChildOf>(*x).map(ChildOf::parent))
        .find_map(|a| world.get::<Region>(a).copied())
        .expect("in a region")
}

#[test]
fn tab_walks_top_bar_to_status_bar_through_the_rail_in_order() {
    let mut app = live(900);
    let walk = tab_walk(&mut app);
    let mut regions: Vec<Region> = walk.iter().map(|e| region_of(app.world(), *e)).collect();
    regions.dedup();
    use Region::*;
    assert_eq!(
        regions,
        [
            TopBar,
            ActivityBar,
            Sidebar,
            ViewportArea,
            Inspector,
            StatusBar
        ]
    );
    let in_view: Vec<Entity> = walk
        .into_iter()
        .filter(|e| region_of(app.world(), *e) == ViewportArea)
        .collect();
    let mut want: Vec<Entity> = DRAG_ACTIONS
        .into_iter()
        .map(|a| radio(&mut app, a))
        .collect();
    want.push(with(&mut app, RailButton::Projection));
    want.push(find::<CameraModeButton>(&mut app));
    want.push(with(&mut app, RailButton::Focus));
    want.push(find::<OverlaysButton>(&mut app));
    want.push(find::<LayoutButton>(&mut app));
    assert_eq!(in_view, want);
    // No tab group of their own: Bevy 0.20 sorts groups globally, so one
    // would jump ahead of the top bar (D-58).
    for e in [find::<Rail>(&mut app), find::<ViewCube>(&mut app)] {
        assert!(!has::<bevy::input_focus::tab_navigation::TabGroup>(&app, e));
    }
}

#[test]
fn a_press_on_the_rail_keeps_the_views_keys() {
    let mut app = live(900);
    let rail = find::<Rail>(&mut app);
    let top = region(&mut app, Region::TopBar);
    let window = find::<Window>(&mut app);
    let press_over = |app: &mut App, e: Entity, at: Vec2| {
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(at));
        let mut hover = app.world_mut().resource_mut::<HoverMap>();
        hover.clear();
        hover
            .entry(PointerId::Mouse)
            .or_default()
            .insert(e, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
        mouse(app, ButtonState::Pressed);
        app.update();
        mouse(app, ButtonState::Released);
        app.update();
        *app.world().resource::<ViewRouting>()
    };
    let (r, _) = rect(&app, rail);
    let on_rail = r + Vec2::splat(5.0);
    // The rail blocks the camera's pointer, but keeps the keys.
    let routing = press_over(&mut app, rail, on_rail);
    assert!(!routing.pointer && routing.keys);
    // A press on the top bar takes them; a Tab-focused rail button gives
    // them back.
    let routing = press_over(&mut app, top, Vec2::new(700.0, 20.0));
    assert!(!routing.keys);
    let focus = with(&mut app, RailButton::Focus);
    tab_to(&mut app, focus);
    app.update();
    assert!(app.world().resource::<ViewRouting>().keys);
}
