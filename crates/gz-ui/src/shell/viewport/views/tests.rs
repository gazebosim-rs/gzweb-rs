use bevy::input::ButtonState;
use bevy::picking::events::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::PointerButton;
use bevy::ui_widgets::Activate;
use gz_bevy::camera::MainCamera;
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::{ortho_height, ViewId, ViewPlacement};
use std::time::Duration;

use super::*;
use crate::shell::viewport::rail::RailButton;
use crate::shell::viewport::view::ViewNode;
use crate::shell::viewport::view_cube::scene::CubeScene;
use crate::shell::ShellLayout;
use crate::test_support::frames;
use crate::test_support::shell::*;

/// The full UI live at 1440 × `h` with `preset` picked.
fn split(h: u32, preset: LayoutPreset) -> App {
    let mut app = live_view(h);
    command(&mut app, LayoutCommand::Preset(preset));
    app
}

fn command(app: &mut App, cmd: LayoutCommand) {
    app.world_mut().write_message(cmd);
    frames(app, 4);
}

/// The view nodes by view, with their rects.
fn nodes(app: &mut App) -> Vec<(ViewId, Entity, (Vec2, Vec2))> {
    let world = app.world_mut();
    let mut out: Vec<(ViewId, Entity)> = world
        .query::<(Entity, &ViewNode)>()
        .iter(world)
        .map(|(e, n)| (n.0, e))
        .collect();
    out.sort_by_key(|n| n.0);
    out.into_iter()
        .map(|(id, e)| (id, e, rect(app, e)))
        .collect()
}

/// Each view's camera: (id, entity, shown, placement).
fn cameras(app: &mut App) -> Vec<(ViewId, Entity, bool, ViewPlacement)> {
    let world = app.world_mut();
    let mut out: Vec<_> = world
        .query_filtered::<(Entity, &ViewId, &Camera, &ViewPlacement), With<MainCamera>>()
        .iter(world)
        .map(|(e, id, c, p)| (*id, e, c.is_active, p.clone()))
        .collect();
    out.sort_by_key(|c| c.0);
    out
}

fn camera(app: &mut App, id: u8) -> Entity {
    cameras(app)
        .into_iter()
        .find(|c| c.0 == ViewId(id))
        .expect("a camera")
        .1
}

fn layout(app: &App) -> ViewLayout {
    app.world().resource::<ViewLayout>().clone()
}

fn routing(app: &App) -> ViewRouting {
    *app.world().resource::<ViewRouting>()
}

/// The viewport area at 1440 × 900 with both panels open.
const AREA: (Vec2, Vec2) = (Vec2::new(356.0, 44.0), Vec2::new(784.0, 828.0));

#[test]
fn presets_split_the_area_and_place_each_view_in_its_node() {
    let mut app = split(900, LayoutPreset::TwoColumns);
    let n = nodes(&mut app);
    assert_eq!(n.len(), 2);
    // 3:2 with a 2 px gap (laid out to whole pixels).
    let (min, size) = AREA;
    let left = ((size.x - 2.0) * 0.6).round();
    assert_eq!(n[0].2, (min, Vec2::new(left, size.y)));
    assert_eq!(
        n[1].2,
        (
            min + Vec2::new(left + 2.0, 0.0),
            Vec2::new(size.x - 2.0 - left, size.y)
        )
    );
    // Each camera renders into its own node's rect and texture.
    let cams = cameras(&mut app);
    assert_eq!(cams.len(), 2);
    for ((id, node, (min, size)), (cid, _, on, placement)) in n.iter().zip(&cams) {
        assert_eq!(id, cid);
        assert!(on);
        assert_eq!(placement.rect, Some(Rect::from_corners(*min, *min + *size)));
        let shown = app.world().get::<ImageNode>(*node).map(|i| i.image.clone());
        assert_eq!(shown, placement.image);
    }
    assert_ne!(cams[0].3.image, cams[1].3.image);
    // Quad: four quarters.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Quad));
    let n = nodes(&mut app);
    assert_eq!(n.len(), 4);
    let quarter = ((size - Vec2::splat(2.0)) / 2.0).round();
    for (_, _, (_, s)) in &n {
        assert_eq!(*s, quarter);
    }
    // One plus two: the first full height.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::OnePlusTwo));
    let n = nodes(&mut app);
    assert_eq!(n.len(), 3);
    assert_eq!(n[0].2 .1.y, size.y);
    assert_eq!(n[1].2 .1.y, quarter.y);
    // Back to Single: the other cameras are kept, off.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    assert_eq!(nodes(&mut app).len(), 1);
    let cams = cameras(&mut app);
    assert_eq!(cams.len(), 4);
    assert!(cams[0].2 && cams[1..].iter().all(|c| !c.2 && c.3.rect.is_none()));
}

