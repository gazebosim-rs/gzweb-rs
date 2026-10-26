//! The view cube: its maths (regions, picking, shading, labels), then
//! picking through its image in the full UI: hover, click, drag, the
//! texture's size and the cube camera following the view.

use bevy::picking::backend::HitData;
use bevy::picking::events::{
    Pointer, PointerClick, PointerDrag, PointerDragStart, PointerMove, PointerOut, PointerPress,
};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use gz_bevy::camera::MainCamera;
use gz_bevy::view::{home_transform, SnapView, ViewCommand, ViewOp};
use std::time::Duration;

use super::pick::*;
use super::scene::{patch_role, CubeInk, CubePatch, CubeScene};
use super::systems::{cube_uv, CubeDrag, CubeHover, CubePointerAt, CLICK_SLOP};
use super::*;
use crate::test_support::frames;
use crate::test_support::shell::*;
use crate::theme::{ColorRole, Theme, ThemeMode};

// The maths.

fn top_rotation() -> Quat {
    gz_rotation(SnapView::Top.rotation())
}

fn home_rotation() -> Quat {
    gz_rotation(home_transform().rotation)
}

#[test]
fn faces_snap_to_the_axis_view_that_looks_at_them() {
    for face in SnapView::ALL {
        let region = CubeRegion::of_snap(face);
        assert_eq!(region.snap(), Some(face));
        assert_eq!(region.op(), ViewOp::Snap(face));
        // The view looks at the face from outside it.
        let (dir, _) = face.gz_axes();
        assert_eq!(region.0.as_vec3(), -dir);
    }
    assert_eq!(CubeRegion(IVec3::new(1, 0, 1)).snap(), None);
    assert_eq!(
        CubeRegion(IVec3::new(1, -1, 1)).op(),
        ViewOp::LookFrom(Vec3::new(1.0, -1.0, 1.0))
    );
    let labels: Vec<&str> = CubeRegion::FACES.iter().map(|f| face_label(*f)).collect();
    assert_eq!(labels, ["TOP", "BTM", "E", "W", "N", "S"]);
}

#[test]
fn picking_finds_faces_edges_and_corners() {
    // Looking straight down: the middle is the top face, the image's edge
    // midpoints the top's edges, its corners the top's corners. Top view
    // has +X up the screen (D-13), so up is +X, right is -Y.
    let rot = top_rotation();
    // The top face's half side on the image: its edge is 1 from the
    // middle, at the near face's depth.
    let side = 1.0 / ((CAMERA_DISTANCE - 1.0) * tan_half_fov());
    assert!((project(rot, Vec3::new(1.0, 0.0, 1.0)).0.y - side).abs() < 1e-5);
    assert_eq!(pick(rot, Vec2::ZERO), Some(CubeRegion(IVec3::Z)));
    assert_eq!(
        pick(rot, Vec2::new(0.0, 0.95 * side)),
        Some(CubeRegion(IVec3::new(1, 0, 1)))
    );
    assert_eq!(
        pick(rot, Vec2::new(0.95 * side, 0.95 * side)),
        Some(CubeRegion(IVec3::new(1, -1, 1)))
    );
    // Off the cube.
    assert_eq!(pick(rot, Vec2::new(1.05 * side, 0.0)), None);
    assert_eq!(pick(rot, Vec2::new(0.99, 0.99)), None);
    // From gzweb's home view (along +X, 45° down) the middle of the image
    // is the edge between the top and the west face.
    let home = home_rotation();
    assert_eq!(
        pick(home, Vec2::ZERO),
        Some(CubeRegion(IVec3::new(-1, 0, 1)))
    );
    assert_eq!(pick(home, Vec2::new(0.0, 0.4)), Some(CubeRegion(IVec3::Z)));
    assert_eq!(
        pick(home, Vec2::new(0.0, -0.4)),
        Some(CubeRegion(IVec3::NEG_X))
    );
}