/// Drags the divider for `axis` by `d` pixels, then releases it.
fn drag_divider(app: &mut App, axis: usize, d: f32) {
    let divider = with_divider(app, axis);
    let w = app.world_mut();
    w.trigger(PointerDragStart {
        entity: divider,
        pointer: pointer(),
        button: PointerButton::Primary,
        hit: hit(),
    });
    w.flush();
    let distance = if axis == 0 {
        Vec2::new(d, 0.0)
    } else {
        Vec2::new(0.0, d)
    };
    w.trigger(PointerDrag {
        entity: divider,
        pointer: pointer(),
        button: PointerButton::Primary,
        distance,
        delta: distance,
    });
    w.trigger(PointerDragEnd {
        entity: divider,
        pointer: pointer(),
        button: PointerButton::Primary,
        distance,
    });
    frames(app, 3);
}

fn with_divider(app: &mut App, axis: usize) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ViewDivider)>()
        .iter(world)
        .find(|(_, d)| d.0 == axis)
        .expect("a divider")
        .0
}

#[test]
fn a_divider_drags_its_share_short_of_the_floor_and_a_double_click_resets_it() {
    let mut app = split(900, LayoutPreset::TwoColumns);
    // The views' length: the gap left out.
    let row = AREA.1.x - 2.0;
    drag_divider(&mut app, 0, -100.0);
    let share = layout(&app).shares()[0];
    assert!((share - (0.6 - 100.0 / row)).abs() < 1e-4, "{share}");
    let n = nodes(&mut app);
    assert!((n[0].2 .1.x - row * share).abs() < 1.0);
    // Not below 240 a side.
    drag_divider(&mut app, 0, -1000.0);
    let n = nodes(&mut app);
    assert!(n[0].2 .1.x >= 238.0, "{:?}", n[0].2);
    // A double click (not the end of a drag) puts 3:2 back.
    let divider = with_divider(&mut app, 0);
    for count in [1, 2] {
        app.world_mut().trigger(PointerClick {
            entity: divider,
            pointer: pointer(),
            button: PointerButton::Primary,
            hit: hit(),
            duration: Duration::from_millis(80),
            count,
        });
    }
    frames(&mut app, 2);
    assert_eq!(layout(&app).shares()[0], 0.6);
    // Each preset keeps its own shares.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::TwoRows));
    drag_divider(&mut app, 1, 80.0);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::TwoColumns));
    assert_eq!(layout(&app).shares()[0], 0.6);
}

/// Puts the cursor at `at` (window logical pixels) and runs a frame.
fn cursor(app: &mut App, at: Vec2) {
    let window = find::<Window>(app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(at));
    app.update();
}

#[test]
fn the_view_under_the_pointer_is_hovered_and_a_press_makes_it_active() {
    let mut app = split(900, LayoutPreset::TwoColumns);
    let n = nodes(&mut app);
    let right = n[1].2 .0 + n[1].2 .1 / 2.0;
    cursor(&mut app, right);
    assert_eq!(routing(&app).hovered, Some(ViewId(1)));
    assert_eq!(routing(&app).active, Some(ViewId(0)));
    mouse(&mut app, ButtonState::Pressed);
    cursor(&mut app, right);
    mouse(&mut app, ButtonState::Released);
    frames(&mut app, 2);
    assert_eq!(routing(&app).active, Some(ViewId(1)));
    // The active view has the accent frame, the other none.
    let world = app.world_mut();
    let frames_shown: Vec<bool> = world
        .query::<(&ActiveFrame, &Node, &ChildOf)>()
        .iter(world)
        .map(|(_, node, parent)| (parent.parent(), node.display != Display::None))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(p, shown)| (p == n[1].1) == shown)
        .collect();
    assert_eq!(frames_shown, [true, true]);
    // Off the views: nothing hovered, still active.
    cursor(&mut app, Vec2::new(100.0, 400.0));
    assert_eq!(routing(&app).hovered, None);
    assert_eq!(routing(&app).key_view(), Some(ViewId(1)));
    // One view: no frame.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    let world = app.world_mut();
    let any = world
        .query::<(&ActiveFrame, &Node)>()
        .iter(world)
        .any(|(_, n)| n.display != Display::None);
    assert!(!any);
}

#[test]
fn each_view_has_its_own_rail_and_cube() {
    let mut app = split(900, LayoutPreset::TwoColumns);
    // View 1's Projection button turns view 1 orthographic, not view 0.
    let world = app.world_mut();
    let buttons: Vec<Entity> = world
        .query::<(Entity, &RailButton)>()
        .iter(world)
        .filter(|(_, b)| **b == RailButton::Projection)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(buttons.len(), 2);
    let views = nodes(&mut app);
    let in_view = |app: &mut App, e: Entity| {
        let world = app.world_mut();
        let mut q = world.query::<&ChildOf>();
        let mut at = e;
        while let Ok(p) = q.get(world, at) {
            at = p.parent();
            if let Some(v) = views.iter().find(|v| v.1 == at) {
                return v.0;
            }
        }
        panic!("not in a view");
    };
    let b1 = *buttons
        .iter()
        .find(|b| in_view(&mut app, **b) == ViewId(1))
        .unwrap();
    app.world_mut().trigger(Activate { entity: b1 });
    frames(&mut app, 3);
    let ortho = |app: &mut App, id| {
        let e = camera(app, id);
        ortho_height(app.world().get::<Projection>(e).unwrap()).is_some()
    };
    assert!(ortho(&mut app, 1) && !ortho(&mut app, 0));
    // A cube scene each, on its own layer; the dropped view's goes.
    let world = app.world_mut();
    let scenes: Vec<CubeScene> = world.query::<&CubeScene>().iter(world).cloned().collect();
    assert_eq!(scenes.len(), 2);
    let layers: Vec<_> = scenes
        .iter()
        .map(|s| {
            app.world()
                .get::<bevy::camera::visibility::RenderLayers>(s.camera)
                .unwrap()
                .clone()
        })
        .collect();
    assert_ne!(layers[0], layers[1]);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    let gone = scenes
        .iter()
        .filter(|s| app.world().get_entity(s.root).is_err())
        .count();
    assert_eq!(gone, 2, "rebuilt: both old scenes go, one new one comes");
    let world = app.world_mut();
    assert_eq!(world.query::<&CubeScene>().iter(world).count(), 1);
}

#[test]
fn maximize_shows_one_view_and_close_view_renumbers_the_rest() {
    let mut app = split(900, LayoutPreset::Quad);
    let before: Vec<Entity> = (0..4).map(|i| camera(&mut app, i)).collect();
    app.world_mut().resource_mut::<ViewRouting>().active = Some(ViewId(2));
    // ⇧Space maximizes the active view; again restores.
    chord(&mut app, &[KeyCode::ShiftLeft, KeyCode::Space]);
    frames(&mut app, 2);
    assert_eq!(layout(&app).maximized, Some(ViewId(2)));
    let n = nodes(&mut app);
    assert_eq!(n.len(), 1);
    assert_eq!((n[0].0, n[0].2), (ViewId(2), AREA));
    command(&mut app, LayoutCommand::Maximize(None));
    assert_eq!(nodes(&mut app).len(), 4);
    // Close view 1: the views after it move up, its camera goes last.
    app.world_mut().resource_mut::<ViewRouting>().active = Some(ViewId(1));
    app.world_mut().resource_mut::<ViewRouting>().hovered = None;
    command(&mut app, LayoutCommand::Close(None));
    assert_eq!(layout(&app).preset, LayoutPreset::OnePlusTwo);
    let after: Vec<Entity> = (0..4).map(|i| camera(&mut app, i)).collect();
    assert_eq!(after, [before[0], before[2], before[3], before[1]]);
    assert_eq!(routing(&app).active, Some(ViewId(1)));
    let cams = cameras(&mut app);
    assert!(cams[3].3.rect.is_none() && !cams[3].2);
    // Down to one: nothing more to close.
    command(&mut app, LayoutCommand::Close(None));
    command(&mut app, LayoutCommand::Close(None));
    assert_eq!(layout(&app).preset, LayoutPreset::Single);
    command(&mut app, LayoutCommand::Close(None));
    assert_eq!(layout(&app).preset, LayoutPreset::Single);
    // Splits: right, then down.
    command(&mut app, LayoutCommand::Split { down: false });
    assert_eq!(layout(&app).preset, LayoutPreset::TwoColumns);
    command(&mut app, LayoutCommand::Split { down: true });
    assert_eq!(layout(&app).preset, LayoutPreset::OnePlusTwo);
}