/// Views with no roll, as the view's camera turns (yaw, then elevation
/// from straight down to straight up), and the corner views.
fn views() -> Vec<Quat> {
    let mut out = vec![top_rotation(), home_rotation()];
    for yaw in 0..24 {
        for el in -6..=6 {
            let (yaw, el) = (
                yaw as f32 * 15f32.to_radians(),
                el as f32 * 15f32.to_radians(),
            );
            let eye = Vec3::new(el.cos() * yaw.cos(), el.cos() * yaw.sin(), el.sin());
            let up = if el.abs() > 1.5 { Vec3::X } else { Vec3::Z };
            out.push(
                Transform::from_translation(eye)
                    .looking_at(Vec3::ZERO, up)
                    .rotation,
            );
        }
    }
    for c in [
        Vec3::ONE,
        Vec3::new(1.0, -1.0, 1.0),
        Vec3::new(-1.0, 1.0, -1.0),
    ] {
        out.push(
            Transform::from_translation(c)
                .looking_at(Vec3::ZERO, Vec3::Z)
                .rotation,
        );
    }
    out
}

#[test]
fn the_cube_fills_the_image_less_a_margin_from_any_view() {
    // Every corner, edges included, stays inside the margin from every
    // view: the cube is never clipped.
    let r = 1.0 + EDGE / 2.0;
    let corners: Vec<Vec3> = (0..8)
        .map(|i| {
            Vec3::new(r, r, r)
                * Vec3::select(
                    BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                    Vec3::ONE,
                    Vec3::NEG_ONE,
                )
        })
        .collect();
    let extent = |rot: Quat| {
        corners
            .iter()
            .map(|c| project(rot, *c).0.abs().max_element())
            .fold(0.0, f32::max)
    };
    for rot in views() {
        assert!(extent(rot) <= FILL + 1e-4, "{rot}");
    }
    // A corner view (the board's) fills it; a face on is well over half
    // of it (it was 30 of 56 px from the orthographic camera's framing).
    let corner = Transform::from_translation(Vec3::ONE)
        .looking_at(Vec3::ZERO, Vec3::Z)
        .rotation;
    assert!(extent(corner) > 0.95 * FILL, "{}", extent(corner));
    let face = extent(top_rotation());
    assert!(face > 0.6 && face < 0.7, "{face}");
    let home = extent(home_rotation());
    assert!(home > 0.75, "{home}");
    // From home the top and the west face both show, and the near edge
    // is wider than the far ones (the perspective).
    let home = home_rotation();
    let near = project(home, Vec3::new(-1.0, 1.0, 1.0)).0.x.abs();
    let far = project(home, Vec3::new(1.0, 1.0, 1.0)).0.x.abs();
    assert!(near > 1.2 * far, "{near} {far}");
}

#[test]
fn picking_finds_what_the_image_shows() {
    // Pick undoes the projection: the middle of every patch the camera
    // sees picks that patch's region.
    for rot in views() {
        let camera = rot * Vec3::Z * CAMERA_DISTANCE;
        for n in CubeRegion::FACES {
            if n.as_vec3().dot(camera - n.as_vec3()) <= 1e-3 {
                continue;
            }
            for (region, c) in face_patches(n) {
                let mid = (c[0] + c[2]) / 2.0;
                assert_eq!(pick(rot, project(rot, mid).0), Some(region), "{rot} {mid}");
            }
        }
    }
}

#[test]
fn regions_split_each_face_three_by_three() {
    let edge = 1.0 - BAND;
    assert_eq!(region_at(Vec3::new(0.0, 0.0, 1.0)), CubeRegion(IVec3::Z));
    assert_eq!(
        region_at(Vec3::new(edge + 0.01, 0.0, 1.0)),
        CubeRegion(IVec3::new(1, 0, 1))
    );
    assert_eq!(
        region_at(Vec3::new(-1.0, -edge - 0.01, edge + 0.01)),
        CubeRegion(IVec3::new(-1, -1, 1))
    );
    for n in CubeRegion::FACES {
        let patches = face_patches(n);
        assert_eq!(patches.len(), 9);
        for (region, c) in patches {
            // Facing out, on the face, and in the region its middle is.
            let normal = (c[1] - c[0]).cross(c[2] - c[0]).normalize();
            assert!((normal - n.as_vec3()).length() < 1e-5, "{n}");
            let mid = (c[0] + c[2]) / 2.0;
            assert_eq!(region_at(mid), region, "{n} {mid}");
        }
    }
}