#[test]
fn the_floor_collapses_the_panels_then_falls_back_to_single() {
    // Wide panels leave 424 px: Quad needs 482, so the inspector goes.
    let mut app = live_view(900);
    {
        let mut shell = app.world_mut().resource_mut::<ShellLayout>();
        shell.sidebar_w = 480.0;
        shell.inspector_w = 480.0;
        shell.startup = false;
    }
    frames(&mut app, 2);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Quad));
    let shell = app.world().resource::<ShellLayout>().clone();
    assert!(!shell.inspector_open && shell.sidebar_open);
    assert!(!layout(&app).fallback);
    assert_eq!(nodes(&mut app).len(), 4);
    // Single again: the inspector comes back.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    let shell = app.world().resource::<ShellLayout>().clone();
    assert!(shell.inspector_open);
    // Too short for two rows: Single, until there is room.
    let mut app = split(520, LayoutPreset::TwoRows);
    assert!(layout(&app).fallback);
    assert_eq!(nodes(&mut app).len(), 1);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::TwoColumns));
    assert!(!layout(&app).fallback);
    assert_eq!(nodes(&mut app).len(), 2);
}

#[test]
fn shares_clamp_or_split_evenly() {
    assert_eq!(share_after(0.5, 100.0, 1000.0, 240.0), 0.6);
    assert_eq!(share_after(0.5, -400.0, 1000.0, 240.0), 0.24);
    assert_eq!(share_after(0.5, 10.0, 400.0, 240.0), 0.5);
    // Before layout: as it was.
    assert_eq!(fit_share(0.6, 0.0, 240.0), 0.6);
    assert_eq!(fit_share(0.6, 502.0, 240.0), 1.0 - 240.0 / 502.0);
    assert_eq!(
        ViewLayout::floor(LayoutPreset::Quad, 240.0, 2.0),
        Vec2::new(482.0, 482.0)
    );
    for p in LayoutPreset::ALL {
        assert_eq!(p.panes().len(), usize::from(p.count()));
    }
}

#[test]
fn a_rail_menu_over_another_view_acts_on_its_own() {
    use super::super::rail::menus::LayoutItem;
    use bevy::picking::hover::HoverMap;
    use bevy::picking::pointer::PointerId;
    let mut app = split(900, LayoutPreset::Quad);
    let before: Vec<Entity> = (0..4).map(|i| camera(&mut app, i)).collect();
    // View 1's Close view item, wherever it hangs.
    let n = nodes(&mut app);
    let world = app.world_mut();
    let items: Vec<(Entity, LayoutItem)> = world
        .query::<(Entity, &LayoutItem)>()
        .iter(world)
        .map(|(e, i)| (e, *i))
        .collect();
    let views = n.clone();
    let in_view = |app: &mut App, e: Entity| {
        let world = app.world_mut();
        let mut q = world.query::<&ChildOf>();
        let mut at = e;
        while let Ok(p) = q.get(world, at) {
            at = p.parent();
            if let Some(v) = views.iter().find(|v| v.1 == at) {
                return Some(v.0);
            }
        }
        None
    };
    let close = items
        .iter()
        .filter(|(_, i)| *i == LayoutItem::Close)
        .map(|(e, _)| *e)
        .find(|e| in_view(&mut app, *e) == Some(ViewId(1)))
        .expect("view 1's Close view");
    // The pointer over view 3's rect, but on view 1's menu: view 1's.
    let window = find::<Window>(&mut app);
    let over3 = n[3].2 .0 + n[3].2 .1 / 2.0;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(over3));
    app.world_mut()
        .resource_mut::<HoverMap>()
        .entry(PointerId::Mouse)
        .or_default()
        .insert(close, hit());
    app.update();
    assert_eq!(routing(&app).hovered, Some(ViewId(1)));
    app.world_mut().resource_mut::<HoverMap>().clear();
    app.world_mut().trigger(Activate { entity: close });
    frames(&mut app, 3);
    assert_eq!(layout(&app).preset, LayoutPreset::OnePlusTwo);
    let after: Vec<Entity> = (0..4).map(|i| camera(&mut app, i)).collect();
    assert_eq!(after, [before[0], before[2], before[3], before[1]]);
}

#[test]
fn the_active_view_is_always_one_that_shows() {
    let mut app = split(900, LayoutPreset::Quad);
    app.world_mut().resource_mut::<ViewRouting>().active = Some(ViewId(3));
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    assert_eq!(routing(&app).active, Some(ViewId(0)));
    // Maximizing a view makes it the active one; restoring keeps it.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Quad));
    command(&mut app, LayoutCommand::Maximize(Some(ViewId(2))));
    assert_eq!(routing(&app).active, Some(ViewId(2)));
    command(&mut app, LayoutCommand::Maximize(None));
    assert_eq!(routing(&app).active, Some(ViewId(2)));
    // The floor's fallback shows view 0 only.
    let mut app = live_view(520);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Quad));
    assert!(layout(&app).fallback);
    app.world_mut().resource_mut::<ViewRouting>().active = Some(ViewId(3));
    command(&mut app, LayoutCommand::Preset(LayoutPreset::TwoRows));
    assert_eq!(routing(&app).active, Some(ViewId(0)));
}

#[test]
fn close_and_split_wait_for_the_preset_to_show_and_for_the_mouse() {
    // The fallback: nothing to close or split.
    let mut app = split(520, LayoutPreset::Quad);
    assert!(layout(&app).fallback);
    command(&mut app, LayoutCommand::Close(None));
    command(&mut app, LayoutCommand::Split { down: false });
    assert_eq!(layout(&app).preset, LayoutPreset::Quad);
    // A drag under way keeps its view: no renumbering meanwhile.
    let mut app = split(900, LayoutPreset::TwoColumns);
    mouse(&mut app, ButtonState::Pressed);
    command(&mut app, LayoutCommand::Close(None));
    assert_eq!(layout(&app).preset, LayoutPreset::TwoColumns);
    mouse(&mut app, ButtonState::Released);
    command(&mut app, LayoutCommand::Close(None));
    assert_eq!(layout(&app).preset, LayoutPreset::Single);
}

#[test]
fn a_narrow_area_keeps_each_view_at_the_floor() {
    // 1024 wide with a 440 px sidebar: 504 px for Two columns at 3:2,
    // which would make one view 201 px.
    let mut app = shell_app((1024, 900), 1.0);
    frames(&mut app, 2);
    go_live(&mut app, true);
    {
        let mut shell = app.world_mut().resource_mut::<ShellLayout>();
        shell.startup = false;
        shell.sidebar_open = true;
        shell.sidebar_w = 440.0;
        shell.inspector_open = false;
    }
    command(&mut app, LayoutCommand::Preset(LayoutPreset::TwoColumns));
    assert!(!layout(&app).fallback);
    let n = nodes(&mut app);
    assert_eq!(n.len(), 2);
    for (_, _, (_, size)) in &n {
        assert!(size.x >= 240.0, "{size}");
    }
    // The share is kept for when there is room again.
    assert_eq!(layout(&app).shares()[0], 0.6);
}