#[test]
fn faces_shade_by_where_they_face_and_labels_sit_on_them() {
    let home = home_rotation();
    assert_eq!(shade(home, Vec3::Z), Shade::Top);
    // The west face, squarely facing it, in the darker side shade.
    assert_eq!(shade(home, Vec3::NEG_X), Shade::Left);
    // Turned 45° about Z from home, two sides show: one left, one right.
    let turned = Quat::from_rotation_z(std::f32::consts::FRAC_PI_4) * home;
    let a = shade(turned, Vec3::NEG_X);
    let b = shade(turned, Vec3::NEG_Y);
    assert_ne!(a, b);
    assert!(matches!(a, Shade::Left | Shade::Right));
    // Labels: the top's lands above the middle and faces the camera; the
    // bottom faces away.
    let (at, facing) = label_at(home, Vec3::Z, 56.0);
    assert!(at.y < 28.0 && (at.x - 28.0).abs() < 1e-3 && facing > 0.5);
    assert!(label_at(home, Vec3::NEG_Z, 56.0).1 < 0.0);
    // A straight-down view centres the top's label.
    let (at, facing) = label_at(top_rotation(), Vec3::Z, 56.0);
    assert!((at - Vec2::splat(28.0)).length() < 1e-3 && facing > 0.99);
}

#[test]
fn patches_show_hover_then_the_snapped_face_then_their_shade() {
    let patch = CubePatch {
        region: CubeRegion(IVec3::Z),
        face: IVec3::Z,
        role: None,
    };
    let home = home_rotation();
    let top = Some(CubeRegion(IVec3::Z));
    assert_eq!(patch_role(&patch, home, top, None), ColorRole::AccentRing);
    assert_eq!(patch_role(&patch, home, None, top), ColorRole::CubeSnapped);
    assert_eq!(patch_role(&patch, home, None, None), ColorRole::CubeTop);
    let side = CubePatch {
        region: CubeRegion(IVec3::NEG_X),
        face: IVec3::NEG_X,
        role: None,
    };
    assert_eq!(patch_role(&side, home, top, top), ColorRole::CubeLeft);
}

#[test]
fn image_coordinates_run_minus_one_to_one_with_y_up() {
    let rect = Rect::new(100.0, 50.0, 212.0, 162.0);
    // Window scale 2: logical 78, 53 is physical 156, 106, the middle.
    assert_eq!(cube_uv(Vec2::new(78.0, 53.0), rect, 2.0), Vec2::ZERO);
    assert_eq!(
        cube_uv(Vec2::new(50.0, 25.0), rect, 2.0),
        Vec2::new(-1.0, 1.0)
    );
}

#[test]
fn snaps_animate_per_the_motion_tokens() {
    use crate::theme::motion;
    assert_eq!(
        gz_bevy::view::ANIMATION_SECS * 1000.0,
        motion::CAMERA_SNAP_MS
    );
    assert_eq!(motion::EASING, "cubic-out");
}

// Through the full UI.

#[derive(Resource, Default)]
struct Ops(Vec<ViewOp>);

fn record(mut reader: MessageReader<ViewCommand>, mut ops: ResMut<Ops>) {
    ops.0.extend(reader.read().map(|c| c.op));
}

/// The (first) cube's scene.
fn cube_scene(app: &mut App) -> CubeScene {
    let cube = find::<ViewCube>(app);
    app.world().get::<CubeScene>(cube).expect("a scene").clone()
}

/// What the (first) cube lights up.
fn cube_hover(app: &mut App) -> Option<CubeRegion> {
    let cube = find::<ViewCube>(app);
    app.world().get::<CubeHover>(cube).unwrap().0
}

fn cube_app(scale: f32) -> App {
    let mut app = if scale == 1.0 {
        live_view(900)
    } else {
        let mut app = shell_app((1440, 900), scale);
        frames(&mut app, 2);
        go_live(&mut app, true);
        frames(&mut app, 2);
        app
    };
    app.init_resource::<Ops>().add_systems(Last, record);
    app
}

fn ops(app: &mut App) -> Vec<ViewOp> {
    std::mem::take(&mut app.world_mut().resource_mut::<Ops>().0)
}

/// The window position (logical) of image point `uv` on the cube.
/// Where `uv` is on the cube, in window pixels (the pointer's): UI pixels
/// times the UI scale.
fn at_uv(app: &mut App, uv: Vec2) -> Vec2 {
    let cube = find::<ViewCube>(app);
    let (min, size) = rect(app, cube);
    let scale = app.world().resource::<UiScale>().0;
    (min + Vec2::new((uv.x + 1.0) / 2.0, (1.0 - uv.y) / 2.0) * size) * scale
}

fn pointer_at(p: Vec2) -> Pointer {
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: p,
        },
    )
}

fn hit() -> HitData {
    HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
}

/// A press and release in place at `uv`.
fn click_cube(app: &mut App, uv: Vec2) {
    let entity = find::<ViewCube>(app);
    let p = at_uv(app, uv);
    app.world_mut().trigger(PointerPress {
        entity,
        pointer: pointer_at(p),
        button: PointerButton::Primary,
        hit: hit(),
        count: 1,
    });
    release_on_cube(app, uv);
}

/// The click a release at `uv` sends (after a press, or the drag it ends).
fn release_on_cube(app: &mut App, uv: Vec2) {
    let entity = find::<ViewCube>(app);
    let p = at_uv(app, uv);
    app.world_mut().trigger(PointerClick {
        entity,
        pointer: pointer_at(p),
        button: PointerButton::Primary,
        hit: hit(),
        duration: Duration::from_millis(80),
        count: 1,
    });
    frames(app, 1);
}

#[test]
fn clicking_the_cube_snaps_or_turns_the_view() {
    // From home (a fresh view each time: a click starts an animation):
    // the upper middle of the image is the top face, the middle the edge
    // between the top and the west face, a corner of the image nothing.
    for (uv, want) in [
        (Vec2::new(0.0, 0.4), Some(ViewOp::Snap(SnapView::Top))),
        (
            Vec2::ZERO,
            Some(ViewOp::LookFrom(Vec3::new(-1.0, 0.0, 1.0))),
        ),
        (Vec2::new(0.95, 0.95), None),
    ] {
        let mut app = cube_app(1.0);
        click_cube(&mut app, uv);
        assert_eq!(ops(&mut app), want.into_iter().collect::<Vec<_>>(), "{uv}");
    }
}

#[test]
fn the_cube_picks_at_any_scale_factor() {
    let mut app = cube_app(2.0);
    click_cube(&mut app, Vec2::new(0.0, -0.4));
    assert_eq!(ops(&mut app), [ViewOp::Snap(SnapView::Back)]);
    // Its texture is the node's physical size.
    let scene = cube_scene(&mut app);
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(&scene.image)
        .unwrap()
        .size();
    assert_eq!(size, UVec2::splat(112));
    let image = find::<CubeImage>(&mut app);
    assert_eq!(
        app.world().get::<ImageNode>(image).map(|i| i.image.clone()),
        Some(scene.image.clone())
    );
    let target = app.world().get::<bevy::camera::RenderTarget>(scene.camera);
    assert!(matches!(target, Some(bevy::camera::RenderTarget::Image(t)) if t.scale_factor == 2.0));
}

/// After a UI scale change the cube still picks under the pointer, and
/// its texture follows its node's new physical size (D-61, D-118).
#[test]
fn the_cube_picks_and_stays_sharp_after_a_ui_scale_change() {
    let mut app = cube_app(1.0);
    app.world_mut()
        .write_message(crate::shell::UiCommand::SetUiScale(2.0));
    frames(&mut app, 3);
    click_cube(&mut app, Vec2::new(0.0, -0.4));
    assert_eq!(ops(&mut app), [ViewOp::Snap(SnapView::Back)]);
    let scene = cube_scene(&mut app);
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(&scene.image)
        .unwrap()
        .size();
    assert_eq!(size, UVec2::splat(112));
}