#[test]
fn quads_columns_move_together_and_one_plus_twos_rows_split_its_right() {
    let mut app = split(900, LayoutPreset::Quad);
    drag_divider(&mut app, 0, 60.0);
    let n = nodes(&mut app);
    assert_eq!(n[0].2 .1.x, n[2].2 .1.x, "both rows' columns");
    assert!(n[0].2 .1.x > n[1].2 .1.x);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::OnePlusTwo));
    drag_divider(&mut app, 1, -100.0);
    let n = nodes(&mut app);
    assert!(n[1].2 .1.y < n[2].2 .1.y);
    assert_eq!(n[0].2 .1.y, AREA.1.y);
}

#[test]
fn a_layout_change_shows_its_views_images_at_once() {
    let mut app = split(900, LayoutPreset::Quad);
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    app.world_mut()
        .write_message(LayoutCommand::Preset(LayoutPreset::Quad));
    app.update();
    let world = app.world_mut();
    let shown = world
        .query_filtered::<(), (With<ViewNode>, With<ImageNode>)>()
        .iter(world)
        .count();
    assert_eq!(shown, 4);
}

#[test]
fn an_inactive_view_shows_its_face_marker_until_hovered() {
    use crate::shell::viewport::rail::Rail;
    use crate::shell::viewport::view_cube::ViewCube;
    use gz_bevy::view::{ViewCommand, ViewOp};
    let mut app = split(900, LayoutPreset::TwoColumns);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(60),
    ));
    frames(&mut app, 3);
    let n = nodes(&mut app);
    // (rail shown, cube shown, marker shown, marker text) for a view.
    let look = |app: &mut App, view: Entity| {
        let world = app.world_mut();
        let mut kids =
            world.query::<(&ChildOf, &Node, Has<Rail>, Has<ViewCube>, Has<FaceMarker>)>();
        let mut out = (false, false, false);
        for (p, node, rail, cube, marker) in kids.iter(world) {
            if p.parent() != view {
                continue;
            }
            let shown = node.display != Display::None;
            if rail {
                out.0 = shown;
            }
            if cube {
                out.1 = shown;
            }
            if marker {
                out.2 = shown;
            }
        }
        out
    };
    // View 0 is active: in full. View 1: its marker.
    assert_eq!(look(&mut app, n[0].1), (true, true, false));
    assert_eq!(look(&mut app, n[1].1), (false, false, true));
    // Hovering view 1 brings its cube and rail back within 100 ms.
    let right = n[1].2 .0 + n[1].2 .1 / 2.0;
    cursor(&mut app, right);
    frames(&mut app, 2);
    assert_eq!(look(&mut app, n[1].1), (true, true, false));
    // The marker reads the face the view looks at: a top view's TOP.
    let cam = camera(&mut app, 0);
    app.world_mut().write_message(ViewCommand {
        view: cam,
        op: ViewOp::Snap(gz_bevy::view::SnapView::Top),
    });
    cursor(&mut app, n[1].2 .0 + Vec2::new(5.0, 300.0));
    frames(&mut app, 8);
    app.world_mut().resource_mut::<ViewRouting>().active = Some(ViewId(1));
    frames(&mut app, 3);
    assert_eq!(look(&mut app, n[0].1), (false, false, true));
    let world = app.world_mut();
    let labels: Vec<(Entity, String)> = world
        .query_filtered::<(Entity, &Text), With<FaceMarkerLabel>>()
        .iter(world)
        .map(|(e, t)| (e, t.0.clone()))
        .collect();
    let in0 = labels
        .iter()
        .find(|(e, _)| {
            let world = app.world();
            let mut at = *e;
            while let Some(p) = world.get::<ChildOf>(at) {
                at = p.parent();
                if at == n[0].1 {
                    return true;
                }
            }
            false
        })
        .unwrap();
    assert_eq!(in0.1, "TOP");
    // One view: always in full.
    command(&mut app, LayoutCommand::Preset(LayoutPreset::Single));
    let n = nodes(&mut app);
    assert_eq!(look(&mut app, n[0].1), (true, true, false));
}