#[test]
fn hovering_lights_the_region_and_dragging_orbits() {
    let mut app = cube_app(1.0);
    let entity = find::<ViewCube>(&mut app);
    let p = at_uv(&mut app, Vec2::new(0.0, 0.4));
    app.world_mut().trigger(PointerMove {
        entity,
        pointer: pointer_at(p),
        hit: hit(),
        delta: Vec2::ONE,
    });
    frames(&mut app, 2);
    assert_eq!(cube_hover(&mut app), Some(CubeRegion(IVec3::Z)));
    // The top's middle patch is lit.
    let accent = app.world().resource::<Theme>().color(ColorRole::AccentRing);
    let world = app.world_mut();
    let lit: Vec<Color> = world
        .query::<(&CubePatch, &MeshMaterial3d<StandardMaterial>)>()
        .iter(world)
        .filter(|(p, _)| p.region == CubeRegion(IVec3::Z))
        .map(|(_, m)| m.0.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| {
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&h)
                .unwrap()
                .base_color
        })
        .collect();
    assert_eq!(lit, [accent]);
    app.world_mut().trigger(PointerOut {
        entity,
        pointer: pointer_at(p),
        hit: hit(),
    });
    frames(&mut app, 1);
    assert_eq!(cube_hover(&mut app), None);
    // A drag orbits, and the click that ends it picks nothing.
    app.world_mut().trigger(PointerDragStart {
        entity,
        pointer: pointer_at(p),
        button: PointerButton::Primary,
        hit: hit(),
    });
    for _ in 0..3 {
        app.world_mut().trigger(PointerDrag {
            entity,
            pointer: pointer_at(p),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
            delta: Vec2::new(2.0, 0.0),
        });
    }
    frames(&mut app, 1);
    release_on_cube(&mut app, Vec2::new(0.0, 0.4));
    // The first 2 px are within the click slop.
    assert_eq!(
        ops(&mut app),
        [
            ViewOp::Orbit(Vec2::new(2.0, 0.0)),
            ViewOp::Orbit(Vec2::new(2.0, 0.0))
        ]
    );
    assert_eq!(app.world().resource::<CubeDrag>().0, 0.0);
}

#[test]
fn the_cube_turns_with_the_view_and_labels_its_facing_faces() {
    let mut app = cube_app(1.0);
    let scene = cube_scene(&mut app);
    let world = app.world_mut();
    let main = world
        .query_filtered::<&Transform, With<MainCamera>>()
        .single(world)
        .unwrap()
        .rotation;
    let cam = *app.world().get::<Transform>(scene.camera).unwrap();
    assert!(cam.rotation.angle_between(main) < 1e-5);
    let shown = |app: &mut App| {
        let world = app.world_mut();
        let mut v: Vec<&'static str> = world
            .query::<(&CubeLabel, &Node)>()
            .iter(world)
            .filter(|(_, n)| n.display != Display::None)
            .map(|(l, _)| face_label(l.0))
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(shown(&mut app), ["TOP", "W"]);
    // A top view: only TOP, and the top face is the snapped one.
    send(&mut app, ViewOp::Snap(SnapView::Top));
    // Land the animation.
    send(&mut app, ViewOp::Pan(Vec2::ZERO));
    frames(&mut app, 2);
    assert_eq!(shown(&mut app), ["TOP"]);
    let cam = *app.world().get::<Transform>(scene.camera).unwrap();
    assert!(cam.rotation.angle_between(SnapView::Top.rotation()) < 1e-4);
    let snapped = app
        .world()
        .resource::<Theme>()
        .color(ColorRole::CubeSnapped);
    let world = app.world_mut();
    let handle = world
        .query::<(&CubePatch, &MeshMaterial3d<StandardMaterial>)>()
        .iter(world)
        .find(|(p, _)| p.face == IVec3::Z && p.region != CubeRegion(IVec3::Z))
        .map(|(_, m)| m.0.clone())
        .unwrap();
    let c = app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&handle)
        .unwrap()
        .base_color;
    assert_eq!(c, snapped);
    // The cube stays on its own layer and out of the views' picks.
    let world = app.world_mut();
    let parts = world
        .query_filtered::<&bevy::camera::visibility::RenderLayers, With<Mesh3d>>()
        .iter(world)
        .count();
    assert_eq!(parts, 6 * 9 + 12);
}

#[test]
fn the_cube_casts_its_own_shadow_a_pixel_down_in_either_theme() {
    let mut app = cube_app(1.0);
    let scene = cube_scene(&mut app);
    let (cube, image, shadow) = (
        find::<ViewCube>(&mut app),
        find::<CubeImage>(&mut app),
        find::<CubeShadow>(&mut app),
    );
    // Under the image, the same texture, a pixel lower, the cube's size.
    let children: Vec<Entity> = app.world().get::<Children>(cube).unwrap().to_vec();
    assert_eq!(children[..2], [shadow, image]);
    let ((c, cs), (i, is), (s, ss)) = (rect(&app, cube), rect(&app, image), rect(&app, shadow));
    assert_eq!((i, is), (c, cs));
    assert_eq!((s - c, ss), (Vec2::new(0.0, 1.0), cs));
    let tint = |app: &App, e| {
        app.world()
            .get::<ImageNode>(e)
            .map(|n| (n.image.clone(), n.color))
    };
    assert_eq!(
        tint(&app, image),
        Some((scene.image.clone(), ImageNode::default().color))
    );
    for dark in [false, true] {
        let mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        app.world_mut().resource_mut::<Theme>().set_mode(mode);
        frames(&mut app, 2);
        let want = app.world().resource::<Theme>().color(ColorRole::Shadow);
        assert_eq!(
            tint(&app, shadow),
            Some((scene.image.clone(), want)),
            "{mode:?}"
        );
        // The edges follow the theme too.
        let edge = app.world().resource::<Theme>().color(ColorRole::CubeEdge);
        let world = app.world_mut();
        let handle = world
            .query::<(&CubeInk, &MeshMaterial3d<StandardMaterial>)>()
            .iter(world)
            .next()
            .map(|(_, m)| m.0.clone())
            .unwrap();
        let got = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .base_color;
        assert_eq!(got, edge);
    }
}

/// The cube camera's transform is final in the frame the view turns, so the
/// cube is drawn with the view's own rotation, not the last frame's.
#[test]
fn the_cube_turns_in_the_frame_the_view_does() {
    let mut app = cube_app(1.0);
    let camera = cube_scene(&mut app).camera;
    for _ in 0..5 {
        send(&mut app, ViewOp::Orbit(Vec2::new(15.0, 3.0)));
        let world = app.world_mut();
        let view = *world
            .query_filtered::<&Transform, With<MainCamera>>()
            .single(world)
            .unwrap();
        let drawn = app.world().get::<GlobalTransform>(camera).unwrap();
        // A frame behind is a whole orbit step (about 0.1 rad); f32 leaves
        // about 1e-3.
        assert!(drawn.rotation().angle_between(view.rotation) < 0.01);
    }
}

/// The hover lights what a click would pick, also after the view turns
/// under a pointer that hasn't moved; leaving the cube clears it.
#[test]
fn the_hover_follows_the_view_under_a_still_pointer() {
    let mut app = cube_app(1.0);
    let entity = find::<ViewCube>(&mut app);
    let p = at_uv(&mut app, Vec2::ZERO);
    app.world_mut().trigger(PointerMove {
        entity,
        pointer: pointer_at(p),
        hit: hit(),
        delta: Vec2::ONE,
    });
    frames(&mut app, 1);
    assert_eq!(cube_hover(&mut app), pick(home_rotation(), Vec2::ZERO));
    send(&mut app, ViewOp::Snap(SnapView::Top));
    frames(&mut app, 30);
    let want = pick(top_rotation(), Vec2::ZERO);
    assert_ne!(want, pick(home_rotation(), Vec2::ZERO));
    assert_eq!(cube_hover(&mut app), want);
    app.world_mut().trigger(PointerOut {
        entity,
        pointer: pointer_at(p),
        hit: hit(),
    });
    frames(&mut app, 1);
    assert_eq!(cube_hover(&mut app), None);
    let cube = find::<ViewCube>(&mut app);
    assert_eq!(app.world().get::<CubePointerAt>(cube).unwrap().0, None);
}

/// A drag that starts on the cube and is released off it (no click on the
/// cube to end it) leaves the next click in place to pick.
#[test]
fn a_drag_released_off_the_cube_leaves_the_next_click_alone() {
    let mut app = cube_app(1.0);
    let entity = find::<ViewCube>(&mut app);
    let p = at_uv(&mut app, Vec2::new(0.0, 0.4));
    app.world_mut().trigger(PointerDragStart {
        entity,
        pointer: pointer_at(p),
        button: PointerButton::Primary,
        hit: hit(),
    });
    for _ in 0..5 {
        app.world_mut().trigger(PointerDrag {
            entity,
            pointer: pointer_at(p),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
            delta: Vec2::new(4.0, 0.0),
        });
    }
    assert!(app.world().resource::<CubeDrag>().0 > CLICK_SLOP);
    frames(&mut app, 30);
    ops(&mut app);
    click_cube(&mut app, Vec2::new(0.0, 0.4));
    assert_eq!(ops(&mut app), [ViewOp::Snap(SnapView::Top)]);
}
